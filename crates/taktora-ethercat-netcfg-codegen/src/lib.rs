//! Code generator for the `taktora-ethercat-netcfg` IR.
//!
//! Turns a parsed [`taktora_ethercat_netcfg::NetworkConfig`] into a `String` of
//! formatted Rust source for the `taktora-connector-ethercat` runtime.
//! The runtime types are named **textually** so this crate never
//! depends on `taktora-connector-ethercat`.

#![warn(missing_docs)]

use std::collections::HashMap;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use taktora_ethercat_netcfg::{
    ChannelBinding, DeviceInstance, NetworkConfig, PdoDirection, PdoEntry, SdoValueSpec,
    StartupSdoSpec,
};

/// Errors that can occur while [`generate`]-ing Rust source.
#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    /// The internally generated token stream failed to parse back into a
    /// `syn::File`. This indicates a bug in the generator, not in the
    /// input config.
    #[error("generated token stream is not a valid Rust file: {0}")]
    Syn(#[from] syn::Error),
    /// The input config failed a build-time validation rule.
    #[error(transparent)]
    Validation(#[from] ValidationError),
}

/// A single build-time validation fault, one variant per rule.
///
/// Each variant carries enough context (channel name(s), device label,
/// values) for a clear [`Display`](std::fmt::Display) message.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// A channel binding declares a zero-bit slice.
    #[error("channel `{channel}` has a zero-length slice (bit_length == 0)")]
    ZeroLengthSlice {
        /// Name of the offending channel.
        channel: String,
    },
    /// A channel references a device label that is not declared.
    #[error("channel `{channel}` references unknown device `{device}`")]
    UnknownDevice {
        /// Name of the offending channel.
        channel: String,
        /// The unresolved device label.
        device: String,
    },
    /// Two channels share the same name.
    #[error("duplicate channel name `{name}`")]
    DuplicateChannelName {
        /// The repeated channel name.
        name: String,
    },
    /// Two devices resolve to the same configured station address.
    #[error("devices {labels:?} resolve to the same configured address {address:#06x}")]
    DuplicateAddress {
        /// The conflicting configured address.
        address: u16,
        /// Labels of the two devices that collide.
        labels: [String; 2],
    },
    /// A channel slice extends past the device's process image for its
    /// direction.
    #[error(
        "channel `{channel}` slice ends at bit {slice_end} but device process image is only {image_bits} bits"
    )]
    SliceOutOfImage {
        /// Name of the offending channel.
        channel: String,
        /// One past the last bit the slice covers (`bit_offset + bit_length`).
        slice_end: u32,
        /// Process-image size, in bits, for the channel's direction.
        image_bits: u32,
    },
    /// Two channels on the same device and direction cover intersecting
    /// bit ranges, and neither opted in via `allow_overlap`.
    #[error("channels `{a}` and `{b}` overlap on the same device and direction")]
    OverlappingSlices {
        /// Name of the first channel.
        a: String,
        /// Name of the second channel.
        b: String,
    },
}

/// A non-fatal build-time diagnostic.
///
/// Warnings are surfaced to the user (a future build-glue layer turns them
/// into `cargo:warning=` lines) but never make [`generate`] fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// A bit range inside a device's process image is covered by no PDO
    /// entry. Legal and often intentional, but never silent.
    UnmappedGap {
        /// Label of the device with the gap.
        device: String,
        /// Direction (`tx`/`rx`) whose process image has the gap.
        direction: PdoDirection,
        /// First unmapped bit (inclusive).
        start_bit: u32,
        /// One past the last unmapped bit (exclusive).
        end_bit: u32,
    },
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnmappedGap {
                device,
                direction,
                start_bit,
                end_bit,
            } => {
                let dir = match direction {
                    PdoDirection::Tx => "tx",
                    PdoDirection::Rx => "rx",
                };
                write!(
                    f,
                    "device '{device}' {dir} process image has an unmapped gap at bits {start_bit}..{end_bit}"
                )
            }
        }
    }
}

/// Collect non-fatal diagnostics for a network config.
///
/// Currently reports [`Warning::UnmappedGap`] for every unmapped bit range
/// within each device's process image (`REQ_0837`). Deterministic order: by
/// device (bus order), then tx before rx, then ascending `start_bit`.
pub fn warnings(config: &NetworkConfig) -> Vec<Warning> {
    let mut out = Vec::new();
    for device in &config.devices {
        for direction in [PdoDirection::Tx, PdoDirection::Rx] {
            gap_warnings_for(device, direction, &mut out);
        }
    }
    out
}

