//! Lowering: project a [`DbcDatabase`] onto the message IR + a layout sidecar.

use taktora_idl_core::{
    EnumDef, EnumVariant, Field, IrError, Module, Scalar, Struct, Type, TypeName,
};

use crate::{
    ast::{ByteOrder, DbcDatabase, DbcMessage, DbcSignal},
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
    /// A signal's bit range extends past the message's declared `DLC`, so a
    /// `DLC`-sized frame could never hold it. Caught here rather than failing
    /// every encode/decode at runtime against the frame bound.
    #[error("signal `{message}.{signal}` needs {needed} byte(s) but message DLC is {dlc}")]
    SignalExceedsFrame {
        /// Message name.
        message: String,
        /// Signal name.
        signal: String,
        /// The message's declared data length, in bytes.
        dlc: u8,
        /// Byte extent the signal actually requires.
        needed: usize,
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
            let ty = match db.value_table_for(msg.id, &sig.name) {
                Some(table) => {
                    // A value table with any negative entry must ride on a
                    // signed scalar, regardless of the signal's own sign flag,
                    // or the emitted `#[repr]` could not hold the discriminant.
                    let has_negative = table.entries.iter().any(|(value, _)| *value < 0);
                    let underlying = enum_scalar(msg, sig, has_negative)?;
                    let enum_name = format!("{}_{}", msg.name, sig.name);
                    module.enums.push(EnumDef {
                        name: enum_name.clone(),
                        underlying,
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
                None => Type::Scalar(integer_scalar(msg, sig)?),
            };
            // Width is now validated (1..=64); reject any signal that cannot
            // fit inside the frame the DLC promises.
            check_frame_fit(msg, sig)?;
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
    scalar_for(msg, sig, sig.signed)
}

/// The integer scalar an enum's discriminant rides on. A negative value table
/// forces a signed scalar even for an unsigned signal.
fn enum_scalar(
    msg: &DbcMessage,
    sig: &DbcSignal,
    has_negative: bool,
) -> Result<Scalar, LowerError> {
    scalar_for(msg, sig, sig.signed || has_negative)
}

fn scalar_for(msg: &DbcMessage, sig: &DbcSignal, signed: bool) -> Result<Scalar, LowerError> {
    Scalar::integer_for_bits(sig.bit_len, signed).ok_or_else(|| LowerError::SignalTooWide {
        message: msg.name.clone(),
        signal: sig.name.clone(),
        bits: sig.bit_len,
    })
}

/// Reject a signal whose bit range spills past the message's `DLC` bytes.
fn check_frame_fit(msg: &DbcMessage, sig: &DbcSignal) -> Result<(), LowerError> {
    let needed = signal_byte_extent(sig.start_bit, sig.bit_len, sig.byte_order);
    if needed > usize::from(msg.dlc) {
        return Err(LowerError::SignalExceedsFrame {
            message: msg.name.clone(),
            signal: sig.name.clone(),
            dlc: msg.dlc,
            needed,
        });
    }
    Ok(())
}

/// The number of frame bytes a signal occupies, by the same bit-numbering
/// conventions the wire runtime packs with (see `taktora-idl-wire`). `bit_len`
/// must be in `1..=64` (callers validate this first).
///
/// * Little-endian (Intel): bits ascend from `start_bit`, so the last bit is
///   `start_bit + bit_len - 1`.
/// * Big-endian (Motorola): the start bit is the MSB; less significant bits
///   walk *down* within the byte and jump to bit 7 of the next byte on
///   underflow, so the extent grows with each byte the descent crosses into.
fn signal_byte_extent(start_bit: u16, bit_len: u16, order: ByteOrder) -> usize {
    let start = usize::from(start_bit);
    let len = usize::from(bit_len);
    match order {
        ByteOrder::LittleEndian => (start + len - 1) / 8 + 1,
        ByteOrder::BigEndian => {
            let first_byte_bits = (start % 8) + 1; // MSB down to bit 0 of the start byte
            if len <= first_byte_bits {
                start / 8 + 1
            } else {
                start / 8 + (len - first_byte_bits).div_ceil(8) + 1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LowerError, lower, signal_byte_extent};
    use crate::{ast::ByteOrder, parse};

    #[test]
    fn byte_extent_matches_wire_conventions() {
        // Intel: 16-bit at bit 0 spans bytes 0..=1.
        assert_eq!(signal_byte_extent(0, 16, ByteOrder::LittleEndian), 2);
        // Intel: 8-bit at bit 16 spans up to byte 2.
        assert_eq!(signal_byte_extent(16, 8, ByteOrder::LittleEndian), 3);
        // Motorola: 16-bit with MSB at bit 7 spans bytes 0..=1.
        assert_eq!(signal_byte_extent(7, 16, ByteOrder::BigEndian), 2);
        // Motorola: a sub-byte signal stays within its start byte.
        assert_eq!(signal_byte_extent(4, 4, ByteOrder::BigEndian), 1);
    }

    #[test]
    fn signal_past_dlc_is_rejected() {
        // A 16-bit signal at bit 0 needs 2 bytes, but DLC is 1.
        let dbc = "BO_ 1 Tiny: 1 ECU\n SG_ Wide : 0|16@1+ (1,0) [0|0] \"\" Rx\n";
        let db = parse(dbc).unwrap();
        let err = lower(&db, "m").unwrap_err();
        assert!(matches!(
            err,
            LowerError::SignalExceedsFrame {
                needed: 2,
                dlc: 1,
                ..
            }
        ));
    }

    #[test]
    fn negative_value_table_lowers_to_signed_enum() {
        let dbc = "BO_ 7 Trans: 1 ECU\n SG_ Dir : 0|4@1- (1,0) [-8|7] \"\" Rx\n\
                   VAL_ 7 Dir -1 \"Reverse\" 0 \"Neutral\" 1 \"Forward\" ;\n";
        let db = parse(dbc).unwrap();
        let lowered = lower(&db, "m").unwrap();
        let dir = lowered.module.enum_by_name("Trans_Dir").unwrap();
        assert!(dir.underlying.is_signed_integer());
    }

    #[test]
    fn negative_table_on_unsigned_signal_still_picks_signed_repr() {
        // Unsigned signal (`@1+`) but a negative VAL_ entry: the enum repr must
        // be signed so the discriminant is representable.
        let dbc = "BO_ 8 Sw: 1 ECU\n SG_ State : 0|4@1+ (1,0) [0|0] \"\" Rx\n\
                   VAL_ 8 State -1 \"Fault\" 0 \"Ok\" ;\n";
        let db = parse(dbc).unwrap();
        let lowered = lower(&db, "m").unwrap();
        assert!(
            lowered
                .module
                .enum_by_name("Sw_State")
                .unwrap()
                .underlying
                .is_signed_integer()
        );
    }
}
