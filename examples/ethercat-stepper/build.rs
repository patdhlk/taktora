fn main() {
    taktora_ethercat_esi_build::Builder::new()
        .glob("esi/*.xml")
        .out_file("devices.rs")
        .build()
        .unwrap_or_else(|e| panic!("ESI codegen failed: {e}"));
}