/// Append one [`Warning::UnmappedGap`] per gap in `device`'s `direction`
/// process image. Entries are assumed non-overlapping.
fn gap_warnings_for(device: &DeviceInstance, direction: PdoDirection, out: &mut Vec<Warning>) {
    let entries = match direction {
        PdoDirection::Rx => device.source.rx(),
        PdoDirection::Tx => device.source.tx(),
    };
    if entries.is_empty() {
        return;
    }

    let mut ranges: Vec<(u32, u32)> = entries
        .iter()
        .map(|e| {
            let start = u32::from(e.bit_offset);
            (start, start + u32::from(e.bit_length))
        })
        .collect();
    ranges.sort_by_key(|&(start, _)| start);

    let mut cursor = 0u32;
    for (start, end) in ranges {
        if start > cursor {
            out.push(Warning::UnmappedGap {
                device: device.label.clone(),
                direction,
                start_bit: cursor,
                end_bit: start,
            });
        }
        cursor = cursor.max(end);
    }
}

/// Inline process-image size, in bits, for `direction`: the maximum
/// `bit_offset + bit_length` over the device's entries in that direction
/// (`0` if the entry list for that direction is empty).
fn image_bits(device: &DeviceInstance, direction: PdoDirection) -> u32 {
    let entries = match direction {
        PdoDirection::Rx => device.source.rx(),
        PdoDirection::Tx => device.source.tx(),
    };
    entries
        .iter()
        .map(|e| u32::from(e.bit_offset) + u32::from(e.bit_length))
        .max()
        .unwrap_or(0)
}

/// Validate per-channel rules 1, 2, and 5 for a single [`ChannelBinding`].
///
/// Checks, in order: zero-length slice (rule 1), unknown device (rule 2),
/// slice out of process image (rule 5). Returns the first fault found.
fn validate_channel(
    channel: &ChannelBinding,
    device_by_label: &HashMap<&str, &DeviceInstance>,
) -> Result<(), CodegenError> {
    // Rule 1: zero-length slice.
    if channel.bit_length == 0 {
        return Err(ValidationError::ZeroLengthSlice {
            channel: channel.name.clone(),
        }
        .into());
    }
    // Rule 2: unknown / dangling device.
    let Some(device) = device_by_label.get(channel.device.as_str()) else {
        return Err(ValidationError::UnknownDevice {
            channel: channel.name.clone(),
            device: channel.device.clone(),
        }
        .into());
    };
    // Rule 5: slice out of process image (rule 2 takes precedence).
    let slice_end = channel.bit_offset + u32::from(channel.bit_length);
    let image = image_bits(device, channel.direction);
    if slice_end > image {
        return Err(ValidationError::SliceOutOfImage {
            channel: channel.name.clone(),
            slice_end,
            image_bits: image,
        }
        .into());
    }
    Ok(())
}

/// Validate a network config against the build-time rules.
///
/// Returns the first fault found as a [`CodegenError::Validation`].
pub fn validate(config: &NetworkConfig) -> Result<(), CodegenError> {
    let device_by_label: HashMap<&str, &DeviceInstance> = config
        .devices
        .iter()
        .map(|d| (d.label.as_str(), d))
        .collect();

    // Rules 1, 2, 5 — per-channel checks (zero-length, unknown device, out-of-image).
    for channel in &config.channels {
        validate_channel(channel, &device_by_label)?;
    }

    // Rule 3: duplicate channel name.
    let mut seen_names: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for channel in &config.channels {
        if !seen_names.insert(channel.name.as_str()) {
            return Err(ValidationError::DuplicateChannelName {
                name: channel.name.clone(),
            }
            .into());
        }
    }

    // Rule 4: duplicate configured address (only reachable via override).
    let mut addr_by_address: HashMap<u16, &str> = HashMap::new();
    for (index, device) in config.devices.iter().enumerate() {
        let address = device_address(index, device);
        if let Some(&prior) = addr_by_address.get(&address) {
            return Err(ValidationError::DuplicateAddress {
                address,
                labels: [prior.to_owned(), device.label.clone()],
            }
            .into());
        }
        addr_by_address.insert(address, device.label.as_str());
    }

    // Rule 6: overlapping slices on the same device + direction. Pairwise
    // check; an overlap is permitted if either channel sets `allow_overlap`.
    for (i, a) in config.channels.iter().enumerate() {
        for b in &config.channels[i + 1..] {
            if a.device != b.device || a.direction != b.direction {
                continue;
            }
            if a.allow_overlap || b.allow_overlap {
                continue;
            }
            let a_end = a.bit_offset + u32::from(a.bit_length);
            let b_end = b.bit_offset + u32::from(b.bit_length);
            // Half-open ranges intersect iff each starts before the other ends.
            if a.bit_offset < b_end && b.bit_offset < a_end {
                return Err(ValidationError::OverlappingSlices {
                    a: a.name.clone(),
                    b: b.name.clone(),
                }
                .into());
            }
        }
    }

    Ok(())
}

