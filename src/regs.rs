use nusb::MaybeFuture;
use nusb::transfer::{ControlIn, ControlOut, ControlType, Recipient, TransferError};
use std::time::Duration;

pub enum Block {
    Usb = 1,
    Sys = 2,
    I2c = 6,
}

/// Reads `length` bytes from a register in the given block.
///
/// Sends a USB vendor control-in transfer. The register address is passed as
/// `wValue` and the block selects the target subsystem via `wIndex`.
///
/// Registers are byte-addressable and little-endian.
pub fn read_reg(
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
pub fn write_reg(
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
pub fn read_demod_reg(
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
pub fn write_demod_reg(
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
pub fn read_i2c(
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
pub fn write_i2c(
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
