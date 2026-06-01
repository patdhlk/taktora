#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL3001_like {
    pub underrange: bool,
    pub value: i16,
}
pub const EL3001_LIKE_REV00100000: taktora_ethercat_esi_rt::Identity = taktora_ethercat_esi_rt::Identity {
    vendor_id: 2u32,
    product_code: 196685906u32,
    revision: 1048576u32,
};
impl taktora_ethercat_esi_rt::EsiDevice for EL3001_like {
    fn identity(&self) -> taktora_ethercat_esi_rt::Identity {
        EL3001_LIKE_REV00100000
    }
    fn input_len(&self) -> usize {
        3usize
    }
    fn output_len(&self) -> usize {
        0usize
    }
    fn decode_inputs(
        &mut self,
        bits: &taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        use bitvec::field::BitField as _;
        const NEED: usize = 24usize;
        if bits.len() < NEED {
            return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                expected_bits: NEED,
                got_bits: bits.len(),
            });
        }
        self.underrange = bits[0usize];
        self.value = bits[8usize..24usize].load_le::<i16>();
        Ok(())
    }
    fn encode_outputs(
        &self,
        bits: &mut taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        let _ = bits;
        Ok(())
    }
}
