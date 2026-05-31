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
}

/// Generate formatted Rust source for the given network config.
///
/// This slice emits a single item: a `pub static PDO_MAP` of
/// `taktora_connector_ethercat::SubDeviceMap` entries, one per device.
pub fn generate(config: &NetworkConfig) -> Result<String, CodegenError> {
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
        // Slice 5 will derive `expected_wkc`; placeholder for now.
        let expected_wkc: u16 = 0;

        let DeviceSource::Inline { rx, tx } = &device.source;
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
