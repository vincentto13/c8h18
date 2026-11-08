//! Register-level OPL3 (YMF262) emulator front-end.
//!
//! The FM core is the pure-Rust `oplon` crate (MIT): 18 two-operator channels, ADSR with
//! KSR/KSL, multipliers, all 8 waveforms, feedback, tremolo/vibrato LFOs, OPL3 stereo bits.
//! This wrapper exposes the 9-bit register space (0x000..0x0FF = bank 0, 0x100..0x1FF = bank 1)
//! and renders float stereo at the output rate.

/// Native FM sample rate of the chip (14.31818 MHz / 288).
pub const NATIVE_RATE: f64 = 49716.0;
/// Number of 2-operator channels in OPL3 mode.
pub const CHANNELS: usize = 18;

/// Operator register offsets of channel ch (0..8 within a bank): (modulator, carrier).
pub fn op_offsets(ch: usize) -> (u16, u16) {
    const MOD: [u16; 9] = [0x00, 0x01, 0x02, 0x08, 0x09, 0x0a, 0x10, 0x11, 0x12];
    (MOD[ch], MOD[ch] + 3)
}

pub struct Opl3 {
    chip: oplon::Opl3,
    gain: f32,
}

impl Opl3 {
    /// New chip rendering at `rate` Hz, switched to OPL3 mode.
    pub fn new(rate: u32) -> Opl3 {
        let mut chip = oplon::Opl3::with_native_rate(NATIVE_RATE as u32, rate.max(1));
        chip.write_reg_high(0x05, 0x01); // OPL3 enable
        chip.write_reg_high(0x04, 0x00); // no 4-op channels
        chip.write_reg(0x01, 0x20); // waveform select enable (OPL2 compatibility)
        chip.write_reg(0xbd, 0x00); // melodic mode, shallow tremolo/vibrato
        Opl3 { chip, gain: 1.0 / 40000.0 }
    }

    /// Write a register: 0x000..0x0FF bank 0, 0x100..0x1FF bank 1.
    pub fn write(&mut self, reg: u16, val: u8) {
        if reg & 0x100 != 0 {
            self.chip.write_reg_high((reg & 0xff) as u8, val);
        } else {
            self.chip.write_reg((reg & 0xff) as u8, val);
        }
    }

    /// Register base for channel 0..17: (bank offset, channel within bank).
    pub fn chan_base(ch: usize) -> (u16, usize) {
        if ch < 9 { (0, ch) } else { (0x100, ch - 9) }
    }

    /// Render one stereo frame as floats (roughly -1..1 for a full mix).
    #[inline]
    pub fn frame(&mut self) -> (f32, f32) {
        let (l, r) = self.chip.render_frame();
        (l as f32 * self.gain, r as f32 * self.gain)
    }
}
