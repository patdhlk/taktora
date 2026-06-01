fn main() {
    taktora_ethercat_esi_build::Builder::new()
        .glob("esi/*.xml")
        .out_file("devices.rs")
        .build()
        .expect("ESI codegen failed");
}
