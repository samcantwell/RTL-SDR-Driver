use crate::error::Error;
use crate::transport::{Block, Transport};
use nusb::MaybeFuture;
use std::io::Read;
use std::time::Duration;

// Vendor ID and product ID of the RTL2832U
const RTL_VID: u16 = 0x0bda;
const RTL_PID: u16 = 0x2838;

/*
const FIR_COEFFICIENTS: [i16; 16] = [
    -54, -36, -41, -40, -32, -14, 14, 53, 101, 156, 215, 273, 327, 372, 404, 421,
];

const TUNER_INIT_ARRAY: [u8; 27] = [
    0x80, 0x13, 0x70, 0xc0, 0x40, 0xdb, 0x6b, 0xeb, 0x53, 0x75, 0x68, 0x6c, 0xbb, 0x80, 0x31, 0x0f,
    0x00, 0xc0, 0x30, 0x48, 0xec, 0x60, 0x00, 0x24, 0xdd, 0x0e, 0x40,
];
*/

const R820T_DEV_ADDR: u16 = 0x0034;

pub struct Device {
    transport: Transport,
}

pub struct ConfiguredDevice {
    transport: Transport,
    // Consider adding config: Config here
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
        //device.init_tuner()?;

        Ok(device)
    }

    /// Configures an initialised RTL-SDR based on the provided options.
    ///
    /// # Errors
    ///
    /// Will return `Err` if there are any USB errors during the process.
    pub fn configure(self, _config: Config) -> Result<ConfiguredDevice, Error> {
        Ok(ConfiguredDevice {
            transport: self.transport,
        })
    }

    fn find_device() -> Result<Self, Error> {
        let device_info = nusb::list_devices()
            .wait()?
            .find(|dev| dev.vendor_id() == RTL_VID && dev.product_id() == RTL_PID)
            .ok_or(Error::DeviceNotFound)?;

        let device = device_info.open().wait()?;
        let interface = device.claim_interface(0).wait()?;

        Ok(Self {
            transport: Transport::new(interface),
        })
    }

    /// Initialises the RTL2832U and R820T chips.
    ///
    /// # Errors
    ///
    /// Will return `Err` if there are any USB errors during the process.
    fn init(&self) -> Result<(), Error> {
        // Init the USB endpoint
        self.transport.write_reg(Block::Usb, 0x2000, &[0x09])?;
        // Power on the demod and ADC
        self.transport.write_reg(Block::Sys, 0x3000, &[0xe8])?;

        // Set minimal FIR coefficients
        self.transport.write_demod_reg(1, 0x2e, &[0x41])?;
        // Enable rawIQ mode and disable DAGC
        // TODO: why disable DAGC?
        self.transport.write_demod_reg(0, 0x19, &[0x05])?;
        // Init FSM register
        self.transport.write_demod_reg(1, 0x94, &[0x0f])?;
        // ADC_I/ADC_Q datapath
        self.transport.write_demod_reg(0, 0x06, &[0x80])?;
        // Open I2C repeater
        self.transport.write_demod_reg(1, 0x01, &[0x18])?;
        // Set AGC target level
        self.transport.write_demod_reg(1, 0x03, &[0x80])?;
        // Set AGC loop gain
        self.transport.write_demod_reg(1, 0x04, &[0xcc])?;
        // Only enable in-phase ADC input
        self.transport.write_demod_reg(0, 0x08, &[0x4d])?;

        // Tuner initial writes
        self.transport.write_reg(
            Block::I2c,
            R820T_DEV_ADDR,
            &[0x05, 0x80, 0x13, 0x70, 0xc0, 0x40, 0xdb, 0x6b],
        )?;
        self.transport.write_reg(
            Block::I2c,
            R820T_DEV_ADDR,
            &[0x13, 0x31, 0x0f, 0x00, 0xc0, 0x30, 0x48, 0xec],
        )?;

        // Fix spectral inversion
        self.transport.write_demod_reg(1, 0x15, &[0x01])?;
        // Resampler ratio
        self.transport.write_demod_reg(1, 0x9f, &[0x03, 0x84])?;
        // Demod soft reset
        self.transport.write_demod_reg(1, 0x01, &[0x14])?;
        // Release soft reset and turn i2c back on
        self.transport.write_demod_reg(1, 0x01, &[0x18])?;

        // Enable channel filter extension
        self.transport
            .write_reg(Block::I2c, R820T_DEV_ADDR, &[0x1e, 0x4e])?;
        // Set DDC IF frequency
        self.transport.write_demod_reg(1, 0x19, &[0x3c])?;
        self.transport.write_demod_reg(1, 0x1a, &[0x99])?;
        // PLL mixer divider number
        self.transport
            .write_reg(Block::I2c, R820T_DEV_ADDR, &[0x10, 0x84])?;
        // PLL integer word
        self.transport
            .write_reg(Block::I2c, R820T_DEV_ADDR, &[0x14, 0x0b])?;
        // PLL sigma-delta
        self.transport
            .write_reg(Block::I2c, R820T_DEV_ADDR, &[0x16, 0x76])?;
        // VGA auto and ADC input path
        self.transport
            .write_reg(Block::I2c, R820T_DEV_ADDR, &[0x0c, 0xf0])?;

        Ok(())
    }
}

