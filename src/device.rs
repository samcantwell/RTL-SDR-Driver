use crate::error::Error;

use nusb::MaybeFuture;
use nusb::transfer::{ControlIn, ControlOut, ControlType, Recipient, TransferError};
use std::time::Duration;

const RTL_VID: u16 = 0x0bda;
const RTL_PID: u16 = 0x2838;

const TUNER_ADDR: u8 = 0x34;
const TUNER_CHIP_ID: u8 = 0x69;

// FIR low-pass coefficients for the DDC, packed per librtlsdr's `rtlsdr_set_fir()`:
//   first 8 coeffs are signed bytes; the remaining 8 are 12-bit signed values packed
//   in pairs as (a >> 4, (a << 4) | (b >> 8 & 0xF), b & 0xFF).
// Decoded values: [-54, -36, -41, -40, -32, -14, 14, 53,
//                  101, 156, 215, 273, 327, 372, 404, 421].
// NOT YET HARDWARE-VERIFIED on the RTL-SDR Blog V3. The chip stores these bytes raw
// and the filter logic is in fixed silicon, so the only way to confirm the encoding
// is correct is to sweep a CW signal through the passband once streaming lands.
// See TODO.md "Hardware verification" and CLAUDE.md "FIR coefficient packing".
const FIR_COEFFS: [u8; 20] = [
    0xCA, 0xDC, 0xD7, 0xD8, 0xE0, 0xF2, 0x0E, 0x35, // -54..53
    0x06, 0x50, 0x9C, // (101, 156)
    0x0D, 0x71, 0x11, // (215, 273)
    0x14, 0x71, 0x74, // (327, 372)
    0x19, 0x41, 0xA5, // (404, 421)
];

#[derive(Debug, Copy, Clone)]
pub enum Block {
    Usb = 1,
    Sys = 2,
    I2c = 6,
}

pub struct Device {
    interface: nusb::Interface,
}

impl Device {
    pub fn open() -> Result<Self, Error> {
        let device = Self::find_device()?;
        device.init()?;
        device.detect_tuner()?;

        Ok(device)
    }

    fn find_device() -> Result<Self, Error> {
        let Some(device_info) = nusb::list_devices()
            .wait()?
            .find(|dev| dev.vendor_id() == RTL_VID && dev.product_id() == RTL_PID)
        else {
            return Err(Error::DeviceNotFound);
        };

        let device = device_info.open().wait()?;
        let interface = device.claim_interface(0).wait()?;

        Ok(Self { interface })
    }

    fn init(&self) -> Result<(), TransferError> {
        // Prepare endpoint for bulk streaming
        // Register 0x2000 bit 0 enables DMA, bit 3 enables full packet mode
        self.write_reg(Block::Usb, 0x2000, &[0x09])?;
        // Register 0x2158 holds the maximum packet size in bytes
        // Set to 512 bytes (USB 2.0 high-speed bulk transfer maximum)
        self.write_reg(Block::Usb, 0x2158, &[0x00, 0x02])?;
        // Register 0x2148 bit 9 clears any stale FIFO data, bit 4 stalls to prevent reading during
        // initialisation
        self.write_reg(Block::Usb, 0x2148, &[0x10, 0x02])?;

        // Allow read/write to demod pages
        self.write_reg(Block::Sys, 0x3000, &[0xe8])?;
        // Undocumented but possible crystal or clock related
        self.write_reg(Block::Sys, 0x300b, &[0x22])?;

        // Undocumented but possibly resets demodulator
        self.write_demod_reg(1, 0x01, &[0x14])?;
        self.write_demod_reg(1, 0x01, &[0x10])?;

        // Disable spectrum inversion
        self.write_demod_reg(1, 0x15, &[0x00])?;
        // Clear IF frequency
        self.write_demod_reg(1, 0x19, &[0x00])?;
        self.write_demod_reg(1, 0x1a, &[0x00])?;
        self.write_demod_reg(1, 0x1b, &[0x00])?;
        // Clear remaining DDC registers
        self.write_demod_reg(1, 0x16, &[0x00])?;
        self.write_demod_reg(1, 0x17, &[0x00])?;
        self.write_demod_reg(1, 0x18, &[0x00])?;

        // Set FIR coefficients
        self.write_demod_reg(1, 0x1c, &FIR_COEFFS)?;

        // Enable digital automatic gain control (DAGC)
        self.write_demod_reg(1, 0x11, &[0x01])?;
        // Enable zero-IF input mode with DC offset cancellation and IQ mismatch compensation
        self.write_demod_reg(1, 0xb1, &[0x1b])?;

        // Disable stall
        self.write_reg(Block::Usb, 0x2148, &[0x00, 0x00])?;

        //Ok(UntunedDevice{ interface: self.interface })
        Ok(())
    }

