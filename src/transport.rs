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

impl Transport {
    pub fn new(interface: Interface) -> Self {
        Transport { interface }
    }

    pub fn get_bulk_reader(&self) -> Result<EndpointRead<Bulk>, Error> {
        Ok(self
            .interface
            .endpoint::<nusb::transfer::Bulk, nusb::transfer::In>(0x81)?
            .reader(4096))
    }

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
                TIMEOUT_DURATION,
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
    pub fn write_reg(&self, block: Block, addr: u16, data: &[u8]) -> Result<(), TransferError> {
        //let reversed: Vec<u8> = data.iter().rev().copied().collect();

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
}
