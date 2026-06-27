//! The unit of resolution: a named set of structs, enums, and services.

use serde::{Deserialize, Serialize};

use crate::{EnumDef, IrError, Service, Struct, Type, ty::LENGTH_PREFIX_BYTES};

/// A self-contained group of message types.
///
/// Type references ([`Type::Struct`] / [`Type::Enum`] / [`Service`] payloads)
/// resolve by name *within one module*; there is no cross-module reference in
/// this slice. A module is the lowering target of one description unit (e.g.
/// one `.dbc` file).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Module {
    /// Module name (typically derived from the source file stem).
    pub name: String,
    /// Struct definitions.
    pub structs: Vec<Struct>,
    /// Enum definitions.
    pub enums: Vec<EnumDef>,
    /// Service definitions.
    pub services: Vec<Service>,
}

impl Module {
    /// Create an empty module with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Look up a struct by name.
    #[must_use]
    pub fn struct_by_name(&self, name: &str) -> Option<&Struct> {
        self.structs.iter().find(|s| s.name == name)
    }

    /// Look up an enum by name.
    #[must_use]
    pub fn enum_by_name(&self, name: &str) -> Option<&EnumDef> {
        self.enums.iter().find(|e| e.name == name)
    }

    /// Check the module for structural soundness:
    ///
    /// * no two structs or enums share a name,
    /// * every type reference resolves to a definition in this module,
    /// * no struct is recursive (every type has a finite serialized length).
    ///
    /// A module that passes `validate` is guaranteed to have a finite
    /// [`max_serialized_len`](Self::max_serialized_len) for every type it
    /// names — the boundedness contract this crate exists to enforce.
    ///
    /// # Errors
    ///
    /// Returns the first [`IrError`] encountered.
    pub fn validate(&self) -> Result<(), IrError> {
        self.check_unique_names()?;
        self.check_references()?;
        // Computing the max size of every struct exercises the recursion guard
        // and surfaces any cycle as a `RecursiveType` error.
        for s in &self.structs {
            self.struct_max_serialized_len(s)?;
        }
        for svc in &self.services {
            self.max_serialized_len(&Type::Struct(svc.request.clone()))
                .map_err(|e| reframe_unknown(e, &svc.name))?;
            if let Some(resp) = &svc.response {
                self.max_serialized_len(&Type::Struct(resp.clone()))
                    .map_err(|e| reframe_unknown(e, &svc.name))?;
            }
        }
        Ok(())
    }

    /// The upper bound, in bytes, on the serialized length of a value of `ty`.
    ///
    /// This is the buffer-sizing number: a `const N` envelope big enough to
    /// hold this many bytes can hold any value of `ty`, whatever the backend's
    /// exact framing. See [`LENGTH_PREFIX_BYTES`] for the framing assumption.
    ///
    /// # Errors
    ///
    /// [`IrError::UnknownType`] if `ty` (transitively) names a type the module
    /// does not define, or [`IrError::RecursiveType`] if it is recursive.
    ///
    /// [`LENGTH_PREFIX_BYTES`]: crate::LENGTH_PREFIX_BYTES
    pub fn max_serialized_len(&self, ty: &Type) -> Result<usize, IrError> {
        self.max_len_inner(ty, &mut Vec::new())
    }

    /// The upper bound on the serialized length of a struct value.
    ///
    /// # Errors
    ///
    /// As [`max_serialized_len`](Self::max_serialized_len).
    pub fn struct_max_serialized_len(&self, s: &Struct) -> Result<usize, IrError> {
        let mut stack = vec![s.name.clone()];
        self.struct_fields_len(s, &mut stack)
    }

    fn max_len_inner(&self, ty: &Type, stack: &mut Vec<String>) -> Result<usize, IrError> {
        match ty {
            Type::Scalar(s) => Ok(s.wire_size()),
            Type::String { capacity } => Ok(LENGTH_PREFIX_BYTES + capacity),
            Type::Array { element, len } => {
                Ok(self.max_len_inner(element, stack)?.saturating_mul(*len))
            }
            Type::Sequence { element, capacity } => Ok(LENGTH_PREFIX_BYTES
                + self
                    .max_len_inner(element, stack)?
                    .saturating_mul(*capacity)),
            Type::Enum(name) => self
                .enum_by_name(name.as_str())
                .map(|e| e.underlying.wire_size())
                .ok_or_else(|| unknown(stack, name.as_str())),
            Type::Struct(name) => {
                let s = self
                    .struct_by_name(name.as_str())
                    .ok_or_else(|| unknown(stack, name.as_str()))?;
                if stack.iter().any(|n| n == &s.name) {
                    let mut cycle = stack.clone();
                    cycle.push(s.name.clone());
                    return Err(IrError::RecursiveType { cycle });
                }
                stack.push(s.name.clone());
                let size = self.struct_fields_len(s, stack)?;
                stack.pop();
                Ok(size)
            }
        }
    }

    fn struct_fields_len(&self, s: &Struct, stack: &mut Vec<String>) -> Result<usize, IrError> {
        let mut total = 0usize;
        for f in &s.fields {
            total = total.saturating_add(self.max_len_inner(&f.ty, stack)?);
        }
        Ok(total)
    }

    fn check_unique_names(&self) -> Result<(), IrError> {
        let mut seen: Vec<&str> = Vec::new();
        for name in self
            .structs
            .iter()
            .map(|s| s.name.as_str())
            .chain(self.enums.iter().map(|e| e.name.as_str()))
        {
            if seen.contains(&name) {
                return Err(IrError::DuplicateType {
                    name: name.to_owned(),
                });
            }
            seen.push(name);
        }
        Ok(())
    }

    fn check_references(&self) -> Result<(), IrError> {
        for s in &self.structs {
            for f in &s.fields {
                self.check_type_ref(&f.ty, &s.name)?;
            }
        }
        for svc in &self.services {
            self.require_struct(svc.request.as_str(), &svc.name)?;
            if let Some(resp) = &svc.response {
                self.require_struct(resp.as_str(), &svc.name)?;
            }
        }
        Ok(())
    }

    fn check_type_ref(&self, ty: &Type, referrer: &str) -> Result<(), IrError> {
        match ty {
            Type::Scalar(_) | Type::String { .. } => Ok(()),
            Type::Array { element, .. } | Type::Sequence { element, .. } => {
                self.check_type_ref(element, referrer)
            }
            Type::Struct(name) => self.require_struct(name.as_str(), referrer),
            Type::Enum(name) => {
                if self.enum_by_name(name.as_str()).is_some() {
                    Ok(())
                } else {
                    Err(IrError::UnknownType {
                        referrer: referrer.to_owned(),
                        name: name.as_str().to_owned(),
                    })
                }
            }
        }
    }

    fn require_struct(&self, name: &str, referrer: &str) -> Result<(), IrError> {
        if self.struct_by_name(name).is_some() {
            Ok(())
        } else {
            Err(IrError::UnknownType {
                referrer: referrer.to_owned(),
                name: name.to_owned(),
            })
        }
    }
}

fn unknown(stack: &[String], name: &str) -> IrError {
    IrError::UnknownType {
        referrer: stack.last().cloned().unwrap_or_default(),
        name: name.to_owned(),
    }
}

fn reframe_unknown(err: IrError, service: &str) -> IrError {
    match err {
        IrError::UnknownType { name, .. } => IrError::UnknownType {
            referrer: service.to_owned(),
            name,
        },
        other => other,
    }
}