    fn detect_tuner(&self) -> Result<(), Error> {
        let i2c = I2cRepeater::open(self)?;
        let value = i2c.read(TUNER_ADDR, 0x00, 1)?;
        if value[0] != TUNER_CHIP_ID {
            return Err(Error::TunerNotFound);
        }
        Ok(())
    }

    /// Reads `length` bytes from a register in the given block.
    ///
    /// Sends a USB vendor control-in transfer. The register address is passed as
    /// `wValue` and the block selects the target subsystem via `wIndex`.
    ///
    /// Registers are byte-addressable and little-endian.
    #[expect(dead_code)]
    fn read_reg(&self, block: Block, addr: u16, length: u16) -> Result<Vec<u8>, TransferError> {
        self.interface
            .control_in(
                Self::encode_read_reg(block, addr, length),
                Duration::from_millis(300),
            )
            .wait()
    }

    fn encode_read_reg(block: Block, addr: u16, length: u16) -> ControlIn {
        ControlIn {
            control_type: ControlType::Vendor,
            recipient: Recipient::Device,
            request: 0,
            value: addr,
            index: (block as u16) << 8,
            length,
        }
    }

    /// Writes `data` to a register in the given block.
    ///
    /// Sends a USB vendor control-out transfer. The `0x10` flag in `wIndex`
    /// distinguishes writes from reads.
    pub fn write_reg(&self, block: Block, addr: u16, data: &[u8]) -> Result<(), TransferError> {
        self.interface
            .control_out(
                Self::encode_write_reg(block, addr, data),
                Duration::from_millis(300),
            )
            .wait()
    }

    fn encode_write_reg(block: Block, addr: u16, data: &[u8]) -> ControlOut<'_> {
        ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Device,
            request: 0,
            value: addr,
            index: (block as u16) << 8 | 0x10,
            data,
        }
    }

    /// Reads `length` bytes from a demodulator register.
    ///
    /// Demod registers use a different encoding to standard blocks: `wValue` is
    /// `(addr << 8) | 0x20` and `wIndex` is the page number directly (not shifted).
    ///
    /// # Datasheet deviation
    ///
    /// The datasheet (section 11.2) shows `wValue = reg_offset` for demod access,
    /// but passing raw offsets causes a USB Stall. The `(addr << 8) | 0x20`
    /// encoding is not documented — it was found empirically and matches librtlsdr.
    ///
    /// # Prerequisites
    ///
    /// Demod registers will stall until `DEMOD_CTL` (0x3000) has bit 7 (PLL on) and
    /// bit 5 (reset released) set. Minimum: write `0xa0` to `0x3000`.
    #[expect(dead_code)]
    pub fn read_demod_reg(
        &self,
        page: u8,
        addr: u8,
        length: u16,
    ) -> Result<Vec<u8>, TransferError> {
        self.interface
            .control_in(
                Self::encode_read_demod(page, addr, length),
                Duration::from_millis(300),
            )
            .wait()
    }

    fn encode_read_demod(page: u8, addr: u8, length: u16) -> ControlIn {
        ControlIn {
            control_type: ControlType::Vendor,
            recipient: Recipient::Device,
            request: 0,
            value: u16::from(addr) << 8 | 0x20,
            index: u16::from(page),
            length,
        }
    }

    /// Writes `data` to a demodulator register.
    ///
    /// Uses the same `(addr << 8) | 0x20` encoding as [`read_demod_reg`], with
    /// `0x10` OR'd into `wIndex` to indicate a write.
    ///
    /// # Prerequisites
    ///
    /// Same as [`read_demod_reg`] — `DEMOD_CTL` must be powered on first.
    pub fn write_demod_reg(&self, page: u8, addr: u8, data: &[u8]) -> Result<(), TransferError> {
        // NOTE: librtlsdr does a dummy read_demod_reg(0x0a, 0x01, 1) after every demod write
        // as a flush. Add this if fast sequential writes cause issues.
        self.interface
            .control_out(
                Self::encode_write_demod(page, addr, data),
                Duration::from_millis(300),
            )
            .wait()
    }

    fn encode_write_demod(page: u8, addr: u8, data: &[u8]) -> ControlOut<'_> {
        ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Device,
            request: 0,
            value: u16::from(addr) << 8 | 0x20,
            index: u16::from(page) | 0x10,
            data,
        }
    }
}

