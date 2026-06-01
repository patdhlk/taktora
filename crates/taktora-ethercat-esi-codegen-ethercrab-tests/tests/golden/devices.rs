#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008Channel1 {
    pub input: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008Channel2 {
    pub input: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008Channel3 {
    pub input: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008Channel4 {
    pub input: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008Channel5 {
    pub input: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008Channel6 {
    pub input: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008Channel7 {
    pub input: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008Channel8 {
    pub input: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008 {
    pub channel_1: EL1008Channel1,
    pub channel_2: EL1008Channel2,
    pub channel_3: EL1008Channel3,
    pub channel_4: EL1008Channel4,
    pub channel_5: EL1008Channel5,
    pub channel_6: EL1008Channel6,
    pub channel_7: EL1008Channel7,
    pub channel_8: EL1008Channel8,
}
pub const EL1008_REV00100000: taktora_ethercat_esi_rt::Identity = taktora_ethercat_esi_rt::Identity {
    vendor_id: 2u32,
    product_code: 66072658u32,
    revision: 1048576u32,
};
impl taktora_ethercat_esi_rt::EsiDevice for EL1008 {
    fn identity(&self) -> taktora_ethercat_esi_rt::Identity {
        EL1008_REV00100000
    }
    fn input_len(&self) -> usize {
        1usize
    }
    fn output_len(&self) -> usize {
        0usize
    }
    fn decode_inputs(
        &mut self,
        bits: &taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        const NEED: usize = 8usize;
        if bits.len() < NEED {
            return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                expected_bits: NEED,
                got_bits: bits.len(),
            });
        }
        self.channel_1.input = bits[0usize];
        self.channel_2.input = bits[1usize];
        self.channel_3.input = bits[2usize];
        self.channel_4.input = bits[3usize];
        self.channel_5.input = bits[4usize];
        self.channel_6.input = bits[5usize];
        self.channel_7.input = bits[6usize];
        self.channel_8.input = bits[7usize];
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
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL2004Channel1 {
    pub output: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL2004Channel2 {
    pub output: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL2004Channel3 {
    pub output: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL2004Channel4 {
    pub output: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL2004 {
    pub channel_1: EL2004Channel1,
    pub channel_2: EL2004Channel2,
    pub channel_3: EL2004Channel3,
    pub channel_4: EL2004Channel4,
}
pub const EL2004_REV00000000: taktora_ethercat_esi_rt::Identity = taktora_ethercat_esi_rt::Identity {
    vendor_id: 2u32,
    product_code: 131346514u32,
    revision: 0u32,
};
impl taktora_ethercat_esi_rt::EsiDevice for EL2004 {
    fn identity(&self) -> taktora_ethercat_esi_rt::Identity {
        EL2004_REV00000000
    }
    fn input_len(&self) -> usize {
        0usize
    }
    fn output_len(&self) -> usize {
        1usize
    }
    fn decode_inputs(
        &mut self,
        bits: &taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        const NEED: usize = 0usize;
        if bits.len() < NEED {
            return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                expected_bits: NEED,
                got_bits: bits.len(),
            });
        }
        Ok(())
    }
    fn encode_outputs(
        &self,
        bits: &mut taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        const NEED: usize = 4usize;
        if bits.len() < NEED {
            return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                expected_bits: NEED,
                got_bits: bits.len(),
            });
        }
        bits.set(0usize, self.channel_1.output);
        bits.set(1usize, self.channel_2.output);
        bits.set(2usize, self.channel_3.output);
        bits.set(3usize, self.channel_4.output);
        Ok(())
    }
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL3602AiInputsChannel1 {
    pub underrange: bool,
    pub overrange: bool,
    pub limit_1: u8,
    pub limit_2: u8,
    pub error: bool,
    pub tx_pdo_state: bool,
    pub tx_pdo_toggle: bool,
    pub value: i32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL3602AiInputsChannel2 {
    pub underrange: bool,
    pub overrange: bool,
    pub limit_1: u8,
    pub limit_2: u8,
    pub error: bool,
    pub tx_pdo_state: bool,
    pub tx_pdo_toggle: bool,
    pub value: i32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL3602 {
    pub ai_inputs_channel_1: EL3602AiInputsChannel1,
    pub ai_inputs_channel_2: EL3602AiInputsChannel2,
}
pub const EL3602_REV00100000: taktora_ethercat_esi_rt::Identity = taktora_ethercat_esi_rt::Identity {
    vendor_id: 2u32,
    product_code: 236073042u32,
    revision: 1048576u32,
};
impl taktora_ethercat_esi_rt::EsiDevice for EL3602 {
    fn identity(&self) -> taktora_ethercat_esi_rt::Identity {
        EL3602_REV00100000
    }
    fn input_len(&self) -> usize {
        12usize
    }
    fn output_len(&self) -> usize {
        0usize
    }
    fn decode_inputs(
        &mut self,
        bits: &taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        use bitvec::field::BitField as _;
        const NEED: usize = 96usize;
        if bits.len() < NEED {
            return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                expected_bits: NEED,
                got_bits: bits.len(),
            });
        }
        self.ai_inputs_channel_1.underrange = bits[0usize];
        self.ai_inputs_channel_1.overrange = bits[1usize];
        self.ai_inputs_channel_1.limit_1 = bits[2usize..4usize].load_le::<u8>();
        self.ai_inputs_channel_1.limit_2 = bits[4usize..6usize].load_le::<u8>();
        self.ai_inputs_channel_1.error = bits[6usize];
        self.ai_inputs_channel_1.tx_pdo_state = bits[14usize];
        self.ai_inputs_channel_1.tx_pdo_toggle = bits[15usize];
        self.ai_inputs_channel_1.value = bits[16usize..48usize].load_le::<i32>();
        self.ai_inputs_channel_2.underrange = bits[48usize];
        self.ai_inputs_channel_2.overrange = bits[49usize];
        self.ai_inputs_channel_2.limit_1 = bits[50usize..52usize].load_le::<u8>();
        self.ai_inputs_channel_2.limit_2 = bits[52usize..54usize].load_le::<u8>();
        self.ai_inputs_channel_2.error = bits[54usize];
        self.ai_inputs_channel_2.tx_pdo_state = bits[62usize];
        self.ai_inputs_channel_2.tx_pdo_toggle = bits[63usize];
        self.ai_inputs_channel_2.value = bits[64usize..96usize].load_le::<i32>();
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
/// All devices generated in this module, keyed by EtherCAT identity.
/// A linear scan over this slice is reducible to a `HashMap` lookup.
pub static REGISTRY: &[(
    taktora_ethercat_esi_rt::Identity,
    fn() -> Box<dyn taktora_ethercat_esi_rt::EsiDevice>,
)] = &[
    (
        EL1008_REV00100000,
        || Box::new(EL1008::default()) as Box<dyn taktora_ethercat_esi_rt::EsiDevice>,
    ),
    (
        EL2004_REV00000000,
        || Box::new(EL2004::default()) as Box<dyn taktora_ethercat_esi_rt::EsiDevice>,
    ),
    (
        EL3602_REV00100000,
        || Box::new(EL3602::default()) as Box<dyn taktora_ethercat_esi_rt::EsiDevice>,
    ),
    (
        EL3001_LIKE_REV00100000,
        || {
            Box::new(EL3001_like::default())
                as Box<dyn taktora_ethercat_esi_rt::EsiDevice>
        },
    ),
];
/// Construct a fresh device instance for the given identity, if known.
pub fn device_for(
    identity: taktora_ethercat_esi_rt::Identity,
) -> Option<Box<dyn taktora_ethercat_esi_rt::EsiDevice>> {
    REGISTRY.iter().find(|(id, _)| *id == identity).map(|(_, make)| make())
}
