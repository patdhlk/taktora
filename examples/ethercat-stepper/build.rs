use std::path::Path;

fn main() {
    // Per-device ESI codegen -> $OUT_DIR/devices.rs (generated::*).
    taktora_ethercat_esi_build::Builder::new()
        .glob("esi/*.xml")
        .out_file("devices.rs")
        .build()
        .unwrap_or_else(|e| panic!("ESI codegen failed: {e}"));

    // Per-bus netcfg codegen -> $OUT_DIR/network.rs (generated_net::*).
    taktora_ethercat_netcfg_build::run(Path::new("network.yaml"))
        .unwrap_or_else(|e| panic!("netcfg codegen failed: {e}"));
}
