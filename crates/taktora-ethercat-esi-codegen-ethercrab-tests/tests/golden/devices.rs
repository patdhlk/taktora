#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008DefaultChannel1 {
    pub input: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008DefaultChannel2 {
    pub input: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008DefaultChannel3 {
    pub input: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008DefaultChannel4 {
    pub input: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008DefaultChannel5 {
    pub input: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008DefaultChannel6 {
    pub input: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008DefaultChannel7 {
    pub input: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008DefaultChannel8 {
    pub input: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008DefaultIn {
    pub channel_1: EL1008DefaultChannel1,
    pub channel_2: EL1008DefaultChannel2,
    pub channel_3: EL1008DefaultChannel3,
    pub channel_4: EL1008DefaultChannel4,
    pub channel_5: EL1008DefaultChannel5,
    pub channel_6: EL1008DefaultChannel6,
    pub channel_7: EL1008DefaultChannel7,
    pub channel_8: EL1008DefaultChannel8,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008DefaultOut {}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008Default {
    pub inputs: EL1008DefaultIn,
    pub outputs: EL1008DefaultOut,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Clone)]
pub enum EL1008OpMode {
    Default(EL1008Default),
}
impl Default for EL1008OpMode {
    fn default() -> Self {
        Self::Default(Default::default())
    }
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL1008 {
    pub mode: EL1008OpMode,
}
impl EL1008 {
    /// The Rx/Tx PDO-assignment index lists (0x1C12/0x1C13) for the
    /// active mode. (issue #70)
    #[must_use]
    pub fn pdo_assignment(&self) -> PdoAssignment<'static> {
        match &self.mode {
            EL1008OpMode::Default(_) => {
                PdoAssignment {
                    rx: &[],
                    tx: &[
                        6656u16, 6657u16, 6658u16, 6659u16, 6660u16, 6661u16, 6662u16,
                        6663u16,
                    ],
                }
            }
        }
    }
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
        match &self.mode {
            EL1008OpMode::Default(_) => 1usize,
        }
    }
    fn output_len(&self) -> usize {
        match &self.mode {
            EL1008OpMode::Default(_) => 0usize,
        }
    }
    fn decode_inputs(
        &mut self,
        bits: &taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        match &mut self.mode {
            EL1008OpMode::Default(m) => {
                const NEED: usize = 8usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                m.inputs.channel_1.input = bits[0usize];
                m.inputs.channel_2.input = bits[1usize];
                m.inputs.channel_3.input = bits[2usize];
                m.inputs.channel_4.input = bits[3usize];
                m.inputs.channel_5.input = bits[4usize];
                m.inputs.channel_6.input = bits[5usize];
                m.inputs.channel_7.input = bits[6usize];
                m.inputs.channel_8.input = bits[7usize];
            }
        }
        Ok(())
    }
    fn encode_outputs(
        &self,
        bits: &mut taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        match &self.mode {
            EL1008OpMode::Default(m) => {
                const NEED: usize = 0usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
            }
        }
        Ok(())
    }
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL2004DefaultIn {}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL2004DefaultChannel1 {
    pub output: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL2004DefaultChannel2 {
    pub output: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL2004DefaultChannel3 {
    pub output: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL2004DefaultChannel4 {
    pub output: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL2004DefaultOut {
    pub channel_1: EL2004DefaultChannel1,
    pub channel_2: EL2004DefaultChannel2,
    pub channel_3: EL2004DefaultChannel3,
    pub channel_4: EL2004DefaultChannel4,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL2004Default {
    pub inputs: EL2004DefaultIn,
    pub outputs: EL2004DefaultOut,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Clone)]
pub enum EL2004OpMode {
    Default(EL2004Default),
}
impl Default for EL2004OpMode {
    fn default() -> Self {
        Self::Default(Default::default())
    }
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL2004 {
    pub mode: EL2004OpMode,
}
impl EL2004 {
    /// The Rx/Tx PDO-assignment index lists (0x1C12/0x1C13) for the
    /// active mode. (issue #70)
    #[must_use]
    pub fn pdo_assignment(&self) -> PdoAssignment<'static> {
        match &self.mode {
            EL2004OpMode::Default(_) => {
                PdoAssignment {
                    rx: &[5632u16, 5633u16, 5634u16, 5635u16],
                    tx: &[],
                }
            }
        }
    }
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
        match &self.mode {
            EL2004OpMode::Default(_) => 0usize,
        }
    }
    fn output_len(&self) -> usize {
        match &self.mode {
            EL2004OpMode::Default(_) => 1usize,
        }
    }
    fn decode_inputs(
        &mut self,
        bits: &taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        match &mut self.mode {
            EL2004OpMode::Default(m) => {
                const NEED: usize = 0usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
            }
        }
        Ok(())
    }
    fn encode_outputs(
        &self,
        bits: &mut taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        match &self.mode {
            EL2004OpMode::Default(m) => {
                const NEED: usize = 4usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                bits.set(0usize, m.outputs.channel_1.output);
                bits.set(1usize, m.outputs.channel_2.output);
                bits.set(2usize, m.outputs.channel_3.output);
                bits.set(3usize, m.outputs.channel_4.output);
            }
        }
        Ok(())
    }
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL3602DefaultAiInputsChannel1 {
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
pub struct EL3602DefaultAiInputsChannel2 {
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
pub struct EL3602DefaultIn {
    pub ai_inputs_channel_1: EL3602DefaultAiInputsChannel1,
    pub ai_inputs_channel_2: EL3602DefaultAiInputsChannel2,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL3602DefaultOut {}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL3602Default {
    pub inputs: EL3602DefaultIn,
    pub outputs: EL3602DefaultOut,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Clone)]
pub enum EL3602OpMode {
    Default(EL3602Default),
}
impl Default for EL3602OpMode {
    fn default() -> Self {
        Self::Default(Default::default())
    }
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL3602 {
    pub mode: EL3602OpMode,
}
impl EL3602 {
    /// The Rx/Tx PDO-assignment index lists (0x1C12/0x1C13) for the
    /// active mode. (issue #70)
    #[must_use]
    pub fn pdo_assignment(&self) -> PdoAssignment<'static> {
        match &self.mode {
            EL3602OpMode::Default(_) => {
                PdoAssignment {
                    rx: &[],
                    tx: &[6656u16, 6657u16],
                }
            }
        }
    }
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
        match &self.mode {
            EL3602OpMode::Default(_) => 12usize,
        }
    }
    fn output_len(&self) -> usize {
        match &self.mode {
            EL3602OpMode::Default(_) => 0usize,
        }
    }
    fn decode_inputs(
        &mut self,
        bits: &taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        use bitvec::field::BitField as _;
        match &mut self.mode {
            EL3602OpMode::Default(m) => {
                const NEED: usize = 96usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                m.inputs.ai_inputs_channel_1.underrange = bits[0usize];
                m.inputs.ai_inputs_channel_1.overrange = bits[1usize];
                m.inputs.ai_inputs_channel_1.limit_1 = bits[2usize..4usize]
                    .load_le::<u8>();
                m.inputs.ai_inputs_channel_1.limit_2 = bits[4usize..6usize]
                    .load_le::<u8>();
                m.inputs.ai_inputs_channel_1.error = bits[6usize];
                m.inputs.ai_inputs_channel_1.tx_pdo_state = bits[14usize];
                m.inputs.ai_inputs_channel_1.tx_pdo_toggle = bits[15usize];
                m.inputs.ai_inputs_channel_1.value = bits[16usize..48usize]
                    .load_le::<i32>();
                m.inputs.ai_inputs_channel_2.underrange = bits[48usize];
                m.inputs.ai_inputs_channel_2.overrange = bits[49usize];
                m.inputs.ai_inputs_channel_2.limit_1 = bits[50usize..52usize]
                    .load_le::<u8>();
                m.inputs.ai_inputs_channel_2.limit_2 = bits[52usize..54usize]
                    .load_le::<u8>();
                m.inputs.ai_inputs_channel_2.error = bits[54usize];
                m.inputs.ai_inputs_channel_2.tx_pdo_state = bits[62usize];
                m.inputs.ai_inputs_channel_2.tx_pdo_toggle = bits[63usize];
                m.inputs.ai_inputs_channel_2.value = bits[64usize..96usize]
                    .load_le::<i32>();
            }
        }
        Ok(())
    }
    fn encode_outputs(
        &self,
        bits: &mut taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        match &self.mode {
            EL3602OpMode::Default(m) => {
                const NEED: usize = 0usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
            }
        }
        Ok(())
    }
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompactEncStatusCompact {
    pub status_latch_c_valid: bool,
    pub status_latch_extern_valid: bool,
    pub status_set_counter_done: bool,
    pub status_counter_underflow: bool,
    pub status_counter_overflow: bool,
    pub status_extrapolation_stall: bool,
    pub status_status_of_input_a: bool,
    pub status_status_of_input_b: bool,
    pub status_status_of_input_c: bool,
    pub status_status_of_extern_latch: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
    pub counter_value: u16,
    pub latch_value: u16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompactStmStatus {
    pub status_ready_to_enable: bool,
    pub status_ready: bool,
    pub status_warning: bool,
    pub status_error: bool,
    pub status_moving_positive: bool,
    pub status_moving_negative: bool,
    pub status_torque_reduced: bool,
    pub status_motor_stall: bool,
    pub status_digital_input_1: bool,
    pub status_digital_input_2: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompactIn {
    pub enc_status_compact: EL7047VelocityControlCompactEncStatusCompact,
    pub stm_status: EL7047VelocityControlCompactStmStatus,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompactEncControlCompact {
    pub control_enable_latch_c: bool,
    pub control_enable_latch_extern_on_positive_edge: bool,
    pub control_set_counter: bool,
    pub control_enable_latch_extern_on_negative_edge: bool,
    pub set_counter_value: u16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompactStmControl {
    pub control_enable: bool,
    pub control_reset: bool,
    pub control_reduce_torque: bool,
    pub control_digital_output_1: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompactStmVelocity {
    pub velocity: i16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompactOut {
    pub enc_control_compact: EL7047VelocityControlCompactEncControlCompact,
    pub stm_control: EL7047VelocityControlCompactStmControl,
    pub stm_velocity: EL7047VelocityControlCompactStmVelocity,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompact {
    pub inputs: EL7047VelocityControlCompactIn,
    pub outputs: EL7047VelocityControlCompactOut,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompactWithInfoDataEncStatusCompact {
    pub status_latch_c_valid: bool,
    pub status_latch_extern_valid: bool,
    pub status_set_counter_done: bool,
    pub status_counter_underflow: bool,
    pub status_counter_overflow: bool,
    pub status_extrapolation_stall: bool,
    pub status_status_of_input_a: bool,
    pub status_status_of_input_b: bool,
    pub status_status_of_input_c: bool,
    pub status_status_of_extern_latch: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
    pub counter_value: u16,
    pub latch_value: u16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompactWithInfoDataStmStatus {
    pub status_ready_to_enable: bool,
    pub status_ready: bool,
    pub status_warning: bool,
    pub status_error: bool,
    pub status_moving_positive: bool,
    pub status_moving_negative: bool,
    pub status_torque_reduced: bool,
    pub status_motor_stall: bool,
    pub status_digital_input_1: bool,
    pub status_digital_input_2: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompactWithInfoDataStmSynchronInfoData {
    pub info_data_1: u16,
    pub info_data_2: u16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompactWithInfoDataIn {
    pub enc_status_compact: EL7047VelocityControlCompactWithInfoDataEncStatusCompact,
    pub stm_status: EL7047VelocityControlCompactWithInfoDataStmStatus,
    pub stm_synchron_info_data: EL7047VelocityControlCompactWithInfoDataStmSynchronInfoData,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompactWithInfoDataEncControlCompact {
    pub control_enable_latch_c: bool,
    pub control_enable_latch_extern_on_positive_edge: bool,
    pub control_set_counter: bool,
    pub control_enable_latch_extern_on_negative_edge: bool,
    pub set_counter_value: u16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompactWithInfoDataStmControl {
    pub control_enable: bool,
    pub control_reset: bool,
    pub control_reduce_torque: bool,
    pub control_digital_output_1: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompactWithInfoDataStmVelocity {
    pub velocity: i16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompactWithInfoDataOut {
    pub enc_control_compact: EL7047VelocityControlCompactWithInfoDataEncControlCompact,
    pub stm_control: EL7047VelocityControlCompactWithInfoDataStmControl,
    pub stm_velocity: EL7047VelocityControlCompactWithInfoDataStmVelocity,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlCompactWithInfoData {
    pub inputs: EL7047VelocityControlCompactWithInfoDataIn,
    pub outputs: EL7047VelocityControlCompactWithInfoDataOut,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlEncStatus {
    pub status_latch_c_valid: bool,
    pub status_latch_extern_valid: bool,
    pub status_set_counter_done: bool,
    pub status_counter_underflow: bool,
    pub status_counter_overflow: bool,
    pub status_extrapolation_stall: bool,
    pub status_status_of_input_a: bool,
    pub status_status_of_input_b: bool,
    pub status_status_of_input_c: bool,
    pub status_status_of_extern_latch: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
    pub counter_value: u32,
    pub latch_value: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlStmStatus {
    pub status_ready_to_enable: bool,
    pub status_ready: bool,
    pub status_warning: bool,
    pub status_error: bool,
    pub status_moving_positive: bool,
    pub status_moving_negative: bool,
    pub status_torque_reduced: bool,
    pub status_motor_stall: bool,
    pub status_digital_input_1: bool,
    pub status_digital_input_2: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlIn {
    pub enc_status: EL7047VelocityControlEncStatus,
    pub stm_status: EL7047VelocityControlStmStatus,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlEncControl {
    pub control_enable_latch_c: bool,
    pub control_enable_latch_extern_on_positive_edge: bool,
    pub control_set_counter: bool,
    pub control_enable_latch_extern_on_negative_edge: bool,
    pub set_counter_value: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlStmControl {
    pub control_enable: bool,
    pub control_reset: bool,
    pub control_reduce_torque: bool,
    pub control_digital_output_1: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlStmVelocity {
    pub velocity: i16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControlOut {
    pub enc_control: EL7047VelocityControlEncControl,
    pub stm_control: EL7047VelocityControlStmControl,
    pub stm_velocity: EL7047VelocityControlStmVelocity,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047VelocityControl {
    pub inputs: EL7047VelocityControlIn,
    pub outputs: EL7047VelocityControlOut,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositionControlEncStatus {
    pub status_latch_c_valid: bool,
    pub status_latch_extern_valid: bool,
    pub status_set_counter_done: bool,
    pub status_counter_underflow: bool,
    pub status_counter_overflow: bool,
    pub status_extrapolation_stall: bool,
    pub status_status_of_input_a: bool,
    pub status_status_of_input_b: bool,
    pub status_status_of_input_c: bool,
    pub status_status_of_extern_latch: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
    pub counter_value: u32,
    pub latch_value: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositionControlStmStatus {
    pub status_ready_to_enable: bool,
    pub status_ready: bool,
    pub status_warning: bool,
    pub status_error: bool,
    pub status_moving_positive: bool,
    pub status_moving_negative: bool,
    pub status_torque_reduced: bool,
    pub status_motor_stall: bool,
    pub status_digital_input_1: bool,
    pub status_digital_input_2: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositionControlIn {
    pub enc_status: EL7047PositionControlEncStatus,
    pub stm_status: EL7047PositionControlStmStatus,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositionControlEncControl {
    pub control_enable_latch_c: bool,
    pub control_enable_latch_extern_on_positive_edge: bool,
    pub control_set_counter: bool,
    pub control_enable_latch_extern_on_negative_edge: bool,
    pub set_counter_value: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositionControlStmControl {
    pub control_enable: bool,
    pub control_reset: bool,
    pub control_reduce_torque: bool,
    pub control_digital_output_1: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositionControlStmPosition {
    pub position: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositionControlOut {
    pub enc_control: EL7047PositionControlEncControl,
    pub stm_control: EL7047PositionControlStmControl,
    pub stm_position: EL7047PositionControlStmPosition,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositionControl {
    pub inputs: EL7047PositionControlIn,
    pub outputs: EL7047PositionControlOut,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceCompactEncStatus {
    pub status_latch_c_valid: bool,
    pub status_latch_extern_valid: bool,
    pub status_set_counter_done: bool,
    pub status_counter_underflow: bool,
    pub status_counter_overflow: bool,
    pub status_extrapolation_stall: bool,
    pub status_status_of_input_a: bool,
    pub status_status_of_input_b: bool,
    pub status_status_of_input_c: bool,
    pub status_status_of_extern_latch: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
    pub counter_value: u32,
    pub latch_value: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceCompactStmStatus {
    pub status_ready_to_enable: bool,
    pub status_ready: bool,
    pub status_warning: bool,
    pub status_error: bool,
    pub status_moving_positive: bool,
    pub status_moving_negative: bool,
    pub status_torque_reduced: bool,
    pub status_motor_stall: bool,
    pub status_digital_input_1: bool,
    pub status_digital_input_2: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceCompactPosStatusCompact {
    pub status_busy: bool,
    pub status_in_target: bool,
    pub status_warning: bool,
    pub status_error: bool,
    pub status_calibrated: bool,
    pub status_accelerate: bool,
    pub status_decelerate: bool,
    pub status_ready_to_execute: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceCompactIn {
    pub enc_status: EL7047PositioningInterfaceCompactEncStatus,
    pub stm_status: EL7047PositioningInterfaceCompactStmStatus,
    pub pos_status_compact: EL7047PositioningInterfaceCompactPosStatusCompact,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceCompactEncControl {
    pub control_enable_latch_c: bool,
    pub control_enable_latch_extern_on_positive_edge: bool,
    pub control_set_counter: bool,
    pub control_enable_latch_extern_on_negative_edge: bool,
    pub set_counter_value: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceCompactStmControl {
    pub control_enable: bool,
    pub control_reset: bool,
    pub control_reduce_torque: bool,
    pub control_digital_output_1: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceCompactPosControlCompact {
    pub control_execute: bool,
    pub control_emergency_stop: bool,
    pub target_position: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceCompactOut {
    pub enc_control: EL7047PositioningInterfaceCompactEncControl,
    pub stm_control: EL7047PositioningInterfaceCompactStmControl,
    pub pos_control_compact: EL7047PositioningInterfaceCompactPosControlCompact,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceCompact {
    pub inputs: EL7047PositioningInterfaceCompactIn,
    pub outputs: EL7047PositioningInterfaceCompactOut,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceEncStatus {
    pub status_latch_c_valid: bool,
    pub status_latch_extern_valid: bool,
    pub status_set_counter_done: bool,
    pub status_counter_underflow: bool,
    pub status_counter_overflow: bool,
    pub status_extrapolation_stall: bool,
    pub status_status_of_input_a: bool,
    pub status_status_of_input_b: bool,
    pub status_status_of_input_c: bool,
    pub status_status_of_extern_latch: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
    pub counter_value: u32,
    pub latch_value: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceStmStatus {
    pub status_ready_to_enable: bool,
    pub status_ready: bool,
    pub status_warning: bool,
    pub status_error: bool,
    pub status_moving_positive: bool,
    pub status_moving_negative: bool,
    pub status_torque_reduced: bool,
    pub status_motor_stall: bool,
    pub status_digital_input_1: bool,
    pub status_digital_input_2: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfacePosStatus {
    pub status_busy: bool,
    pub status_in_target: bool,
    pub status_warning: bool,
    pub status_error: bool,
    pub status_calibrated: bool,
    pub status_accelerate: bool,
    pub status_decelerate: bool,
    pub status_ready_to_execute: bool,
    pub actual_position: u32,
    pub actual_velocity: i16,
    pub actual_drive_time: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceIn {
    pub enc_status: EL7047PositioningInterfaceEncStatus,
    pub stm_status: EL7047PositioningInterfaceStmStatus,
    pub pos_status: EL7047PositioningInterfacePosStatus,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceEncControl {
    pub control_enable_latch_c: bool,
    pub control_enable_latch_extern_on_positive_edge: bool,
    pub control_set_counter: bool,
    pub control_enable_latch_extern_on_negative_edge: bool,
    pub set_counter_value: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceStmControl {
    pub control_enable: bool,
    pub control_reset: bool,
    pub control_reduce_torque: bool,
    pub control_digital_output_1: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfacePosControl {
    pub control_execute: bool,
    pub control_emergency_stop: bool,
    pub target_position: u32,
    pub velocity: i16,
    pub start_type: u16,
    pub acceleration: u16,
    pub deceleration: u16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceOut {
    pub enc_control: EL7047PositioningInterfaceEncControl,
    pub stm_control: EL7047PositioningInterfaceStmControl,
    pub pos_control: EL7047PositioningInterfacePosControl,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterface {
    pub inputs: EL7047PositioningInterfaceIn,
    pub outputs: EL7047PositioningInterfaceOut,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceWithInfoDataEncStatus {
    pub status_latch_c_valid: bool,
    pub status_latch_extern_valid: bool,
    pub status_set_counter_done: bool,
    pub status_counter_underflow: bool,
    pub status_counter_overflow: bool,
    pub status_extrapolation_stall: bool,
    pub status_status_of_input_a: bool,
    pub status_status_of_input_b: bool,
    pub status_status_of_input_c: bool,
    pub status_status_of_extern_latch: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
    pub counter_value: u32,
    pub latch_value: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceWithInfoDataStmStatus {
    pub status_ready_to_enable: bool,
    pub status_ready: bool,
    pub status_warning: bool,
    pub status_error: bool,
    pub status_moving_positive: bool,
    pub status_moving_negative: bool,
    pub status_torque_reduced: bool,
    pub status_motor_stall: bool,
    pub status_digital_input_1: bool,
    pub status_digital_input_2: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceWithInfoDataStmSynchronInfoData {
    pub info_data_1: u16,
    pub info_data_2: u16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceWithInfoDataPosStatus {
    pub status_busy: bool,
    pub status_in_target: bool,
    pub status_warning: bool,
    pub status_error: bool,
    pub status_calibrated: bool,
    pub status_accelerate: bool,
    pub status_decelerate: bool,
    pub status_ready_to_execute: bool,
    pub actual_position: u32,
    pub actual_velocity: i16,
    pub actual_drive_time: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceWithInfoDataIn {
    pub enc_status: EL7047PositioningInterfaceWithInfoDataEncStatus,
    pub stm_status: EL7047PositioningInterfaceWithInfoDataStmStatus,
    pub stm_synchron_info_data: EL7047PositioningInterfaceWithInfoDataStmSynchronInfoData,
    pub pos_status: EL7047PositioningInterfaceWithInfoDataPosStatus,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceWithInfoDataEncControl {
    pub control_enable_latch_c: bool,
    pub control_enable_latch_extern_on_positive_edge: bool,
    pub control_set_counter: bool,
    pub control_enable_latch_extern_on_negative_edge: bool,
    pub set_counter_value: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceWithInfoDataStmControl {
    pub control_enable: bool,
    pub control_reset: bool,
    pub control_reduce_torque: bool,
    pub control_digital_output_1: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceWithInfoDataPosControl {
    pub control_execute: bool,
    pub control_emergency_stop: bool,
    pub target_position: u32,
    pub velocity: i16,
    pub start_type: u16,
    pub acceleration: u16,
    pub deceleration: u16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceWithInfoDataOut {
    pub enc_control: EL7047PositioningInterfaceWithInfoDataEncControl,
    pub stm_control: EL7047PositioningInterfaceWithInfoDataStmControl,
    pub pos_control: EL7047PositioningInterfaceWithInfoDataPosControl,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceWithInfoData {
    pub inputs: EL7047PositioningInterfaceWithInfoDataIn,
    pub outputs: EL7047PositioningInterfaceWithInfoDataOut,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartEncStatus {
    pub status_latch_c_valid: bool,
    pub status_latch_extern_valid: bool,
    pub status_set_counter_done: bool,
    pub status_counter_underflow: bool,
    pub status_counter_overflow: bool,
    pub status_extrapolation_stall: bool,
    pub status_status_of_input_a: bool,
    pub status_status_of_input_b: bool,
    pub status_status_of_input_c: bool,
    pub status_status_of_extern_latch: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
    pub counter_value: u32,
    pub latch_value: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartStmStatus {
    pub status_ready_to_enable: bool,
    pub status_ready: bool,
    pub status_warning: bool,
    pub status_error: bool,
    pub status_moving_positive: bool,
    pub status_moving_negative: bool,
    pub status_torque_reduced: bool,
    pub status_motor_stall: bool,
    pub status_digital_input_1: bool,
    pub status_digital_input_2: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartPosStatus {
    pub status_busy: bool,
    pub status_in_target: bool,
    pub status_warning: bool,
    pub status_error: bool,
    pub status_calibrated: bool,
    pub status_accelerate: bool,
    pub status_decelerate: bool,
    pub status_ready_to_execute: bool,
    pub actual_position: u32,
    pub actual_velocity: i16,
    pub actual_drive_time: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartIn {
    pub enc_status: EL7047PositioningInterfaceAutoStartEncStatus,
    pub stm_status: EL7047PositioningInterfaceAutoStartStmStatus,
    pub pos_status: EL7047PositioningInterfaceAutoStartPosStatus,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartEncControl {
    pub control_enable_latch_c: bool,
    pub control_enable_latch_extern_on_positive_edge: bool,
    pub control_set_counter: bool,
    pub control_enable_latch_extern_on_negative_edge: bool,
    pub set_counter_value: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartStmControl {
    pub control_enable: bool,
    pub control_reset: bool,
    pub control_reduce_torque: bool,
    pub control_digital_output_1: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartPosControl {
    pub control_execute: bool,
    pub control_emergency_stop: bool,
    pub target_position: u32,
    pub velocity: i16,
    pub start_type: u16,
    pub acceleration: u16,
    pub deceleration: u16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartPosControl2 {
    pub control_enable_auto_start: bool,
    pub target_position: u32,
    pub velocity: i16,
    pub start_type: u16,
    pub acceleration: u16,
    pub deceleration: u16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartOut {
    pub enc_control: EL7047PositioningInterfaceAutoStartEncControl,
    pub stm_control: EL7047PositioningInterfaceAutoStartStmControl,
    pub pos_control: EL7047PositioningInterfaceAutoStartPosControl,
    pub pos_control_2: EL7047PositioningInterfaceAutoStartPosControl2,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStart {
    pub inputs: EL7047PositioningInterfaceAutoStartIn,
    pub outputs: EL7047PositioningInterfaceAutoStartOut,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartWithInfoDataEncStatus {
    pub status_latch_c_valid: bool,
    pub status_latch_extern_valid: bool,
    pub status_set_counter_done: bool,
    pub status_counter_underflow: bool,
    pub status_counter_overflow: bool,
    pub status_extrapolation_stall: bool,
    pub status_status_of_input_a: bool,
    pub status_status_of_input_b: bool,
    pub status_status_of_input_c: bool,
    pub status_status_of_extern_latch: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
    pub counter_value: u32,
    pub latch_value: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartWithInfoDataStmStatus {
    pub status_ready_to_enable: bool,
    pub status_ready: bool,
    pub status_warning: bool,
    pub status_error: bool,
    pub status_moving_positive: bool,
    pub status_moving_negative: bool,
    pub status_torque_reduced: bool,
    pub status_motor_stall: bool,
    pub status_digital_input_1: bool,
    pub status_digital_input_2: bool,
    pub status_sync_error: bool,
    pub status_tx_pdo_toggle: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartWithInfoDataStmSynchronInfoData {
    pub info_data_1: u16,
    pub info_data_2: u16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartWithInfoDataPosStatus {
    pub status_busy: bool,
    pub status_in_target: bool,
    pub status_warning: bool,
    pub status_error: bool,
    pub status_calibrated: bool,
    pub status_accelerate: bool,
    pub status_decelerate: bool,
    pub status_ready_to_execute: bool,
    pub actual_position: u32,
    pub actual_velocity: i16,
    pub actual_drive_time: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartWithInfoDataIn {
    pub enc_status: EL7047PositioningInterfaceAutoStartWithInfoDataEncStatus,
    pub stm_status: EL7047PositioningInterfaceAutoStartWithInfoDataStmStatus,
    pub stm_synchron_info_data: EL7047PositioningInterfaceAutoStartWithInfoDataStmSynchronInfoData,
    pub pos_status: EL7047PositioningInterfaceAutoStartWithInfoDataPosStatus,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartWithInfoDataEncControl {
    pub control_enable_latch_c: bool,
    pub control_enable_latch_extern_on_positive_edge: bool,
    pub control_set_counter: bool,
    pub control_enable_latch_extern_on_negative_edge: bool,
    pub set_counter_value: u32,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartWithInfoDataStmControl {
    pub control_enable: bool,
    pub control_reset: bool,
    pub control_reduce_torque: bool,
    pub control_digital_output_1: bool,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartWithInfoDataPosControl {
    pub control_execute: bool,
    pub control_emergency_stop: bool,
    pub target_position: u32,
    pub velocity: i16,
    pub start_type: u16,
    pub acceleration: u16,
    pub deceleration: u16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartWithInfoDataPosControl2 {
    pub control_enable_auto_start: bool,
    pub target_position: u32,
    pub velocity: i16,
    pub start_type: u16,
    pub acceleration: u16,
    pub deceleration: u16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartWithInfoDataOut {
    pub enc_control: EL7047PositioningInterfaceAutoStartWithInfoDataEncControl,
    pub stm_control: EL7047PositioningInterfaceAutoStartWithInfoDataStmControl,
    pub pos_control: EL7047PositioningInterfaceAutoStartWithInfoDataPosControl,
    pub pos_control_2: EL7047PositioningInterfaceAutoStartWithInfoDataPosControl2,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047PositioningInterfaceAutoStartWithInfoData {
    pub inputs: EL7047PositioningInterfaceAutoStartWithInfoDataIn,
    pub outputs: EL7047PositioningInterfaceAutoStartWithInfoDataOut,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Clone)]
pub enum EL7047OpMode {
    VelocityControlCompact(EL7047VelocityControlCompact),
    VelocityControlCompactWithInfoData(EL7047VelocityControlCompactWithInfoData),
    VelocityControl(EL7047VelocityControl),
    PositionControl(EL7047PositionControl),
    PositioningInterfaceCompact(EL7047PositioningInterfaceCompact),
    PositioningInterface(EL7047PositioningInterface),
    PositioningInterfaceWithInfoData(EL7047PositioningInterfaceWithInfoData),
    PositioningInterfaceAutoStart(EL7047PositioningInterfaceAutoStart),
    PositioningInterfaceAutoStartWithInfoData(
        EL7047PositioningInterfaceAutoStartWithInfoData,
    ),
}
impl Default for EL7047OpMode {
    fn default() -> Self {
        Self::VelocityControlCompact(Default::default())
    }
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL7047 {
    pub mode: EL7047OpMode,
}
impl EL7047 {
    /// The Rx/Tx PDO-assignment index lists (0x1C12/0x1C13) for the
    /// active mode. (issue #70)
    #[must_use]
    pub fn pdo_assignment(&self) -> PdoAssignment<'static> {
        match &self.mode {
            EL7047OpMode::VelocityControlCompact(_) => {
                PdoAssignment {
                    rx: &[5632u16, 5634u16, 5636u16],
                    tx: &[6656u16, 6659u16],
                }
            }
            EL7047OpMode::VelocityControlCompactWithInfoData(_) => {
                PdoAssignment {
                    rx: &[5632u16, 5634u16, 5636u16],
                    tx: &[6656u16, 6659u16, 6660u16],
                }
            }
            EL7047OpMode::VelocityControl(_) => {
                PdoAssignment {
                    rx: &[5633u16, 5634u16, 5636u16],
                    tx: &[6657u16, 6659u16],
                }
            }
            EL7047OpMode::PositionControl(_) => {
                PdoAssignment {
                    rx: &[5633u16, 5634u16, 5635u16],
                    tx: &[6657u16, 6659u16],
                }
            }
            EL7047OpMode::PositioningInterfaceCompact(_) => {
                PdoAssignment {
                    rx: &[5633u16, 5634u16, 5637u16],
                    tx: &[6657u16, 6659u16, 6662u16],
                }
            }
            EL7047OpMode::PositioningInterface(_) => {
                PdoAssignment {
                    rx: &[5633u16, 5634u16, 5638u16],
                    tx: &[6657u16, 6659u16, 6663u16],
                }
            }
            EL7047OpMode::PositioningInterfaceWithInfoData(_) => {
                PdoAssignment {
                    rx: &[5633u16, 5634u16, 5638u16],
                    tx: &[6657u16, 6659u16, 6660u16, 6663u16],
                }
            }
            EL7047OpMode::PositioningInterfaceAutoStart(_) => {
                PdoAssignment {
                    rx: &[5633u16, 5634u16, 5638u16, 5639u16],
                    tx: &[6657u16, 6659u16, 6663u16],
                }
            }
            EL7047OpMode::PositioningInterfaceAutoStartWithInfoData(_) => {
                PdoAssignment {
                    rx: &[5633u16, 5634u16, 5638u16, 5639u16],
                    tx: &[6657u16, 6659u16, 6660u16, 6663u16],
                }
            }
        }
    }
}
pub const EL7047_REV00170000: taktora_ethercat_esi_rt::Identity = taktora_ethercat_esi_rt::Identity {
    vendor_id: 2u32,
    product_code: 461844562u32,
    revision: 1507328u32,
};
impl taktora_ethercat_esi_rt::EsiDevice for EL7047 {
    fn identity(&self) -> taktora_ethercat_esi_rt::Identity {
        EL7047_REV00170000
    }
    fn input_len(&self) -> usize {
        match &self.mode {
            EL7047OpMode::VelocityControlCompact(_) => 8usize,
            EL7047OpMode::VelocityControlCompactWithInfoData(_) => 12usize,
            EL7047OpMode::VelocityControl(_) => 12usize,
            EL7047OpMode::PositionControl(_) => 12usize,
            EL7047OpMode::PositioningInterfaceCompact(_) => 14usize,
            EL7047OpMode::PositioningInterface(_) => 24usize,
            EL7047OpMode::PositioningInterfaceWithInfoData(_) => 28usize,
            EL7047OpMode::PositioningInterfaceAutoStart(_) => 24usize,
            EL7047OpMode::PositioningInterfaceAutoStartWithInfoData(_) => 28usize,
        }
    }
    fn output_len(&self) -> usize {
        match &self.mode {
            EL7047OpMode::VelocityControlCompact(_) => 8usize,
            EL7047OpMode::VelocityControlCompactWithInfoData(_) => 8usize,
            EL7047OpMode::VelocityControl(_) => 10usize,
            EL7047OpMode::PositionControl(_) => 12usize,
            EL7047OpMode::PositioningInterfaceCompact(_) => 14usize,
            EL7047OpMode::PositioningInterface(_) => 22usize,
            EL7047OpMode::PositioningInterfaceWithInfoData(_) => 22usize,
            EL7047OpMode::PositioningInterfaceAutoStart(_) => 36usize,
            EL7047OpMode::PositioningInterfaceAutoStartWithInfoData(_) => 36usize,
        }
    }
    fn decode_inputs(
        &mut self,
        bits: &taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        use bitvec::field::BitField as _;
        match &mut self.mode {
            EL7047OpMode::VelocityControlCompact(m) => {
                const NEED: usize = 64usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                m.inputs.enc_status_compact.status_latch_c_valid = bits[0usize];
                m.inputs.enc_status_compact.status_latch_extern_valid = bits[1usize];
                m.inputs.enc_status_compact.status_set_counter_done = bits[2usize];
                m.inputs.enc_status_compact.status_counter_underflow = bits[3usize];
                m.inputs.enc_status_compact.status_counter_overflow = bits[4usize];
                m.inputs.enc_status_compact.status_extrapolation_stall = bits[7usize];
                m.inputs.enc_status_compact.status_status_of_input_a = bits[8usize];
                m.inputs.enc_status_compact.status_status_of_input_b = bits[9usize];
                m.inputs.enc_status_compact.status_status_of_input_c = bits[10usize];
                m.inputs.enc_status_compact.status_status_of_extern_latch = bits[12usize];
                m.inputs.enc_status_compact.status_sync_error = bits[13usize];
                m.inputs.enc_status_compact.status_tx_pdo_toggle = bits[15usize];
                m.inputs.enc_status_compact.counter_value = bits[16usize..32usize]
                    .load_le::<u16>();
                m.inputs.enc_status_compact.latch_value = bits[32usize..48usize]
                    .load_le::<u16>();
                m.inputs.stm_status.status_ready_to_enable = bits[48usize];
                m.inputs.stm_status.status_ready = bits[49usize];
                m.inputs.stm_status.status_warning = bits[50usize];
                m.inputs.stm_status.status_error = bits[51usize];
                m.inputs.stm_status.status_moving_positive = bits[52usize];
                m.inputs.stm_status.status_moving_negative = bits[53usize];
                m.inputs.stm_status.status_torque_reduced = bits[54usize];
                m.inputs.stm_status.status_motor_stall = bits[55usize];
                m.inputs.stm_status.status_digital_input_1 = bits[59usize];
                m.inputs.stm_status.status_digital_input_2 = bits[60usize];
                m.inputs.stm_status.status_sync_error = bits[61usize];
                m.inputs.stm_status.status_tx_pdo_toggle = bits[63usize];
            }
            EL7047OpMode::VelocityControlCompactWithInfoData(m) => {
                const NEED: usize = 96usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                m.inputs.enc_status_compact.status_latch_c_valid = bits[0usize];
                m.inputs.enc_status_compact.status_latch_extern_valid = bits[1usize];
                m.inputs.enc_status_compact.status_set_counter_done = bits[2usize];
                m.inputs.enc_status_compact.status_counter_underflow = bits[3usize];
                m.inputs.enc_status_compact.status_counter_overflow = bits[4usize];
                m.inputs.enc_status_compact.status_extrapolation_stall = bits[7usize];
                m.inputs.enc_status_compact.status_status_of_input_a = bits[8usize];
                m.inputs.enc_status_compact.status_status_of_input_b = bits[9usize];
                m.inputs.enc_status_compact.status_status_of_input_c = bits[10usize];
                m.inputs.enc_status_compact.status_status_of_extern_latch = bits[12usize];
                m.inputs.enc_status_compact.status_sync_error = bits[13usize];
                m.inputs.enc_status_compact.status_tx_pdo_toggle = bits[15usize];
                m.inputs.enc_status_compact.counter_value = bits[16usize..32usize]
                    .load_le::<u16>();
                m.inputs.enc_status_compact.latch_value = bits[32usize..48usize]
                    .load_le::<u16>();
                m.inputs.stm_status.status_ready_to_enable = bits[48usize];
                m.inputs.stm_status.status_ready = bits[49usize];
                m.inputs.stm_status.status_warning = bits[50usize];
                m.inputs.stm_status.status_error = bits[51usize];
                m.inputs.stm_status.status_moving_positive = bits[52usize];
                m.inputs.stm_status.status_moving_negative = bits[53usize];
                m.inputs.stm_status.status_torque_reduced = bits[54usize];
                m.inputs.stm_status.status_motor_stall = bits[55usize];
                m.inputs.stm_status.status_digital_input_1 = bits[59usize];
                m.inputs.stm_status.status_digital_input_2 = bits[60usize];
                m.inputs.stm_status.status_sync_error = bits[61usize];
                m.inputs.stm_status.status_tx_pdo_toggle = bits[63usize];
                m.inputs.stm_synchron_info_data.info_data_1 = bits[64usize..80usize]
                    .load_le::<u16>();
                m.inputs.stm_synchron_info_data.info_data_2 = bits[80usize..96usize]
                    .load_le::<u16>();
            }
            EL7047OpMode::VelocityControl(m) => {
                const NEED: usize = 96usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                m.inputs.enc_status.status_latch_c_valid = bits[0usize];
                m.inputs.enc_status.status_latch_extern_valid = bits[1usize];
                m.inputs.enc_status.status_set_counter_done = bits[2usize];
                m.inputs.enc_status.status_counter_underflow = bits[3usize];
                m.inputs.enc_status.status_counter_overflow = bits[4usize];
                m.inputs.enc_status.status_extrapolation_stall = bits[7usize];
                m.inputs.enc_status.status_status_of_input_a = bits[8usize];
                m.inputs.enc_status.status_status_of_input_b = bits[9usize];
                m.inputs.enc_status.status_status_of_input_c = bits[10usize];
                m.inputs.enc_status.status_status_of_extern_latch = bits[12usize];
                m.inputs.enc_status.status_sync_error = bits[13usize];
                m.inputs.enc_status.status_tx_pdo_toggle = bits[15usize];
                m.inputs.enc_status.counter_value = bits[16usize..48usize]
                    .load_le::<u32>();
                m.inputs.enc_status.latch_value = bits[48usize..80usize]
                    .load_le::<u32>();
                m.inputs.stm_status.status_ready_to_enable = bits[80usize];
                m.inputs.stm_status.status_ready = bits[81usize];
                m.inputs.stm_status.status_warning = bits[82usize];
                m.inputs.stm_status.status_error = bits[83usize];
                m.inputs.stm_status.status_moving_positive = bits[84usize];
                m.inputs.stm_status.status_moving_negative = bits[85usize];
                m.inputs.stm_status.status_torque_reduced = bits[86usize];
                m.inputs.stm_status.status_motor_stall = bits[87usize];
                m.inputs.stm_status.status_digital_input_1 = bits[91usize];
                m.inputs.stm_status.status_digital_input_2 = bits[92usize];
                m.inputs.stm_status.status_sync_error = bits[93usize];
                m.inputs.stm_status.status_tx_pdo_toggle = bits[95usize];
            }
            EL7047OpMode::PositionControl(m) => {
                const NEED: usize = 96usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                m.inputs.enc_status.status_latch_c_valid = bits[0usize];
                m.inputs.enc_status.status_latch_extern_valid = bits[1usize];
                m.inputs.enc_status.status_set_counter_done = bits[2usize];
                m.inputs.enc_status.status_counter_underflow = bits[3usize];
                m.inputs.enc_status.status_counter_overflow = bits[4usize];
                m.inputs.enc_status.status_extrapolation_stall = bits[7usize];
                m.inputs.enc_status.status_status_of_input_a = bits[8usize];
                m.inputs.enc_status.status_status_of_input_b = bits[9usize];
                m.inputs.enc_status.status_status_of_input_c = bits[10usize];
                m.inputs.enc_status.status_status_of_extern_latch = bits[12usize];
                m.inputs.enc_status.status_sync_error = bits[13usize];
                m.inputs.enc_status.status_tx_pdo_toggle = bits[15usize];
                m.inputs.enc_status.counter_value = bits[16usize..48usize]
                    .load_le::<u32>();
                m.inputs.enc_status.latch_value = bits[48usize..80usize]
                    .load_le::<u32>();
                m.inputs.stm_status.status_ready_to_enable = bits[80usize];
                m.inputs.stm_status.status_ready = bits[81usize];
                m.inputs.stm_status.status_warning = bits[82usize];
                m.inputs.stm_status.status_error = bits[83usize];
                m.inputs.stm_status.status_moving_positive = bits[84usize];
                m.inputs.stm_status.status_moving_negative = bits[85usize];
                m.inputs.stm_status.status_torque_reduced = bits[86usize];
                m.inputs.stm_status.status_motor_stall = bits[87usize];
                m.inputs.stm_status.status_digital_input_1 = bits[91usize];
                m.inputs.stm_status.status_digital_input_2 = bits[92usize];
                m.inputs.stm_status.status_sync_error = bits[93usize];
                m.inputs.stm_status.status_tx_pdo_toggle = bits[95usize];
            }
            EL7047OpMode::PositioningInterfaceCompact(m) => {
                const NEED: usize = 112usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                m.inputs.enc_status.status_latch_c_valid = bits[0usize];
                m.inputs.enc_status.status_latch_extern_valid = bits[1usize];
                m.inputs.enc_status.status_set_counter_done = bits[2usize];
                m.inputs.enc_status.status_counter_underflow = bits[3usize];
                m.inputs.enc_status.status_counter_overflow = bits[4usize];
                m.inputs.enc_status.status_extrapolation_stall = bits[7usize];
                m.inputs.enc_status.status_status_of_input_a = bits[8usize];
                m.inputs.enc_status.status_status_of_input_b = bits[9usize];
                m.inputs.enc_status.status_status_of_input_c = bits[10usize];
                m.inputs.enc_status.status_status_of_extern_latch = bits[12usize];
                m.inputs.enc_status.status_sync_error = bits[13usize];
                m.inputs.enc_status.status_tx_pdo_toggle = bits[15usize];
                m.inputs.enc_status.counter_value = bits[16usize..48usize]
                    .load_le::<u32>();
                m.inputs.enc_status.latch_value = bits[48usize..80usize]
                    .load_le::<u32>();
                m.inputs.stm_status.status_ready_to_enable = bits[80usize];
                m.inputs.stm_status.status_ready = bits[81usize];
                m.inputs.stm_status.status_warning = bits[82usize];
                m.inputs.stm_status.status_error = bits[83usize];
                m.inputs.stm_status.status_moving_positive = bits[84usize];
                m.inputs.stm_status.status_moving_negative = bits[85usize];
                m.inputs.stm_status.status_torque_reduced = bits[86usize];
                m.inputs.stm_status.status_motor_stall = bits[87usize];
                m.inputs.stm_status.status_digital_input_1 = bits[91usize];
                m.inputs.stm_status.status_digital_input_2 = bits[92usize];
                m.inputs.stm_status.status_sync_error = bits[93usize];
                m.inputs.stm_status.status_tx_pdo_toggle = bits[95usize];
                m.inputs.pos_status_compact.status_busy = bits[96usize];
                m.inputs.pos_status_compact.status_in_target = bits[97usize];
                m.inputs.pos_status_compact.status_warning = bits[98usize];
                m.inputs.pos_status_compact.status_error = bits[99usize];
                m.inputs.pos_status_compact.status_calibrated = bits[100usize];
                m.inputs.pos_status_compact.status_accelerate = bits[101usize];
                m.inputs.pos_status_compact.status_decelerate = bits[102usize];
                m.inputs.pos_status_compact.status_ready_to_execute = bits[103usize];
            }
            EL7047OpMode::PositioningInterface(m) => {
                const NEED: usize = 192usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                m.inputs.enc_status.status_latch_c_valid = bits[0usize];
                m.inputs.enc_status.status_latch_extern_valid = bits[1usize];
                m.inputs.enc_status.status_set_counter_done = bits[2usize];
                m.inputs.enc_status.status_counter_underflow = bits[3usize];
                m.inputs.enc_status.status_counter_overflow = bits[4usize];
                m.inputs.enc_status.status_extrapolation_stall = bits[7usize];
                m.inputs.enc_status.status_status_of_input_a = bits[8usize];
                m.inputs.enc_status.status_status_of_input_b = bits[9usize];
                m.inputs.enc_status.status_status_of_input_c = bits[10usize];
                m.inputs.enc_status.status_status_of_extern_latch = bits[12usize];
                m.inputs.enc_status.status_sync_error = bits[13usize];
                m.inputs.enc_status.status_tx_pdo_toggle = bits[15usize];
                m.inputs.enc_status.counter_value = bits[16usize..48usize]
                    .load_le::<u32>();
                m.inputs.enc_status.latch_value = bits[48usize..80usize]
                    .load_le::<u32>();
                m.inputs.stm_status.status_ready_to_enable = bits[80usize];
                m.inputs.stm_status.status_ready = bits[81usize];
                m.inputs.stm_status.status_warning = bits[82usize];
                m.inputs.stm_status.status_error = bits[83usize];
                m.inputs.stm_status.status_moving_positive = bits[84usize];
                m.inputs.stm_status.status_moving_negative = bits[85usize];
                m.inputs.stm_status.status_torque_reduced = bits[86usize];
                m.inputs.stm_status.status_motor_stall = bits[87usize];
                m.inputs.stm_status.status_digital_input_1 = bits[91usize];
                m.inputs.stm_status.status_digital_input_2 = bits[92usize];
                m.inputs.stm_status.status_sync_error = bits[93usize];
                m.inputs.stm_status.status_tx_pdo_toggle = bits[95usize];
                m.inputs.pos_status.status_busy = bits[96usize];
                m.inputs.pos_status.status_in_target = bits[97usize];
                m.inputs.pos_status.status_warning = bits[98usize];
                m.inputs.pos_status.status_error = bits[99usize];
                m.inputs.pos_status.status_calibrated = bits[100usize];
                m.inputs.pos_status.status_accelerate = bits[101usize];
                m.inputs.pos_status.status_decelerate = bits[102usize];
                m.inputs.pos_status.status_ready_to_execute = bits[103usize];
                m.inputs.pos_status.actual_position = bits[112usize..144usize]
                    .load_le::<u32>();
                m.inputs.pos_status.actual_velocity = bits[144usize..160usize]
                    .load_le::<i16>();
                m.inputs.pos_status.actual_drive_time = bits[160usize..192usize]
                    .load_le::<u32>();
            }
            EL7047OpMode::PositioningInterfaceWithInfoData(m) => {
                const NEED: usize = 224usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                m.inputs.enc_status.status_latch_c_valid = bits[0usize];
                m.inputs.enc_status.status_latch_extern_valid = bits[1usize];
                m.inputs.enc_status.status_set_counter_done = bits[2usize];
                m.inputs.enc_status.status_counter_underflow = bits[3usize];
                m.inputs.enc_status.status_counter_overflow = bits[4usize];
                m.inputs.enc_status.status_extrapolation_stall = bits[7usize];
                m.inputs.enc_status.status_status_of_input_a = bits[8usize];
                m.inputs.enc_status.status_status_of_input_b = bits[9usize];
                m.inputs.enc_status.status_status_of_input_c = bits[10usize];
                m.inputs.enc_status.status_status_of_extern_latch = bits[12usize];
                m.inputs.enc_status.status_sync_error = bits[13usize];
                m.inputs.enc_status.status_tx_pdo_toggle = bits[15usize];
                m.inputs.enc_status.counter_value = bits[16usize..48usize]
                    .load_le::<u32>();
                m.inputs.enc_status.latch_value = bits[48usize..80usize]
                    .load_le::<u32>();
                m.inputs.stm_status.status_ready_to_enable = bits[80usize];
                m.inputs.stm_status.status_ready = bits[81usize];
                m.inputs.stm_status.status_warning = bits[82usize];
                m.inputs.stm_status.status_error = bits[83usize];
                m.inputs.stm_status.status_moving_positive = bits[84usize];
                m.inputs.stm_status.status_moving_negative = bits[85usize];
                m.inputs.stm_status.status_torque_reduced = bits[86usize];
                m.inputs.stm_status.status_motor_stall = bits[87usize];
                m.inputs.stm_status.status_digital_input_1 = bits[91usize];
                m.inputs.stm_status.status_digital_input_2 = bits[92usize];
                m.inputs.stm_status.status_sync_error = bits[93usize];
                m.inputs.stm_status.status_tx_pdo_toggle = bits[95usize];
                m.inputs.stm_synchron_info_data.info_data_1 = bits[96usize..112usize]
                    .load_le::<u16>();
                m.inputs.stm_synchron_info_data.info_data_2 = bits[112usize..128usize]
                    .load_le::<u16>();
                m.inputs.pos_status.status_busy = bits[128usize];
                m.inputs.pos_status.status_in_target = bits[129usize];
                m.inputs.pos_status.status_warning = bits[130usize];
                m.inputs.pos_status.status_error = bits[131usize];
                m.inputs.pos_status.status_calibrated = bits[132usize];
                m.inputs.pos_status.status_accelerate = bits[133usize];
                m.inputs.pos_status.status_decelerate = bits[134usize];
                m.inputs.pos_status.status_ready_to_execute = bits[135usize];
                m.inputs.pos_status.actual_position = bits[144usize..176usize]
                    .load_le::<u32>();
                m.inputs.pos_status.actual_velocity = bits[176usize..192usize]
                    .load_le::<i16>();
                m.inputs.pos_status.actual_drive_time = bits[192usize..224usize]
                    .load_le::<u32>();
            }
            EL7047OpMode::PositioningInterfaceAutoStart(m) => {
                const NEED: usize = 192usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                m.inputs.enc_status.status_latch_c_valid = bits[0usize];
                m.inputs.enc_status.status_latch_extern_valid = bits[1usize];
                m.inputs.enc_status.status_set_counter_done = bits[2usize];
                m.inputs.enc_status.status_counter_underflow = bits[3usize];
                m.inputs.enc_status.status_counter_overflow = bits[4usize];
                m.inputs.enc_status.status_extrapolation_stall = bits[7usize];
                m.inputs.enc_status.status_status_of_input_a = bits[8usize];
                m.inputs.enc_status.status_status_of_input_b = bits[9usize];
                m.inputs.enc_status.status_status_of_input_c = bits[10usize];
                m.inputs.enc_status.status_status_of_extern_latch = bits[12usize];
                m.inputs.enc_status.status_sync_error = bits[13usize];
                m.inputs.enc_status.status_tx_pdo_toggle = bits[15usize];
                m.inputs.enc_status.counter_value = bits[16usize..48usize]
                    .load_le::<u32>();
                m.inputs.enc_status.latch_value = bits[48usize..80usize]
                    .load_le::<u32>();
                m.inputs.stm_status.status_ready_to_enable = bits[80usize];
                m.inputs.stm_status.status_ready = bits[81usize];
                m.inputs.stm_status.status_warning = bits[82usize];
                m.inputs.stm_status.status_error = bits[83usize];
                m.inputs.stm_status.status_moving_positive = bits[84usize];
                m.inputs.stm_status.status_moving_negative = bits[85usize];
                m.inputs.stm_status.status_torque_reduced = bits[86usize];
                m.inputs.stm_status.status_motor_stall = bits[87usize];
                m.inputs.stm_status.status_digital_input_1 = bits[91usize];
                m.inputs.stm_status.status_digital_input_2 = bits[92usize];
                m.inputs.stm_status.status_sync_error = bits[93usize];
                m.inputs.stm_status.status_tx_pdo_toggle = bits[95usize];
                m.inputs.pos_status.status_busy = bits[96usize];
                m.inputs.pos_status.status_in_target = bits[97usize];
                m.inputs.pos_status.status_warning = bits[98usize];
                m.inputs.pos_status.status_error = bits[99usize];
                m.inputs.pos_status.status_calibrated = bits[100usize];
                m.inputs.pos_status.status_accelerate = bits[101usize];
                m.inputs.pos_status.status_decelerate = bits[102usize];
                m.inputs.pos_status.status_ready_to_execute = bits[103usize];
                m.inputs.pos_status.actual_position = bits[112usize..144usize]
                    .load_le::<u32>();
                m.inputs.pos_status.actual_velocity = bits[144usize..160usize]
                    .load_le::<i16>();
                m.inputs.pos_status.actual_drive_time = bits[160usize..192usize]
                    .load_le::<u32>();
            }
            EL7047OpMode::PositioningInterfaceAutoStartWithInfoData(m) => {
                const NEED: usize = 224usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                m.inputs.enc_status.status_latch_c_valid = bits[0usize];
                m.inputs.enc_status.status_latch_extern_valid = bits[1usize];
                m.inputs.enc_status.status_set_counter_done = bits[2usize];
                m.inputs.enc_status.status_counter_underflow = bits[3usize];
                m.inputs.enc_status.status_counter_overflow = bits[4usize];
                m.inputs.enc_status.status_extrapolation_stall = bits[7usize];
                m.inputs.enc_status.status_status_of_input_a = bits[8usize];
                m.inputs.enc_status.status_status_of_input_b = bits[9usize];
                m.inputs.enc_status.status_status_of_input_c = bits[10usize];
                m.inputs.enc_status.status_status_of_extern_latch = bits[12usize];
                m.inputs.enc_status.status_sync_error = bits[13usize];
                m.inputs.enc_status.status_tx_pdo_toggle = bits[15usize];
                m.inputs.enc_status.counter_value = bits[16usize..48usize]
                    .load_le::<u32>();
                m.inputs.enc_status.latch_value = bits[48usize..80usize]
                    .load_le::<u32>();
                m.inputs.stm_status.status_ready_to_enable = bits[80usize];
                m.inputs.stm_status.status_ready = bits[81usize];
                m.inputs.stm_status.status_warning = bits[82usize];
                m.inputs.stm_status.status_error = bits[83usize];
                m.inputs.stm_status.status_moving_positive = bits[84usize];
                m.inputs.stm_status.status_moving_negative = bits[85usize];
                m.inputs.stm_status.status_torque_reduced = bits[86usize];
                m.inputs.stm_status.status_motor_stall = bits[87usize];
                m.inputs.stm_status.status_digital_input_1 = bits[91usize];
                m.inputs.stm_status.status_digital_input_2 = bits[92usize];
                m.inputs.stm_status.status_sync_error = bits[93usize];
                m.inputs.stm_status.status_tx_pdo_toggle = bits[95usize];
                m.inputs.stm_synchron_info_data.info_data_1 = bits[96usize..112usize]
                    .load_le::<u16>();
                m.inputs.stm_synchron_info_data.info_data_2 = bits[112usize..128usize]
                    .load_le::<u16>();
                m.inputs.pos_status.status_busy = bits[128usize];
                m.inputs.pos_status.status_in_target = bits[129usize];
                m.inputs.pos_status.status_warning = bits[130usize];
                m.inputs.pos_status.status_error = bits[131usize];
                m.inputs.pos_status.status_calibrated = bits[132usize];
                m.inputs.pos_status.status_accelerate = bits[133usize];
                m.inputs.pos_status.status_decelerate = bits[134usize];
                m.inputs.pos_status.status_ready_to_execute = bits[135usize];
                m.inputs.pos_status.actual_position = bits[144usize..176usize]
                    .load_le::<u32>();
                m.inputs.pos_status.actual_velocity = bits[176usize..192usize]
                    .load_le::<i16>();
                m.inputs.pos_status.actual_drive_time = bits[192usize..224usize]
                    .load_le::<u32>();
            }
        }
        Ok(())
    }
    fn encode_outputs(
        &self,
        bits: &mut taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        use bitvec::field::BitField as _;
        match &self.mode {
            EL7047OpMode::VelocityControlCompact(m) => {
                const NEED: usize = 64usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                bits.set(0usize, m.outputs.enc_control_compact.control_enable_latch_c);
                bits.set(
                    1usize,
                    m
                        .outputs
                        .enc_control_compact
                        .control_enable_latch_extern_on_positive_edge,
                );
                bits.set(2usize, m.outputs.enc_control_compact.control_set_counter);
                bits.set(
                    3usize,
                    m
                        .outputs
                        .enc_control_compact
                        .control_enable_latch_extern_on_negative_edge,
                );
                bits[16usize..32usize]
                    .store_le::<u16>(m.outputs.enc_control_compact.set_counter_value);
                bits.set(32usize, m.outputs.stm_control.control_enable);
                bits.set(33usize, m.outputs.stm_control.control_reset);
                bits.set(34usize, m.outputs.stm_control.control_reduce_torque);
                bits.set(43usize, m.outputs.stm_control.control_digital_output_1);
                bits[48usize..64usize].store_le::<i16>(m.outputs.stm_velocity.velocity);
            }
            EL7047OpMode::VelocityControlCompactWithInfoData(m) => {
                const NEED: usize = 64usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                bits.set(0usize, m.outputs.enc_control_compact.control_enable_latch_c);
                bits.set(
                    1usize,
                    m
                        .outputs
                        .enc_control_compact
                        .control_enable_latch_extern_on_positive_edge,
                );
                bits.set(2usize, m.outputs.enc_control_compact.control_set_counter);
                bits.set(
                    3usize,
                    m
                        .outputs
                        .enc_control_compact
                        .control_enable_latch_extern_on_negative_edge,
                );
                bits[16usize..32usize]
                    .store_le::<u16>(m.outputs.enc_control_compact.set_counter_value);
                bits.set(32usize, m.outputs.stm_control.control_enable);
                bits.set(33usize, m.outputs.stm_control.control_reset);
                bits.set(34usize, m.outputs.stm_control.control_reduce_torque);
                bits.set(43usize, m.outputs.stm_control.control_digital_output_1);
                bits[48usize..64usize].store_le::<i16>(m.outputs.stm_velocity.velocity);
            }
            EL7047OpMode::VelocityControl(m) => {
                const NEED: usize = 80usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                bits.set(0usize, m.outputs.enc_control.control_enable_latch_c);
                bits.set(
                    1usize,
                    m.outputs.enc_control.control_enable_latch_extern_on_positive_edge,
                );
                bits.set(2usize, m.outputs.enc_control.control_set_counter);
                bits.set(
                    3usize,
                    m.outputs.enc_control.control_enable_latch_extern_on_negative_edge,
                );
                bits[16usize..48usize]
                    .store_le::<u32>(m.outputs.enc_control.set_counter_value);
                bits.set(48usize, m.outputs.stm_control.control_enable);
                bits.set(49usize, m.outputs.stm_control.control_reset);
                bits.set(50usize, m.outputs.stm_control.control_reduce_torque);
                bits.set(59usize, m.outputs.stm_control.control_digital_output_1);
                bits[64usize..80usize].store_le::<i16>(m.outputs.stm_velocity.velocity);
            }
            EL7047OpMode::PositionControl(m) => {
                const NEED: usize = 96usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                bits.set(0usize, m.outputs.enc_control.control_enable_latch_c);
                bits.set(
                    1usize,
                    m.outputs.enc_control.control_enable_latch_extern_on_positive_edge,
                );
                bits.set(2usize, m.outputs.enc_control.control_set_counter);
                bits.set(
                    3usize,
                    m.outputs.enc_control.control_enable_latch_extern_on_negative_edge,
                );
                bits[16usize..48usize]
                    .store_le::<u32>(m.outputs.enc_control.set_counter_value);
                bits.set(48usize, m.outputs.stm_control.control_enable);
                bits.set(49usize, m.outputs.stm_control.control_reset);
                bits.set(50usize, m.outputs.stm_control.control_reduce_torque);
                bits.set(59usize, m.outputs.stm_control.control_digital_output_1);
                bits[64usize..96usize].store_le::<u32>(m.outputs.stm_position.position);
            }
            EL7047OpMode::PositioningInterfaceCompact(m) => {
                const NEED: usize = 112usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                bits.set(0usize, m.outputs.enc_control.control_enable_latch_c);
                bits.set(
                    1usize,
                    m.outputs.enc_control.control_enable_latch_extern_on_positive_edge,
                );
                bits.set(2usize, m.outputs.enc_control.control_set_counter);
                bits.set(
                    3usize,
                    m.outputs.enc_control.control_enable_latch_extern_on_negative_edge,
                );
                bits[16usize..48usize]
                    .store_le::<u32>(m.outputs.enc_control.set_counter_value);
                bits.set(48usize, m.outputs.stm_control.control_enable);
                bits.set(49usize, m.outputs.stm_control.control_reset);
                bits.set(50usize, m.outputs.stm_control.control_reduce_torque);
                bits.set(59usize, m.outputs.stm_control.control_digital_output_1);
                bits.set(64usize, m.outputs.pos_control_compact.control_execute);
                bits.set(65usize, m.outputs.pos_control_compact.control_emergency_stop);
                bits[80usize..112usize]
                    .store_le::<u32>(m.outputs.pos_control_compact.target_position);
            }
            EL7047OpMode::PositioningInterface(m) => {
                const NEED: usize = 176usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                bits.set(0usize, m.outputs.enc_control.control_enable_latch_c);
                bits.set(
                    1usize,
                    m.outputs.enc_control.control_enable_latch_extern_on_positive_edge,
                );
                bits.set(2usize, m.outputs.enc_control.control_set_counter);
                bits.set(
                    3usize,
                    m.outputs.enc_control.control_enable_latch_extern_on_negative_edge,
                );
                bits[16usize..48usize]
                    .store_le::<u32>(m.outputs.enc_control.set_counter_value);
                bits.set(48usize, m.outputs.stm_control.control_enable);
                bits.set(49usize, m.outputs.stm_control.control_reset);
                bits.set(50usize, m.outputs.stm_control.control_reduce_torque);
                bits.set(59usize, m.outputs.stm_control.control_digital_output_1);
                bits.set(64usize, m.outputs.pos_control.control_execute);
                bits.set(65usize, m.outputs.pos_control.control_emergency_stop);
                bits[80usize..112usize]
                    .store_le::<u32>(m.outputs.pos_control.target_position);
                bits[112usize..128usize].store_le::<i16>(m.outputs.pos_control.velocity);
                bits[128usize..144usize]
                    .store_le::<u16>(m.outputs.pos_control.start_type);
                bits[144usize..160usize]
                    .store_le::<u16>(m.outputs.pos_control.acceleration);
                bits[160usize..176usize]
                    .store_le::<u16>(m.outputs.pos_control.deceleration);
            }
            EL7047OpMode::PositioningInterfaceWithInfoData(m) => {
                const NEED: usize = 176usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                bits.set(0usize, m.outputs.enc_control.control_enable_latch_c);
                bits.set(
                    1usize,
                    m.outputs.enc_control.control_enable_latch_extern_on_positive_edge,
                );
                bits.set(2usize, m.outputs.enc_control.control_set_counter);
                bits.set(
                    3usize,
                    m.outputs.enc_control.control_enable_latch_extern_on_negative_edge,
                );
                bits[16usize..48usize]
                    .store_le::<u32>(m.outputs.enc_control.set_counter_value);
                bits.set(48usize, m.outputs.stm_control.control_enable);
                bits.set(49usize, m.outputs.stm_control.control_reset);
                bits.set(50usize, m.outputs.stm_control.control_reduce_torque);
                bits.set(59usize, m.outputs.stm_control.control_digital_output_1);
                bits.set(64usize, m.outputs.pos_control.control_execute);
                bits.set(65usize, m.outputs.pos_control.control_emergency_stop);
                bits[80usize..112usize]
                    .store_le::<u32>(m.outputs.pos_control.target_position);
                bits[112usize..128usize].store_le::<i16>(m.outputs.pos_control.velocity);
                bits[128usize..144usize]
                    .store_le::<u16>(m.outputs.pos_control.start_type);
                bits[144usize..160usize]
                    .store_le::<u16>(m.outputs.pos_control.acceleration);
                bits[160usize..176usize]
                    .store_le::<u16>(m.outputs.pos_control.deceleration);
            }
            EL7047OpMode::PositioningInterfaceAutoStart(m) => {
                const NEED: usize = 288usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                bits.set(0usize, m.outputs.enc_control.control_enable_latch_c);
                bits.set(
                    1usize,
                    m.outputs.enc_control.control_enable_latch_extern_on_positive_edge,
                );
                bits.set(2usize, m.outputs.enc_control.control_set_counter);
                bits.set(
                    3usize,
                    m.outputs.enc_control.control_enable_latch_extern_on_negative_edge,
                );
                bits[16usize..48usize]
                    .store_le::<u32>(m.outputs.enc_control.set_counter_value);
                bits.set(48usize, m.outputs.stm_control.control_enable);
                bits.set(49usize, m.outputs.stm_control.control_reset);
                bits.set(50usize, m.outputs.stm_control.control_reduce_torque);
                bits.set(59usize, m.outputs.stm_control.control_digital_output_1);
                bits.set(64usize, m.outputs.pos_control.control_execute);
                bits.set(65usize, m.outputs.pos_control.control_emergency_stop);
                bits[80usize..112usize]
                    .store_le::<u32>(m.outputs.pos_control.target_position);
                bits[112usize..128usize].store_le::<i16>(m.outputs.pos_control.velocity);
                bits[128usize..144usize]
                    .store_le::<u16>(m.outputs.pos_control.start_type);
                bits[144usize..160usize]
                    .store_le::<u16>(m.outputs.pos_control.acceleration);
                bits[160usize..176usize]
                    .store_le::<u16>(m.outputs.pos_control.deceleration);
                bits.set(178usize, m.outputs.pos_control_2.control_enable_auto_start);
                bits[192usize..224usize]
                    .store_le::<u32>(m.outputs.pos_control_2.target_position);
                bits[224usize..240usize]
                    .store_le::<i16>(m.outputs.pos_control_2.velocity);
                bits[240usize..256usize]
                    .store_le::<u16>(m.outputs.pos_control_2.start_type);
                bits[256usize..272usize]
                    .store_le::<u16>(m.outputs.pos_control_2.acceleration);
                bits[272usize..288usize]
                    .store_le::<u16>(m.outputs.pos_control_2.deceleration);
            }
            EL7047OpMode::PositioningInterfaceAutoStartWithInfoData(m) => {
                const NEED: usize = 288usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                bits.set(0usize, m.outputs.enc_control.control_enable_latch_c);
                bits.set(
                    1usize,
                    m.outputs.enc_control.control_enable_latch_extern_on_positive_edge,
                );
                bits.set(2usize, m.outputs.enc_control.control_set_counter);
                bits.set(
                    3usize,
                    m.outputs.enc_control.control_enable_latch_extern_on_negative_edge,
                );
                bits[16usize..48usize]
                    .store_le::<u32>(m.outputs.enc_control.set_counter_value);
                bits.set(48usize, m.outputs.stm_control.control_enable);
                bits.set(49usize, m.outputs.stm_control.control_reset);
                bits.set(50usize, m.outputs.stm_control.control_reduce_torque);
                bits.set(59usize, m.outputs.stm_control.control_digital_output_1);
                bits.set(64usize, m.outputs.pos_control.control_execute);
                bits.set(65usize, m.outputs.pos_control.control_emergency_stop);
                bits[80usize..112usize]
                    .store_le::<u32>(m.outputs.pos_control.target_position);
                bits[112usize..128usize].store_le::<i16>(m.outputs.pos_control.velocity);
                bits[128usize..144usize]
                    .store_le::<u16>(m.outputs.pos_control.start_type);
                bits[144usize..160usize]
                    .store_le::<u16>(m.outputs.pos_control.acceleration);
                bits[160usize..176usize]
                    .store_le::<u16>(m.outputs.pos_control.deceleration);
                bits.set(178usize, m.outputs.pos_control_2.control_enable_auto_start);
                bits[192usize..224usize]
                    .store_le::<u32>(m.outputs.pos_control_2.target_position);
                bits[224usize..240usize]
                    .store_le::<i16>(m.outputs.pos_control_2.velocity);
                bits[240usize..256usize]
                    .store_le::<u16>(m.outputs.pos_control_2.start_type);
                bits[256usize..272usize]
                    .store_le::<u16>(m.outputs.pos_control_2.acceleration);
                bits[272usize..288usize]
                    .store_le::<u16>(m.outputs.pos_control_2.deceleration);
            }
        }
        Ok(())
    }
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL3001_likeDefaultIn {
    pub underrange: bool,
    pub value: i16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL3001_likeDefaultOut {}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL3001_likeDefault {
    pub inputs: EL3001_likeDefaultIn,
    pub outputs: EL3001_likeDefaultOut,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Clone)]
pub enum EL3001_likeOpMode {
    Default(EL3001_likeDefault),
}
impl Default for EL3001_likeOpMode {
    fn default() -> Self {
        Self::Default(Default::default())
    }
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct EL3001_like {
    pub mode: EL3001_likeOpMode,
}
impl EL3001_like {
    /// The Rx/Tx PDO-assignment index lists (0x1C12/0x1C13) for the
    /// active mode. (issue #70)
    #[must_use]
    pub fn pdo_assignment(&self) -> PdoAssignment<'static> {
        match &self.mode {
            EL3001_likeOpMode::Default(_) => {
                PdoAssignment {
                    rx: &[],
                    tx: &[6656u16],
                }
            }
        }
    }
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
        match &self.mode {
            EL3001_likeOpMode::Default(_) => 3usize,
        }
    }
    fn output_len(&self) -> usize {
        match &self.mode {
            EL3001_likeOpMode::Default(_) => 0usize,
        }
    }
    fn decode_inputs(
        &mut self,
        bits: &taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        use bitvec::field::BitField as _;
        match &mut self.mode {
            EL3001_likeOpMode::Default(m) => {
                const NEED: usize = 24usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                m.inputs.underrange = bits[0usize];
                m.inputs.value = bits[8usize..24usize].load_le::<i16>();
            }
        }
        Ok(())
    }
    fn encode_outputs(
        &self,
        bits: &mut taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        match &self.mode {
            EL3001_likeOpMode::Default(m) => {
                const NEED: usize = 0usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
            }
        }
        Ok(())
    }
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct ALTDefaultStandard {
    pub entry_6000_1: u16,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct ALTDefaultCompact {
    pub entry_6000_1: u8,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct ALTDefaultIn {
    pub standard: ALTDefaultStandard,
    pub compact: ALTDefaultCompact,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct ALTDefaultOut {}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct ALTDefault {
    pub inputs: ALTDefaultIn,
    pub outputs: ALTDefaultOut,
}
#[allow(non_camel_case_types)]
#[derive(Debug, Clone)]
pub enum ALTOpMode {
    Default(ALTDefault),
}
impl Default for ALTOpMode {
    fn default() -> Self {
        Self::Default(Default::default())
    }
}
#[allow(non_camel_case_types)]
#[derive(Debug, Default, Clone)]
pub struct ALT {
    pub mode: ALTOpMode,
}
impl ALT {
    /// The Rx/Tx PDO-assignment index lists (0x1C12/0x1C13) for the
    /// active mode. (issue #70)
    #[must_use]
    pub fn pdo_assignment(&self) -> PdoAssignment<'static> {
        match &self.mode {
            ALTOpMode::Default(_) => {
                PdoAssignment {
                    rx: &[],
                    tx: &[6656u16, 6657u16],
                }
            }
        }
    }
}
pub const ALT_REV00000001: taktora_ethercat_esi_rt::Identity = taktora_ethercat_esi_rt::Identity {
    vendor_id: 2u32,
    product_code: 65536u32,
    revision: 1u32,
};
impl taktora_ethercat_esi_rt::EsiDevice for ALT {
    fn identity(&self) -> taktora_ethercat_esi_rt::Identity {
        ALT_REV00000001
    }
    fn input_len(&self) -> usize {
        match &self.mode {
            ALTOpMode::Default(_) => 3usize,
        }
    }
    fn output_len(&self) -> usize {
        match &self.mode {
            ALTOpMode::Default(_) => 0usize,
        }
    }
    fn decode_inputs(
        &mut self,
        bits: &taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        use bitvec::field::BitField as _;
        match &mut self.mode {
            ALTOpMode::Default(m) => {
                const NEED: usize = 24usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
                m.inputs.standard.entry_6000_1 = bits[0usize..16usize].load_le::<u16>();
                m.inputs.compact.entry_6000_1 = bits[16usize..24usize].load_le::<u8>();
            }
        }
        Ok(())
    }
    fn encode_outputs(
        &self,
        bits: &mut taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
    ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
        match &self.mode {
            ALTOpMode::Default(m) => {
                const NEED: usize = 0usize;
                if bits.len() < NEED {
                    return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                        expected_bits: NEED,
                        got_bits: bits.len(),
                    });
                }
            }
        }
        Ok(())
    }
}
/// Rx/Tx PDO-assignment index lists (0x1C12/0x1C13) for a device's
/// active mode. Returned by each device's `pdo_assignment()`.
#[derive(Debug, Clone, Copy)]
pub struct PdoAssignment<'a> {
    pub rx: &'a [u16],
    pub tx: &'a [u16],
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
        EL7047_REV00170000,
        || Box::new(EL7047::default()) as Box<dyn taktora_ethercat_esi_rt::EsiDevice>,
    ),
    (
        EL3001_LIKE_REV00100000,
        || {
            Box::new(EL3001_like::default())
                as Box<dyn taktora_ethercat_esi_rt::EsiDevice>
        },
    ),
    (
        ALT_REV00000001,
        || Box::new(ALT::default()) as Box<dyn taktora_ethercat_esi_rt::EsiDevice>,
    ),
];
/// Construct a fresh device instance for the given identity, if known.
pub fn device_for(
    identity: taktora_ethercat_esi_rt::Identity,
) -> Option<Box<dyn taktora_ethercat_esi_rt::EsiDevice>> {
    REGISTRY.iter().find(|(id, _)| *id == identity).map(|(_, make)| make())
}
