use crate::error::Error;
use nusb::MaybeFuture;
use nusb::transfer::{ControlIn, ControlOut, ControlType, Recipient, TransferError};
use std::io::Read;
use std::time::Duration;

// Vendor ID and product ID of the RTL2832U
const RTL_VID: u16 = 0x0bda;
const RTL_PID: u16 = 0x2838;

const FIR_COEFFICIENTS: [i16; 16] = [
    -54, -36, -41, -40, -32, -14, 14, 53, 101, 156, 215, 273, 327, 372, 404, 421,
];

const TUNER_INIT_ARRAY: [u8; 27] = [
    0x80, 0x13, 0x70, 0xc0, 0x40, 0xdb, 0x6b, 0xeb, 0x53, 0x75, 0x68, 0x6c, 0xbb, 0x80, 0x31, 0x0f,
    0x00, 0xc0, 0x30, 0x48, 0xec, 0x60, 0x00, 0x24, 0xdd, 0x0e, 0x40,
];

enum Block {
    Usb = 1,
    Sys = 2,
    I2c = 6,
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
        //device.init_tuner()?;

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
        let mut reader = self
            .interface
            .endpoint::<nusb::transfer::Bulk, nusb::transfer::In>(0x81)?
            .reader(4096);

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
        // Init the USB endpoint
        self.write_reg(Block::Usb, 0x2000, &[0x09])?;
        // Power on the demod and ADC
        self.write_reg(Block::Sys, 0x3000, &[0xe8])?;

        // Set minimal FIR coefficients
        self.write_demod_reg(1, 0x2e, &[0x41])?;
        // Enable rawIQ mode and disable DAGC
        // TODO: why disable DAGC?
        self.write_demod_reg(0, 0x19, &[0x05])?;
        self.write_demod_reg(1, 0x94, &[0x0f])?;
        self.write_demod_reg(0, 0x06, &[0x80])?;
        self.write_demod_reg(1, 0x01, &[0x18])?;
        self.write_demod_reg(1, 0x03, &[0x80])?;
        self.write_demod_reg(1, 0x04, &[0xcc])?;
        self.write_demod_reg(0, 0x08, &[0x4d])?;

        self.write_reg(
            Block::I2c,
            0x0034,
            &[0x05, 0x80, 0x13, 0x70, 0xc0, 0x40, 0xdb, 0x6b],
        )?;
        self.write_reg(
            Block::I2c,
            0x0034,
            &[0x13, 0x31, 0x0f, 0x00, 0xc0, 0x30, 0x48, 0xec],
        )?;

        self.write_demod_reg(1, 0x15, &[0x01])?;
        self.write_demod_reg(1, 0x9f, &[0x03, 0x84])?;
        self.write_demod_reg(1, 0x01, &[0x14])?;
        self.write_demod_reg(1, 0x01, &[0x18])?;

        self.write_reg(Block::I2c, 0x34, &[0x1e, 0x4e])?;
        self.write_demod_reg(1, 0x19, &[0x3c])?;
        self.write_demod_reg(1, 0x1a, &[0x99])?;
        self.write_reg(Block::I2c, 0x34, &[0x10, 0x84])?;
        self.write_reg(Block::I2c, 0x34, &[0x14, 0x0b])?;
        self.write_reg(Block::I2c, 0x34, &[0x16, 0x76])?;
        self.write_reg(Block::I2c, 0x34, &[0x0c, 0xf0])?;

        Ok(())
    }
    /*
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

        /*
        // Below demod writes copied from driver code
        /* enable SDR mode, disable DAGC (bit 5) */
        self.write_demod_reg(0, 0x19, &[0x05])?;

        /* init FSM state-holding register */
        self.write_demod_reg(1, 0x93, &[0xf0])?;
        self.write_demod_reg(1, 0x94, &[0x0f])?;

        /* disable AGC (en_dagc, bit 0) (this seems to have no effect) */
        self.write_demod_reg(1, 0x11, &[0x00])?;

        /* disable RF and IF AGC loop */
        self.write_demod_reg(1, 0x04, &[0x00])?;

        /* disable PID filter (enable_PID = 0) */
        self.write_demod_reg(0, 0x61, &[0x60])?;

        /* opt_adc_iq = 0, default ADC_I/ADC_Q datapath */
        self.write_demod_reg(0, 0x06, &[0x80])?;

        /* Enable Zero-IF mode (en_bbin bit), DC cancellation (en_dc_est),
         * IQ estimation/compensation (en_iq_comp, en_iq_est) */
        self.write_demod_reg(1, 0xb1, &[0x1b])?;

        /* disable 4.096 MHz clock output on pin TP_CK0 */
        self.write_demod_reg(0, 0x0d, &[0x83])?;
        */

        // Unstall
        self.write_reg(Block::Usb, 0x2148, &[0x00, 0x00])?;

        Ok(())
    }
    */

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

        let tuner = self.i2c_read_reg(0x34, 0x00, 1)?[0];
        if tuner != 0x69 {
            return Err(Error::TunerNotFound);
        }

        self.i2c_close()?;
        Ok(())
    }

    fn init_tuner(&self) -> Result<(), Error> {
        self.i2c_open()?;

        self.i2c_write_reg(0x34, 0x00, &[0x05, 0b1001_0110, 0b0110_1001])?;
        let regs = self.i2c_read_reg(0x34, 0x00, 16)?;
        for reg in regs {
            println!("0x{reg:02x} 0b{reg:08b}");
        }
        //self.i2c_write_reg(0x34, 0x05, &TUNER_INIT_ARRAY)?;
        //dbg!(self.i2c_read_reg(0x34, 0x00, 32)?);

        self.i2c_close()?;
        Ok(())
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

    fn i2c_open(&self) -> Result<(), Error> {
        self.write_demod_reg(1, 0x01, &[0x18])?;
        Ok(())
    }

    fn i2c_close(&self) -> Result<(), Error> {
        self.write_demod_reg(1, 0x01, &[0x10])?;
        Ok(())
    }

    fn i2c_read_reg(
        &self,
        dev_addr: u8,
        reg_addr: u8,
        length: u16,
    ) -> Result<Vec<u8>, TransferError> {
        self.interface
            .control_in(
                ControlIn {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request: 0,
                    value: u16::from(reg_addr) << 8 | u16::from(dev_addr),
                    index: (Block::I2c as u16) << 8,
                    length,
                },
                Duration::from_millis(300),
            )
            .wait()
    }

    fn i2c_write_reg(&self, dev_addr: u8, reg_addr: u8, data: &[u8]) -> Result<(), TransferError> {
        self.interface
            .control_out(
                ControlOut {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request: 0,
                    value: u16::from(reg_addr) << 8 | u16::from(dev_addr),
                    index: (Block::I2c as u16) << 8 | 0x10,
                    data,
                },
                Duration::from_millis(300),
            )
            .wait()
    }
}
