use crate::error::Error;
use nusb::MaybeFuture;
use nusb::transfer::{ControlOut, ControlType, Recipient, TransferError};
use std::time::Duration;

// Vendor ID and product ID of the RTL2832U
const RTL_VID: u16 = 0x0bda;
const RTL_PID: u16 = 0x2838;

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
        //device.init()?;
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
    pub fn init(&self) -> Result<(), Error> {
        self.write_reg(Block::Usb, 0x2000, &[0x09])?;
        Ok(())
    }

    fn write_reg(&self, block: Block, addr: u16, data: &[u8]) -> Result<(), TransferError> {
        self.interface
            .control_out(
                ControlOut {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request: 0,
                    value: addr,
                    index: (block as u16) << 8 | 0x10,
                    data,
                },
                Duration::from_millis(300),
            )
            .wait()
    }
}