struct I2cRepeater<'a> {
    device: &'a Device,
}

impl<'a> I2cRepeater<'a> {
    fn open(device: &'a Device) -> Result<Self, TransferError> {
        // bit 3 opens i2c repeater, bit 4 is undocumented and might not be needed
        device.write_demod_reg(1, 0x01, &[0x18])?;
        Ok(Self { device })
    }

    // TODO: Might be safer to use block 3 (tuner) instead of block 6 (i2c)
    /// Reads `length` bytes from an I2C device via the RTL2832U's I2C bridge.
    ///
    /// Uses block 6 (`IICB`) with `wValue = (reg_addr << 8) | dev_addr`.
    /// Maximum 8 bytes per transfer — larger reads need chunking.
    ///
    /// # Datasheet deviation
    ///
    /// The datasheet shows `wValue = (i2c_addr << 8) | reg_addr` (device address
    /// in the high byte), but this encoding fails on hardware. The working encoding
    /// is `(reg_addr << 8) | dev_addr` — verified empirically.
    ///
    /// # Prerequisites
    ///
    /// The I2C repeater must be enabled first (demod page 1, reg 0x01, bit 3).
    ///
    /// # R820T note
    ///
    /// The R820T always reads from register 0 regardless of `reg_addr`, cycling
    /// through all registers until STOP. Read data is bit-reversed (LSB-first) —
    /// apply [`u8::reverse_bits`] to each byte.
    fn read(&self, dev_addr: u8, reg_addr: u8, length: u16) -> Result<Vec<u8>, TransferError> {
        self.device
            .interface
            .control_in(
                Self::encode_read(dev_addr, reg_addr, length),
                Duration::from_millis(300),
            )
            .wait()
    }

    fn encode_read(dev_addr: u8, reg_addr: u8, length: u16) -> ControlIn {
        ControlIn {
            control_type: ControlType::Vendor,
            recipient: Recipient::Device,
            request: 0,
            value: u16::from(reg_addr) << 8 | u16::from(dev_addr),
            index: (Block::I2c as u16) << 8,
            length,
        }
    }

    /// Writes `data` to an I2C device via the RTL2832U's I2C bridge.
    ///
    /// Uses block 6 (`IICB`) with the same `wValue` encoding as [`read_i2c`].
    /// Maximum 8 bytes per transfer — larger writes need chunking.
    ///
    /// # Prerequisites
    ///
    /// The I2C repeater must be enabled first (demod page 1, reg 0x01, bit 3).
    #[expect(dead_code)]
    fn write(&self, dev_addr: u8, reg_addr: u8, data: &[u8]) -> Result<(), TransferError> {
        self.device
            .interface
            .control_out(
                Self::encode_write(dev_addr, reg_addr, data),
                Duration::from_millis(300),
            )
            .wait()
    }

    fn encode_write(dev_addr: u8, reg_addr: u8, data: &[u8]) -> ControlOut<'_> {
        ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Device,
            request: 0,
            value: u16::from(reg_addr) << 8 | u16::from(dev_addr),
            index: (Block::I2c as u16) << 8 | 0x10,
            data,
        }
    }
}

