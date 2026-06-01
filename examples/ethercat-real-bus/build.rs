fn main() {
    taktora_ethercat_esi_build::Builder::new()
        .glob("esi/*.xml")
        .out_file("devices.rs")
        .build()
        // Surface BuildError's full Display chain (path / parse error), not the
        // terse Debug form `.expect` would print.
        .unwrap_or_else(|e| panic!("ESI codegen failed: {e}"));
}
