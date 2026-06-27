//! Build-time codegen: DBC → message IR → CAN `WireType` code in `OUT_DIR`.
//!
//! This mirrors the device-plane `esi-build` flow (read → parse → generate →
//! format → write) so the generated code is exercised by a real `rustc`
//! compile, not just a token-stream snapshot.

use std::{env, fs, path::Path};

use taktora_idl_codegen::generate;
use taktora_idl_codegen_can::CanBackend;
use taktora_idl_dbc::{lower, parse};

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR");
    let dbc_path = Path::new(&manifest).join("dbc/vehicle.dbc");

    println!("cargo:rerun-if-changed={}", dbc_path.display());
    println!("cargo:rerun-if-changed=build.rs");

    let text = fs::read_to_string(&dbc_path).expect("read fixture DBC");
    let db = parse(&text).expect("parse DBC");
    let lowered = lower(&db, "vehicle").expect("lower DBC");

    let backend = CanBackend::new(&lowered.layout);
    let items = generate(&lowered.module, &backend).expect("generate");

    // Wrap the items in the module the runtime support `use` lives under.
    let module = quote::quote! {
        /// Generated from `dbc/vehicle.dbc`. Do not edit.
        pub mod vehicle {
            #items
        }
    };
    let file: syn::File = syn::parse2(module).expect("generated tokens parse");
    let rendered = prettyplease::unparse(&file);

    fs::write(Path::new(&out_dir).join("vehicle.rs"), rendered).expect("write generated module");
}
