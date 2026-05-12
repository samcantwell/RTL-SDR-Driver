use crate::error::Error;
use crate::regs::{Block, read_i2c, write_demod_reg, write_reg};

use nusb::MaybeFuture;
use nusb::transfer::TransferError;

const RTL_VID: u16 = 0x0bda;
const RTL_PID: u16 = 0x2838;

const TUNER_ADDR: u8 = 0x34;
const TUNER_CHIP_ID: u8 = 0x69;

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
        let fir: [u8; 20] = [
            0xCA, 0xDC, 0xD7, 0xD8, 0xE0, 0xF2, 0x0E,
            0x35, // -54, -36, -41, -40, -32, -14, 14, 53
            0x65, 0xC0, 0x09, // (101, 156)
            0xD7, 0x10, 0x11, // (215, 273)
            0x47, 0x41, 0x17, // (327, 372)
            0x94, 0x51, 0x1A, // (404, 421)
        ];

        write_demod_reg(&self.interface, 1, 0x1c, &fir)?;

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
        // bit 3 opens i2c repeater, bit 4 is undocumented and might not be needed
        write_demod_reg(&self.interface, 1, 0x01, &[0x18])?;

        let value = read_i2c(&self.interface, TUNER_ADDR, 0x00, 1)?;

        // Close i2c repeater
        write_demod_reg(&self.interface, 1, 0x01, &[0x10])?;

        if value[0] != TUNER_CHIP_ID {
            return Err(Error::TunerNotFound);
        }
        Ok(value[0])
    }
}