impl Drop for I2cRepeater<'_> {
    fn drop(&mut self) {
        if let Err(e) = self.device.write_demod_reg(1, 0x01, &[0x10]) {
            eprintln!("Failed to close I2cRepeater: {e}. Continuing anyway...");
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
// Casts in this module are deliberate two's-complement bit reinterpretation —
// exactly what the 8-bit/12-bit FIR packing requires.
mod tests {
    use super::*;

    // The 16 documented FIR coefficients. Indices 0-7 fit in i8 (signed byte).
    // Indices 8-15 are 12-bit signed (i16 here is just a wider holding type).
    const COEFFS: [i16; 16] = [
        -54, -36, -41, -40, -32, -14, 14, 53, 101, 156, 215, 273, 327, 372, 404, 421,
    ];

    // Packing per slides/step-4-baseband-init.md:
    //   byte 0: a[7:0]
    //   byte 1: (b[3:0] << 4) | a[11:8]
    //   byte 2: b[11:4]
    fn pack_slides(coeffs: &[i16; 16]) -> [u8; 20] {
        let mut out = [0u8; 20];
        for i in 0..8 {
            out[i] = coeffs[i] as u8;
        }
        for pair in 0..4 {
            let a = coeffs[8 + pair * 2] as u16 & 0x0FFF;
            let b = coeffs[8 + pair * 2 + 1] as u16 & 0x0FFF;
            let base = 8 + pair * 3;
            out[base] = (a & 0xFF) as u8;
            out[base + 1] = (((b & 0x0F) << 4) | ((a >> 8) & 0x0F)) as u8;
            out[base + 2] = ((b >> 4) & 0xFF) as u8;
        }
        out
    }

    // Packing per librtlsdr rtlsdr_set_fir():
    //   byte 0: a >> 4               (a[11:4])
    //   byte 1: (a << 4) | (b >> 8 & 0xF)
    //   byte 2: b & 0xFF             (b[7:0])
    fn pack_librtlsdr(coeffs: &[i16; 16]) -> [u8; 20] {
        let mut out = [0u8; 20];
        for i in 0..8 {
            out[i] = coeffs[i] as u8;
        }
        for pair in 0..4 {
            let a = coeffs[8 + pair * 2] as u16 & 0x0FFF;
            let b = coeffs[8 + pair * 2 + 1] as u16 & 0x0FFF;
            let base = 8 + pair * 3;
            out[base] = (a >> 4) as u8;
            out[base + 1] = (((a & 0x0F) << 4) | ((b >> 8) & 0x0F)) as u8;
            out[base + 2] = (b & 0xFF) as u8;
        }
        out
    }

    #[test]
    fn fir_const_matches_librtlsdr_packing() {
        assert_eq!(pack_librtlsdr(&COEFFS), FIR_COEFFS);
    }

    #[test]
    fn fir_const_does_not_match_slides_packing() {
        // Sanity: the two schemes really do produce different bytes for these coeffs.
        // If this ever fails it means the two algorithms accidentally converged,
        // and the fir_const_matches_librtlsdr_packing assertion is no longer discriminating.
        assert_ne!(pack_slides(&COEFFS), pack_librtlsdr(&COEFFS));
        assert_ne!(pack_slides(&COEFFS), FIR_COEFFS);
    }

    #[test]
    fn fir_first_eight_are_signed_bytes() {
        for i in 0..8 {
            assert_eq!(FIR_COEFFS[i], COEFFS[i] as u8, "coeff {i}");
        }
    }

    #[test]
    fn read_reg_block_encoding() {
        let cases = [
            (Block::Usb, 0x0100),
            (Block::Sys, 0x0200),
            (Block::I2c, 0x0600),
        ];
        for (block, expected_index) in cases {
            let ctrl = Device::encode_read_reg(block, 0x0000, 1);
            assert_eq!(ctrl.index, expected_index, "block {block:?}");
        }
    }

    #[test]
    fn write_reg_block_encoding() {
        let cases = [
            (Block::Usb, 0x0110),
            (Block::Sys, 0x0210),
            (Block::I2c, 0x0610),
        ];
        for (block, expected_index) in cases {
            let ctrl = Device::encode_write_reg(block, 0x0000, &[0x00]);
            assert_eq!(ctrl.index, expected_index, "block {block:?}");
        }
    }

    #[test]
    fn read_demod_encoding() {
        let cases = [
            (0, 0x00, 0x0020, 0),
            (2, 0x03, 0x0320, 2),
            (4, 0xff, 0xff20, 4),
        ];
        for (page, addr, expected_value, expected_index) in cases {
            let ctrl = Device::encode_read_demod(page, addr, 1);
            assert_eq!(ctrl.index, expected_index, "page {page:?}");
            assert_eq!(ctrl.value, expected_value, "register {addr:?}");
        }
    }

    #[test]
    fn write_demod_encoding() {
        let ctrl = Device::encode_write_demod(2, 0x03, &[0x00]);
        assert_eq!(ctrl.index, 0x12);
        assert_eq!(ctrl.value, 0x0320);
        let cases = [
            (0, 0x00, 0x0020, 0x10),
            (2, 0x03, 0x0320, 0x12),
            (4, 0xff, 0xff20, 0x14),
        ];
        for (page, addr, expected_value, expected_index) in cases {
            let ctrl = Device::encode_write_demod(page, addr, &[0x00]);
            assert_eq!(ctrl.index, expected_index, "page {page:?}");
            assert_eq!(ctrl.value, expected_value, "register {addr:?}");
        }
    }

    #[test]
    fn read_i2c_encoding() {
        assert_eq!(I2cRepeater::encode_read(0x34, 0x56, 1).value, 0x5634);
    }

    #[test]
    fn write_i2c_encoding() {
        assert_eq!(I2cRepeater::encode_write(0x34, 0x56, &[0x00]).value, 0x5634);
    }
}
