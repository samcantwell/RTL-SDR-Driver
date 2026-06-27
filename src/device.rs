use crate::error::Error;
use nusb::MaybeFuture;
use nusb::transfer::{ControlIn, ControlOut, ControlType, Recipient, TransferError};
use std::time::Duration;

// Vendor ID and product ID of the RTL2832U
const RTL_VID: u16 = 0x0bda;
const RTL_PID: u16 = 0x2838;

const FIR_COEFFICIENTS: [i16; 16] = [
    -54, -36, -41, -40, -32, -14, 14, 53, 101, 156, 215, 273, 327, 372, 404, 421,
];

enum Block {
    Usb = 1,
    //Sys = 2,
    //I2c = 6,
}

pub struct Device {
    interface: nusb::Interface,
}

pub struct Config {
    pub frequency: u32,
    pub sample_rate: u32,
}

impl Device {
    /// Opens and initialises the RTL-SDR device.
    ///
    /// # Errors
    ///
    /// Will return `Err` if a device with the RTL2832U's product or vendor ID is not found, if
    /// there are any USB errors during the process, or if a compatable tuner is not found on the
    /// device.
    pub fn open() -> Result<Self, Error> {
        let device = Self::find_device()?;
        device.init()?;
        //device.detect_tuner()?;

        Ok(device)
    }

    /// Configures an initialised RTL-SDR based on the provided options.
    ///
    /// # Errors
    ///
    /// Will return `Err` if there are any USB errors during the process.
    pub fn configure(&self, _config: Config) -> Result<(), Error> {
        Ok(())
    }

    /// Reads samples from a configured device for the time duration specified.
    ///
    /// # Errors
    ///
    /// Will return `Err` if there are any USB errors during the process.
    pub fn sample(&self, _duration: Duration) -> Result<Vec<u8>, Error> {
        Ok(vec![1, 2, 3, 4])
    }

    fn find_device() -> Result<Self, Error> {
        let device_info = nusb::list_devices()
            .wait()?
            .find(|dev| dev.vendor_id() == RTL_VID && dev.product_id() == RTL_PID)
            .ok_or(Error::DeviceNotFound)?;

        let device = device_info.open().wait()?;
        let interface = device.claim_interface(0).wait()?;

        Ok(Self { interface })
    }

    /// Initialises the RTL2832U and R820T chips.
    ///
    /// # Errors
    ///
    /// Will return `Err` if there are any USB errors during the process.
    fn init(&self) -> Result<(), Error> {
        // Initialise USB endpoint
        self.write_reg(Block::Usb, 0x2000, &[0x09])?;
        self.write_reg(Block::Usb, 0x2158, &[0x02, 0x00])?;
        self.write_reg(Block::Usb, 0x2148, &[0x02, 0x10])?;

        // Power on demodulator
        // TODO: Check 0x22 write is needed
        self.write_reg(Block::Usb, 0x300b, &[0x22])?;
        self.write_reg(Block::Usb, 0x3000, &[0xe8])?;

        // Reset demodulator
        self.write_demod_reg(1, 0x01, &[0x14])?;
        self.write_demod_reg(1, 0x01, &[0x10])?;

        // TODO: Check this is needed
        // Disable spectrum inversion and adjacent channel rejection
        self.write_demod_reg(1, 0x15, &[0x00])?;

        // TODO: Check what each register in this write does, not all documented
        // Clear DDC stale state and clear IF, it is set later
        self.write_demod_reg(1, 0x16, &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00])?;

        // Set FIR coefficients
        self.write_demod_reg(1, 0x1c, &Self::convert_fir(FIR_COEFFICIENTS))?;

        Ok(())
    }

    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::cast_sign_loss)]
    fn convert_fir(fir: [i16; 16]) -> Vec<u8> {
        fir[0..8]
            .iter()
            .copied()
            .chain(fir[8..16].chunks_exact(2).flat_map(|a| {
                [
                    (a[0] >> 4 & 0xff),
                    (a[0] << 4 & 0xf0 | a[1] >> 8 & 0x0f),
                    (a[1] & 0xff),
                ]
            }))
            .map(|a| a as u8)
            .collect()
    }
}

impl Device {
    #[expect(dead_code)]
    fn read_reg(&self, block: Block, addr: u16, length: u16) -> Result<Vec<u8>, TransferError> {
        self.interface
            .control_in(
                ControlIn {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request: 0,
                    value: addr,
                    index: (block as u16) << 8,
                    length,
                },
                Duration::from_millis(300),
            )
            .wait()
    }

    /// Writes `data` to consecutive registers starting at `addr`.
    ///
    /// The device expects little-endian byte order.
    /// The last byte in `data` is always written to the lowest register.
    /// That is, given `data: &[0xab, 0xcd]` and `addr: 0x0000`, the data will be written as:
    /// - 0x0000 <- 0xcd
    /// - 0x0001 <- 0xab
    ///
    fn write_reg(&self, block: Block, addr: u16, data: &[u8]) -> Result<(), TransferError> {
        let reversed: Vec<u8> = data.iter().rev().copied().collect();

        self.interface
            .control_out(
                ControlOut {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request: 0,
                    value: addr,
                    index: (block as u16) << 8 | 0x10,
                    data: &reversed,
                },
                Duration::from_millis(300),
            )
            .wait()
    }

    #[expect(dead_code)]
    fn read_demod_reg(&self, page: u8, addr: u8, length: u16) -> Result<Vec<u8>, TransferError> {
        self.interface
            .control_in(
                ControlIn {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request: 0,
                    value: u16::from(addr) << 8 | 0x20,
                    index: u16::from(page),
                    length,
                },
                Duration::from_millis(300),
            )
            .wait()
    }

    fn write_demod_reg(&self, page: u8, addr: u8, data: &[u8]) -> Result<(), TransferError> {
        self.interface
            .control_out(
                ControlOut {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request: 0,
                    value: u16::from(addr) << 8 | 0x20,
                    index: u16::from(page) | 0x10,
                    data,
                },
                Duration::from_millis(300),
            )
            .wait()
    }
}