/*
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

    fn detect_tuner(&self) -> Result<(), Error> {
    self.i2c_open()?;

    let tuner = self.i2c_read_reg(0x00, 1)?[0];
    if tuner != 0x69 {
    return Err(Error::TunerNotFound);
    }

    self.i2c_close()?;
    Ok(())
    }

    fn init_tuner(&self) -> Result<(), Error> {
    self.i2c_open()?;

    self.i2c_write_reg(0x00, &[0x05, 0b1001_0110, 0b0110_1001])?;
    let regs = self.i2c_read_reg(0x00, 16)?;
    for reg in regs {
    println!("0x{reg:02x} 0b{reg:08b}");
    }
    //self.i2c_write_reg(0x34, 0x05, &TUNER_INIT_ARRAY)?;
    //dbg!(self.i2c_read_reg(0x34, 0x00, 32)?);

    self.i2c_close()?;
    Ok(())
    }

    fn i2c_open(&self) -> Result<(), Error> {
        self.transport.write_demod_reg(1, 0x01, &[0x18])?;
        Ok(())
    }

    fn i2c_close(&self) -> Result<(), Error> {
        self.transport.write_demod_reg(1, 0x01, &[0x10])?;
        Ok(())
    }

    fn i2c_read_reg(&self, reg_addr: u8, length: u16) -> Result<Vec<u8>, TransferError> {
        self.interface
            .control_in(
                encode_read_i2c(self.tuner.i2c_addr, reg_addr, length),
                TIMEOUT_DURATION,
            )
            .wait()
    }

    fn i2c_write_reg(&self, reg_addr: u8, data: &[u8]) -> Result<(), TransferError> {
        // TODO: Add max write limits
        // TODO: Add shadow cache
        self.interface
            .control_out(
                encode_write_i2c(self.tuner.i2c_addr, reg_addr, data),
                TIMEOUT_DURATION,
            )
            .wait()
    }
}

fn encode_read_i2c(dev_addr: u16, reg_addr: u8, length: u16) -> ControlIn {
    ControlIn {
        control_type: ControlType::Vendor,
        recipient: Recipient::Device,
        request: 0,
        value: u16::from(reg_addr) << 8 | dev_addr,
        index: (Block::I2c as u16) << 8,
        length,
    }
}

fn encode_write_i2c(dev_addr: u16, reg_addr: u8, data: &[u8]) -> ControlOut {
    ControlOut {
        control_type: ControlType::Vendor,
        recipient: Recipient::Device,
        request: 0,
        value: dev_addr,
        index: (Block::I2c as u16) << 8 | 0x10,
        // TODO: Add register addressing in data
        data,
    }
}
*/

impl ConfiguredDevice {
    /// Reads samples from a configured device for the time duration specified.
    ///
    /// # Errors
    ///
    /// Will return `Err` if there are any USB errors during the process.
    pub fn sample(&self, _duration: Duration) -> Result<Vec<u8>, Error> {
        let mut reader = self.transport.get_bulk_reader()?;

        let mut iq = Vec::new();

        let samples = 2_048_000 * 10 * 2;
        for _ in 0..(samples / 512) {
            let mut buf = [0; 512];
            reader.read_exact(&mut buf)?;

            iq.extend_from_slice(&buf);
        }

        dbg!(iq.len());
        Ok(iq)
    }
}