/// Generate formatted Rust source for the given network config.
///
/// This slice emits a single item: a `pub static PDO_MAP` of
/// `taktora_connector_ethercat::SubDeviceMap` entries, one per device.
pub fn generate(config: &NetworkConfig) -> Result<String, CodegenError> {
    validate(config)?;
    let pdo_map = pdo_map_tokens(config);
    let routing = routing_const_tokens(config);
    let identities = identity_table_tokens(config);
    let tokens = quote! {
        #pdo_map
        #routing
        #identities
    };
    let file: syn::File = syn::parse2(tokens)?;
    Ok(prettyplease::unparse(&file))
}

/// Configured station address for a device: `0x1000 + n` by bus position
/// (mirrors ethercrab's `init_single_group`), unless the device pins an
/// explicit override.
fn device_address(index: usize, device: &DeviceInstance) -> u16 {
    device
        .address_override
        .unwrap_or_else(|| 0x1000 + u16::try_from(index).expect("device index fits in u16"))
}

/// Map every device label to its resolved station address.
fn address_by_label(config: &NetworkConfig) -> HashMap<&str, u16> {
    config
        .devices
        .iter()
        .enumerate()
        .map(|(index, device)| (device.label.as_str(), device_address(index, device)))
        .collect()
}

/// Sanitize a channel name into a `SCREAMING_SNAKE_CASE` Rust identifier: every
/// non-alphanumeric char becomes `_`, ASCII letters are uppercased.
fn sanitize_ident(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// Emit, per channel, a `pub const <NAME>: EthercatRouting` and a matching
/// `pub const <NAME>_NAME: &str`.
fn routing_const_tokens(config: &NetworkConfig) -> TokenStream {
    let addresses = address_by_label(config);
    let consts = config
        .channels
        .iter()
        .map(|channel| channel_const_tokens(channel, &addresses));
    quote! {
        #(#consts)*
    }
}

/// Emit the two consts for a single channel binding.
fn channel_const_tokens(channel: &ChannelBinding, addresses: &HashMap<&str, u16>) -> TokenStream {
    let ident = format_ident!("{}", sanitize_ident(&channel.name));
    let name_ident = format_ident!("{}_NAME", sanitize_ident(&channel.name));

    let address: u16 = *addresses
        .get(channel.device.as_str())
        .expect("channel device label resolves to a configured device");
    let direction = match channel.direction {
        PdoDirection::Tx => quote!(taktora_connector_ethercat::PdoDirection::Tx),
        PdoDirection::Rx => quote!(taktora_connector_ethercat::PdoDirection::Rx),
    };
    let bit_offset = channel.bit_offset;
    let bit_length = channel.bit_length;
    let name = &channel.name;

    quote! {
        pub const #ident: taktora_connector_ethercat::EthercatRouting =
            taktora_connector_ethercat::EthercatRouting::new(
                #address,
                #direction,
                #bit_offset,
                #bit_length,
            );
        pub const #name_ident: &str = #name;
    }
}

/// Build the `static PDO_MAP` token stream.
fn pdo_map_tokens(config: &NetworkConfig) -> TokenStream {
    let entries = config.devices.iter().enumerate().map(|(index, device)| {
        let address: u16 = device_address(index, device);

        let rx = device.source.rx();
        let tx = device.source.tx();
        // Canonical EtherCAT LRW working-counter rule (REQ_0329): +1 per
        // SubDevice written to (RxPDOs / outputs), +2 per SubDevice read
        // from (TxPDOs / inputs). Derivation is the only source — no
        // override path (ADR_0095 / REQ_0828). WKC is derived from the
        // RESOLVED PDO presence regardless of whether the assignment is
        // actually written below.
        let expected_wkc: u16 = u16::from(!rx.is_empty()) + 2 * u16::from(!tx.is_empty());

        // PDO-assignment SDO writes (0x1C12/0x1C13) require a CoE mailbox.
        // Simple terminals (no mailbox) must get an EMPTY assignment — they
        // keep their fixed default mapping; writing 0x1C12 to them fails with
        // `NoReadMailbox` on the bus. They still contribute `expected_wkc`
        // above and their process data is routed via the channel consts.
        let empty: &[taktora_ethercat_netcfg::PdoEntry] = &[];
        let (rx, tx) = if device.supports_coe {
            (rx, tx)
        } else {
            (empty, empty)
        };
        let rx_entries = rx.iter().map(pdo_entry_tokens);
        let tx_entries = tx.iter().map(pdo_entry_tokens);

        // Output (rx-carrying) devices resolve an SM watchdog
        // (REQ_0844); emit it as a `.with_sm_watchdog(..)` chained on the
        // `new(..)` constructor. Input-only devices carry `None` and emit
        // no watchdog (their outputs do not exist, so AOU_0016 does not
        // apply). Field names mirror the connector's `SmWatchdog`.
        let watchdog = device.sm_watchdog.as_ref().map(|wd| {
            let divider = wd.divider;
            let intervals = wd.intervals;
            quote! {
                .with_sm_watchdog(taktora_connector_ethercat::SmWatchdog {
                    divider: #divider,
                    intervals: #intervals,
                })
            }
        });

        let startup = if device.startup_sdos.is_empty() {
            None
        } else {
            let sdos = device.startup_sdos.iter().map(startup_sdo_tokens);
            Some(quote! {
                .with_startup_sdos(&[ #(#sdos),* ])
            })
        };

        // `SubDeviceMap` is `#[non_exhaustive]`; out-of-crate generated
        // code must use the `new` constructor rather than a struct
        // literal. Argument order: address, rx_pdos, tx_pdos,
        // expected_wkc.
        quote! {
            taktora_connector_ethercat::SubDeviceMap::new(
                #address,
                &[ #(#rx_entries),* ],
                &[ #(#tx_entries),* ],
                #expected_wkc,
            )
            #watchdog
            #startup
        }
    });

    quote! {
        pub static PDO_MAP: &[taktora_connector_ethercat::SubDeviceMap] = &[
            #(#entries),*
        ];
    }
}

/// Emit the self-contained `ExpectedIdentity` struct and the
/// `EXPECTED_IDENTITIES` static a future runtime bring-up check consumes
/// (`REQ_0838`).
///
/// The struct is emitted verbatim (not referenced from
/// `taktora-connector-ethercat`, which has no suitable type) so the
/// generated module stays self-contained and always compiles. One entry per
/// device whose `identity` is `Some`, in bus order; devices with no known
/// identity contribute nothing. The struct def and an (possibly empty)
/// static are always emitted to keep the generated surface stable.
fn identity_table_tokens(config: &NetworkConfig) -> TokenStream {
    let entries = config
        .devices
        .iter()
        .enumerate()
        .filter_map(|(index, device)| {
            let identity = device.identity.as_ref()?;
            let address = device_address(index, device);
            let vendor_id = identity.vendor_id;
            let product_code = identity.product_code;
            let revision = identity.revision;
            let station_alias = device
                .station_alias
                .map_or_else(|| quote!(None), |alias| quote!(Some(#alias)));
            Some(quote! {
                ExpectedIdentity {
                    address: #address,
                    vendor_id: #vendor_id,
                    product_code: #product_code,
                    revision: #revision,
                    station_alias: #station_alias,
                }
            })
        });

    quote! {
        pub struct ExpectedIdentity {
            pub address: u16,
            pub vendor_id: u32,
            pub product_code: u32,
            pub revision: u32,
            pub station_alias: Option<u16>,
        }
        pub static EXPECTED_IDENTITIES: &[ExpectedIdentity] = &[
            #(#entries),*
        ];
    }
}

/// Emit one `taktora_connector_ethercat::PdoEntry` literal.
fn pdo_entry_tokens(entry: &PdoEntry) -> TokenStream {
    let index = entry.index;
    let bit_offset = entry.bit_offset;
    let bit_length = entry.bit_length;
    quote! {
        taktora_connector_ethercat::PdoEntry {
            index: #index,
            bit_offset: #bit_offset,
            bit_length: #bit_length,
        }
    }
}

/// Emit one `taktora_connector_ethercat::StartupSdo` literal.
fn startup_sdo_tokens(sdo: &StartupSdoSpec) -> TokenStream {
    let index = sdo.index;
    let subindex = sdo.subindex;
    let value = sdo_value_tokens(sdo.value);
    quote! {
        taktora_connector_ethercat::StartupSdo {
            index: #index,
            subindex: #subindex,
            value: #value,
        }
    }
}

/// Emit the `taktora_connector_ethercat::SdoValue::<V>(..)` literal.
fn sdo_value_tokens(value: SdoValueSpec) -> TokenStream {
    match value {
        SdoValueSpec::U8(v) => quote!(taktora_connector_ethercat::SdoValue::U8(#v)),
        SdoValueSpec::U16(v) => quote!(taktora_connector_ethercat::SdoValue::U16(#v)),
        SdoValueSpec::U32(v) => quote!(taktora_connector_ethercat::SdoValue::U32(#v)),
        SdoValueSpec::I8(v) => quote!(taktora_connector_ethercat::SdoValue::I8(#v)),
        SdoValueSpec::I16(v) => quote!(taktora_connector_ethercat::SdoValue::I16(#v)),
        SdoValueSpec::I32(v) => quote!(taktora_connector_ethercat::SdoValue::I32(#v)),
    }
}
