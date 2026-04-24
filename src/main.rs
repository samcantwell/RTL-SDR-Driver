use nusb::MaybeFuture;
use nusb::transfer::{ControlIn, ControlOut, ControlType, Recipient, TransferError};
use std::time::Duration;

const RTL_VID: u16 = 0x0bda;
const RTL_PID: u16 = 0x2838;
const TUNER_PROBES: [TunerProbe; 2] = [
    TunerProbe {
        addr: 0x34,
        chip_id: 0x69,
        tuner: Tuner::R820T,
    },
    TunerProbe {
        addr: 0x74,
        chip_id: 0x69,
        tuner: Tuner::R828D,
    },
];

enum Block {
    Usb = 1,
    Sys = 2,
    I2c = 6,
}

enum Tuner {
    R820T,
    R828D,
}

struct TunerProbe {
    addr: u8,
    chip_id: u8,
    tuner: Tuner,
}

fn main() {
    let device_info = nusb::list_devices()
        .wait()
        .unwrap()
        .find(|dev| dev.vendor_id() == RTL_VID && dev.product_id() == RTL_PID)
        .expect("RTL-SDR not connected");
    let device = device_info.open().wait().expect("failed to open device");
    let interface = device
        .claim_interface(0)
        .wait()
        .expect("failed to claim interface");

    rtlsdr_init(&interface).unwrap();
    let tuner = detect_tuner(&interface).unwrap();
    match tuner {
        Some(Tuner::R820T) => eprintln!("Tuner R820T found"),
        Some(Tuner::R828D) => eprintln!("Tuner R828D found"),
        None => eprintln!("Tuner not found"),
    }
}

fn detect_tuner(interface: &nusb::Interface) -> Result<Option<Tuner>, TransferError> {
    // bit 3 opens i2c repeater, bit 4 is undocumented and might not be needed
    write_demod_reg(interface, 1, 0x01, &[0x18])?;

    let tuner = TUNER_PROBES.into_iter().find_map(
        |TunerProbe {
             addr,
             chip_id,
             tuner,
         }| {
            read_i2c(interface, addr, 0x00, 1)
                .ok()
                .filter(|data| *data == [chip_id])
                .map(|_| tuner)
        },
    );

    // Close i2c repeater
    write_demod_reg(interface, 1, 0x01, &[0x10])?;

    Ok(tuner)
}

fn rtlsdr_init(interface: &nusb::Interface) -> Result<(), TransferError> {
    // Prepare endpoint for bulk streaming
    // Register 0x2000 bit 0 enables DMA, bit 3 enables full packet mode
    write_reg(interface, Block::Usb, 0x2000, &[0x09])?;
    // Register 0x2158 holds the maximum packet size in bytes
    // Set to 512 bytes (USB 2.0 high-speed bulk transfer maximum)
    write_reg(interface, Block::Usb, 0x2158, &[0x00, 0x02])?;
    // Register 0x2148 bit 9 clears any stale FIFO data, bit 4 stalls to prevent reading during
    // initialisation
    write_reg(interface, Block::Usb, 0x2148, &[0x10, 0x02])?;

    // Allow read/write to demod pages
    write_reg(interface, Block::Sys, 0x3000, &[0xe8])?;
    // Undocumented but possible crystal or clock related
    write_reg(interface, Block::Sys, 0x300b, &[0x22])?;

    // Undocumented but possibly resets demodulator
    write_demod_reg(interface, 1, 0x01, &[0x14])?;
    write_demod_reg(interface, 1, 0x01, &[0x10])?;

    // Disable spectrum inversion
    write_demod_reg(interface, 1, 0x15, &[0x00])?;
    // Clear IF frequency
    write_demod_reg(interface, 1, 0x19, &[0x00])?;
    write_demod_reg(interface, 1, 0x1a, &[0x00])?;
    write_demod_reg(interface, 1, 0x1b, &[0x00])?;
    // Clear remaining DDC registers
    write_demod_reg(interface, 1, 0x16, &[0x00])?;
    write_demod_reg(interface, 1, 0x17, &[0x00])?;
    write_demod_reg(interface, 1, 0x18, &[0x00])?;

    // Set FIR coefficients
    let fir: [u8; 20] = [
        0xCA, 0xDC, 0xD7, 0xD8, 0xE0, 0xF2, 0x0E,
        0x35, // -54, -36, -41, -40, -32, -14, 14, 53
        0x65, 0xC0, 0x09, // (101, 156)
        0xD7, 0x10, 0x11, // (215, 273)
        0x47, 0x41, 0x17, // (327, 372)
        0x94, 0x51, 0x1A, // (404, 421)
    ];

    write_demod_reg(interface, 1, 0x1c, &fir)?;

    // Enable digital automatic gain control (DAGC)
    write_demod_reg(interface, 1, 0x11, &[0x01])?;
    // Enable zero-IF input mode with DC offset cancellation and IQ mismatch compensation
    write_demod_reg(interface, 1, 0xb1, &[0x1b])?;

    // Disable stall
    write_reg(interface, Block::Usb, 0x2148, &[0x00, 0x00])?;

    Ok(())
}

/// Reads `length` bytes from a register in the given block.
///
/// Sends a USB vendor control-in transfer. The register address is passed as
/// `wValue` and the block selects the target subsystem via `wIndex`.
///
/// Registers are byte-addressable and little-endian.
fn read_reg(
    interface: &nusb::Interface,
    block: Block,
    addr: u16,
    length: u16,
) -> Result<Vec<u8>, TransferError> {
    interface
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

/// Writes `data` to a register in the given block.
///
/// Sends a USB vendor control-out transfer. The `0x10` flag in `wIndex`
/// distinguishes writes from reads.
fn write_reg(
    interface: &nusb::Interface,
    block: Block,
    addr: u16,
    data: &[u8],
) -> Result<(), TransferError> {
    interface
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
fn read_demod_reg(
    interface: &nusb::Interface,
    page: u8,
    addr: u8,
    length: u16,
) -> Result<Vec<u8>, TransferError> {
    interface
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

/// Writes `data` to a demodulator register.
///
/// Uses the same `(addr << 8) | 0x20` encoding as [`read_demod_reg`], with
/// `0x10` OR'd into `wIndex` to indicate a write.
///
/// # Prerequisites
///
/// Same as [`read_demod_reg`] — `DEMOD_CTL` must be powered on first.
fn write_demod_reg(
    interface: &nusb::Interface,
    page: u8,
    addr: u8,
    data: &[u8],
) -> Result<(), TransferError> {
    // NOTE: librtlsdr does a dummy read_demod_reg(0x0a, 0x01, 1) after every demod write
    // as a flush. Add this if fast sequential writes cause issues.
    interface
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
fn read_i2c(
    interface: &nusb::Interface,
    dev_addr: u8,
    reg_addr: u8,
    length: u16,
) -> Result<Vec<u8>, TransferError> {
    interface
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

/// Writes `data` to an I2C device via the RTL2832U's I2C bridge.
///
/// Uses block 6 (`IICB`) with the same `wValue` encoding as [`read_i2c`].
/// Maximum 8 bytes per transfer — larger writes need chunking.
///
/// # Prerequisites
///
/// The I2C repeater must be enabled first (demod page 1, reg 0x01, bit 3).
fn write_i2c(
    interface: &nusb::Interface,
    dev_addr: u8,
    reg_addr: u8,
    data: &[u8],
) -> Result<(), TransferError> {
    interface
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
