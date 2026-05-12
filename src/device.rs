use crate::error::Error;
use crate::regs::{Block, read_i2c, write_demod_reg, write_reg};

use nusb::MaybeFuture;
use nusb::transfer::TransferError;

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
        write_reg(&self.interface, Block::Usb, 0x2000, &[0x09])?;
        // Register 0x2158 holds the maximum packet size in bytes
        // Set to 512 bytes (USB 2.0 high-speed bulk transfer maximum)
        write_reg(&self.interface, Block::Usb, 0x2158, &[0x00, 0x02])?;
        // Register 0x2148 bit 9 clears any stale FIFO data, bit 4 stalls to prevent reading during
        // initialisation
        write_reg(&self.interface, Block::Usb, 0x2148, &[0x10, 0x02])?;

        // Allow read/write to demod pages
        write_reg(&self.interface, Block::Sys, 0x3000, &[0xe8])?;
        // Undocumented but possible crystal or clock related
        write_reg(&self.interface, Block::Sys, 0x300b, &[0x22])?;

        // Undocumented but possibly resets demodulator
        write_demod_reg(&self.interface, 1, 0x01, &[0x14])?;
        write_demod_reg(&self.interface, 1, 0x01, &[0x10])?;

        // Disable spectrum inversion
        write_demod_reg(&self.interface, 1, 0x15, &[0x00])?;
        // Clear IF frequency
        write_demod_reg(&self.interface, 1, 0x19, &[0x00])?;
        write_demod_reg(&self.interface, 1, 0x1a, &[0x00])?;
        write_demod_reg(&self.interface, 1, 0x1b, &[0x00])?;
        // Clear remaining DDC registers
        write_demod_reg(&self.interface, 1, 0x16, &[0x00])?;
        write_demod_reg(&self.interface, 1, 0x17, &[0x00])?;
        write_demod_reg(&self.interface, 1, 0x18, &[0x00])?;

        // Set FIR coefficients
        write_demod_reg(&self.interface, 1, 0x1c, &FIR_COEFFS)?;

        // Enable digital automatic gain control (DAGC)
        write_demod_reg(&self.interface, 1, 0x11, &[0x01])?;
        // Enable zero-IF input mode with DC offset cancellation and IQ mismatch compensation
        write_demod_reg(&self.interface, 1, 0xb1, &[0x1b])?;

        // Disable stall
        write_reg(&self.interface, Block::Usb, 0x2148, &[0x00, 0x00])?;

        //Ok(UntunedDevice{ interface: self.interface })
        Ok(())
    }

    fn detect_tuner(&self) -> Result<u8, Error> {
        let i2c = I2cRepeater::open(&self.interface)?;
        let value = i2c.read(TUNER_ADDR, 0x00, 1)?;
        if value[0] != TUNER_CHIP_ID {
            return Err(Error::TunerNotFound);
        }
        Ok(value[0])
    }
}

struct I2cRepeater<'a> {
    interface: &'a nusb::Interface,
}

impl<'a> I2cRepeater<'a> {
    fn open(interface: &'a nusb::Interface) -> Result<Self, TransferError> {
        // bit 3 opens i2c repeater, bit 4 is undocumented and might not be needed
        write_demod_reg(interface, 1, 0x01, &[0x18])?;
        Ok(Self { interface })
    }

    fn read(&self, dev_addr: u8, reg_addr: u8, length: u16) -> Result<Vec<u8>, TransferError> {
        read_i2c(self.interface, dev_addr, reg_addr, length)
    }
}

impl Drop for I2cRepeater<'_> {
    fn drop(&mut self) {
        if let Err(e) = write_demod_reg(self.interface, 1, 0x01, &[0x10]) {
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
    use super::FIR_COEFFS;

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
}
