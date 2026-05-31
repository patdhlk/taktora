//! Code generator for the `ethercat-netcfg` IR.
//!
//! Turns a parsed [`ethercat_netcfg::NetworkConfig`] into a `String` of
//! formatted Rust source for the `taktora-connector-ethercat` runtime.
//! The runtime types are named **textually** so this crate never
//! depends on `taktora-connector-ethercat`.

#![warn(missing_docs)]

use std::collections::HashMap;

use ethercat_netcfg::{
    ChannelBinding, DeviceInstance, DeviceSource, NetworkConfig, PdoDirection, PdoEntry,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

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

/// Inline process-image size, in bits, for `direction`: the maximum
/// `bit_offset + bit_length` over the device's entries in that direction
/// (`0` if the entry list for that direction is empty).
fn image_bits(device: &DeviceInstance, direction: PdoDirection) -> u32 {
    let DeviceSource::Inline { rx, tx } = &device.source;
    let entries = match direction {
        PdoDirection::Rx => rx,
        PdoDirection::Tx => tx,
    };
    entries
        .iter()
        .map(|e| u32::from(e.bit_offset) + u32::from(e.bit_length))
        .max()
        .unwrap_or(0)
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

    for channel in &config.channels {
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
    let tokens = quote! {
        #pdo_map
        #routing
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

        let DeviceSource::Inline { rx, tx } = &device.source;
        // Canonical EtherCAT LRW working-counter rule (REQ_0329): +1 per
        // SubDevice written to (RxPDOs / outputs), +2 per SubDevice read
        // from (TxPDOs / inputs). Derivation is the only source — no
        // override path (ADR_0095 / REQ_0828).
        let expected_wkc: u16 = u16::from(!rx.is_empty()) + 2 * u16::from(!tx.is_empty());
        let rx_entries = rx.iter().map(pdo_entry_tokens);
        let tx_entries = tx.iter().map(pdo_entry_tokens);

        quote! {
            taktora_connector_ethercat::SubDeviceMap {
                address: #address,
                rx_pdos: &[ #(#rx_entries),* ],
                tx_pdos: &[ #(#tx_entries),* ],
                expected_wkc: #expected_wkc,
            }
        }
    });

    quote! {
        pub static PDO_MAP: &[taktora_connector_ethercat::SubDeviceMap] = &[
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
