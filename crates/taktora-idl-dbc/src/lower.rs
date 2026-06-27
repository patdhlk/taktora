//! Lowering: project a [`DbcDatabase`] onto the message IR + a layout sidecar.

use taktora_idl_core::{
    EnumDef, EnumVariant, Field, IrError, Module, Scalar, Struct, Type, TypeName,
};

use crate::{
    ast::{DbcDatabase, DbcMessage, DbcSignal},
    layout::{DbcLayout, FrameLayout, SignalLayout},
};

/// The two coupled outputs of lowering a DBC file.
#[derive(Debug, Clone, PartialEq)]
pub struct LoweredDbc {
    /// The plane-generic logical message IR.
    pub module: Module,
    /// The DBC-specific physical CAN layout, keyed by the module's names.
    pub layout: DbcLayout,
}

/// Why a DBC database could not be lowered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LowerError {
    /// A signal is wider than any supported integer scalar (> 64 bits) or has
    /// zero width.
    #[error("signal `{message}.{signal}` has unsupported width {bits} bits")]
    SignalTooWide {
        /// Message name.
        message: String,
        /// Signal name.
        signal: String,
        /// The offending bit width.
        bits: u16,
    },
    /// The lowered module failed [`Module::validate`]. DBC is bounded by
    /// construction, so this indicates a name clash (e.g. two messages or two
    /// signals colliding after projection), not unboundedness.
    #[error("lowered module failed validation: {0}")]
    Ir(#[from] IrError),
}

/// Lower a parsed DBC database into the message IR plus its CAN layout sidecar.
///
/// `module_name` names the resulting [`Module`] (typically the `.dbc` file
/// stem). The returned module is already [validated](Module::validate).
///
/// Each message becomes a struct; each signal a field whose logical type is the
/// narrowest integer [`Scalar`] holding its raw bits — or an enum when the
/// signal has a `VAL_` value table. The `factor`/`offset` scaling and bit
/// placement are *not* in the module; they ride in the [`DbcLayout`].
///
/// # Errors
///
/// [`LowerError::SignalTooWide`] for a signal outside `1..=64` bits, or
/// [`LowerError::Ir`] if the projected module does not validate.
pub fn lower(db: &DbcDatabase, module_name: impl Into<String>) -> Result<LoweredDbc, LowerError> {
    let mut module = Module::new(module_name);
    let mut layout = DbcLayout::default();

    for msg in &db.messages {
        let mut fields = Vec::with_capacity(msg.signals.len());
        let mut signal_layouts = Vec::with_capacity(msg.signals.len());

        for sig in &msg.signals {
            let scalar = integer_scalar(msg, sig)?;
            let ty = match db.value_table_for(msg.id, &sig.name) {
                Some(table) => {
                    let enum_name = format!("{}_{}", msg.name, sig.name);
                    module.enums.push(EnumDef {
                        name: enum_name.clone(),
                        underlying: scalar,
                        variants: table
                            .entries
                            .iter()
                            .map(|(value, name)| EnumVariant {
                                name: name.clone(),
                                value: *value,
                            })
                            .collect(),
                    });
                    Type::Enum(TypeName(enum_name))
                }
                None => Type::Scalar(scalar),
            };
            fields.push(Field::new(sig.name.clone(), ty));
            signal_layouts.push(SignalLayout {
                field_name: sig.name.clone(),
                start_bit: sig.start_bit,
                bit_len: sig.bit_len,
                byte_order: sig.byte_order,
                signed: sig.signed,
                factor: sig.factor,
                offset: sig.offset,
                min: sig.min,
                max: sig.max,
                unit: sig.unit.clone(),
            });
        }

        module.structs.push(Struct {
            name: msg.name.clone(),
            fields,
        });
        layout.frames.push(FrameLayout {
            struct_name: msg.name.clone(),
            can_id: msg.can_id(),
            extended: msg.is_extended(),
            dlc: msg.dlc,
            signals: signal_layouts,
        });
    }

    module.validate()?;
    Ok(LoweredDbc { module, layout })
}

fn integer_scalar(msg: &DbcMessage, sig: &DbcSignal) -> Result<Scalar, LowerError> {
    Scalar::integer_for_bits(sig.bit_len, sig.signed).ok_or_else(|| LowerError::SignalTooWide {
        message: msg.name.clone(),
        signal: sig.name.clone(),
        bits: sig.bit_len,
    })
}
