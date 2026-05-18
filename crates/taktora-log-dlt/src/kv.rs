//! Map `log::kv::Source` pairs to DLT verbose [`Argument`]s.
//!
//! Native type mapping (REQ_0809):
//! * `u32` / `u64` / `i32` / `i64` → `Value::U32` / `U64` / `I32` / `I64`
//! * `f32` / `f64`                → `Value::F32` / `F64`
//! * `bool`                       → `Value::Bool`
//! * `&str`                       → `Value::StringVal`
//! * everything else              → rendered via `Display` and emitted
//!   as a verbose `StringVal`.

use dlt_core::dlt::Argument;
use log::kv::{Key, Value, VisitSource};

use crate::encode::{
    bool_argument, display_string_argument, float_argument, signed_argument, unsigned_argument,
};

pub(crate) fn collect_arguments(record: &log::Record<'_>, into: &mut Vec<Argument>) {
    let mut v = Collector { out: into };
    let _ = record.key_values().visit(&mut v);
}

struct Collector<'a> {
    out: &'a mut Vec<Argument>,
}

impl<'kvs> VisitSource<'kvs> for Collector<'_> {
    fn visit_pair(&mut self, _key: Key<'kvs>, value: Value<'kvs>) -> Result<(), log::kv::Error> {
        // Check bool first: in the non-value-bag implementation, bool is
        // stored as `Inner::Bool` and is the only type that responds to
        // `to_bool()`. Checking it before numeric conversions avoids
        // ambiguity if an implementation were to coerce bool→int.
        if let Some(b) = value.to_bool() {
            self.out.push(bool_argument(b));
            return Ok(());
        }
        // Check unsigned before signed: a `u32` value is stored as
        // `Inner::U64` which also satisfies `to_i64()` (since 7 fits in
        // i64). Checking unsigned first ensures `u32(7)` maps to
        // `Value::U32(7)` rather than `Value::I32(7)`.
        if let Some(u) = value.to_u64() {
            if let Ok(u32v) = u32::try_from(u) {
                self.out.push(unsigned_argument(u32v));
                return Ok(());
            }
        }
        if let Some(i) = value.to_i64() {
            if let Ok(i32v) = i32::try_from(i) {
                self.out.push(signed_argument(i32v));
                return Ok(());
            }
        }
        if let Some(f) = value.to_f64() {
            self.out.push(float_argument(f));
            return Ok(());
        }
        // Fall back to Display for strings, chars, and any other type
        // that does not map to a native DLT numeric kind.
        self.out.push(display_string_argument(&format!("{value}")));
        Ok(())
    }
}
