use crate::error::Error;
use nusb::Interface;
use nusb::MaybeFuture;
use nusb::io::EndpointRead;
use nusb::transfer::{Bulk, ControlIn, ControlOut, ControlType, Recipient, TransferError};
use std::time::Duration;

const TIMEOUT_DURATION: Duration = Duration::from_millis(300);

pub enum Block {
    Usb = 1,
    Sys = 2,
    I2c = 6,
}

pub struct Transport {
    interface: Interface,
}

//struct I2cRepeater<'a> {
//    transport: &'a Transport,
//}

impl Transport {
    pub fn open(vendor_id: u16, product_id: u16) -> Result<Self, Error> {
        let device_info = nusb::list_devices()
            .wait()?
            .find(|dev| dev.vendor_id() == vendor_id && dev.product_id() == product_id)
            .ok_or(Error::DeviceNotFound)?;

        let device = device_info.open().wait()?;
        let interface = device.claim_interface(0).wait()?;

        Ok(Self { interface })
    }

    pub fn bulk_reader(&self) -> Result<EndpointRead<Bulk>, Error> {
        Ok(self
            .interface
            .endpoint::<nusb::transfer::Bulk, nusb::transfer::In>(0x81)?
            .reader(4096))
    }

    #[expect(dead_code)]
    pub fn read_reg(&self, block: Block, addr: u16, length: u16) -> Result<Vec<u8>, TransferError> {
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
                TIMEOUT_DURATION,
            )
            .wait()
    }

    pub fn write_reg(&self, block: Block, addr: u16, data: &[u8]) -> Result<(), TransferError> {
        self.interface
            .control_out(
                ControlOut {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request: 0,
                    value: addr,
                    index: (block as u16) << 8 | 0x10,
                    //data: &reversed,
                    data,
                },
                TIMEOUT_DURATION,
            )
            .wait()
    }

    #[expect(dead_code)]
    pub fn read_demod_reg(
        &self,
        page: u8,
        addr: u8,
        length: u16,
    ) -> Result<Vec<u8>, TransferError> {
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
                TIMEOUT_DURATION,
            )
            .wait()
    }

    pub fn write_demod_reg(&self, page: u8, addr: u8, data: &[u8]) -> Result<(), TransferError> {
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
                TIMEOUT_DURATION,
            )
            .wait()
    }

    //    pub fn i2c_repeater(&self) -> Result<I2cRepeater<'_>, Error> {
    //        self.write_demod_reg(1, 0x01, &[0x18])?;
    //        Ok(I2cRepeater { transport: self })
    //    }
}

//impl std::ops::Drop for I2cRepeater<'_> {
//    fn drop(&mut self) {
//        let _ = self.transport.write_demod_reg(1, 0x01, &[0x10]);
//    }
//}
