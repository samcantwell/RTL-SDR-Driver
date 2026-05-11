use nusb::MaybeFuture;
use nusb::transfer::{ControlIn, ControlOut, ControlType, Recipient, TransferError};
use std::time::Duration;

#[derive(Debug, Copy, Clone)]
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
#[expect(dead_code)]
pub fn read_reg(
    interface: &nusb::Interface,
    block: Block,
    addr: u16,
    length: u16,
) -> Result<Vec<u8>, TransferError> {
    interface
        .control_in(
            encode_read_reg(block, addr, length),
            Duration::from_millis(300),
        )
        .wait()
}

fn encode_read_reg(block: Block, addr: u16, length: u16) -> ControlIn {
    ControlIn {
        control_type: ControlType::Vendor,
        recipient: Recipient::Device,
        request: 0,
        value: addr,
        index: (block as u16) << 8,
        length,
    }
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
            encode_write_reg(block, addr, data),
            Duration::from_millis(300),
        )
        .wait()
}

fn encode_write_reg(block: Block, addr: u16, data: &[u8]) -> ControlOut<'_> {
    ControlOut {
        control_type: ControlType::Vendor,
        recipient: Recipient::Device,
        request: 0,
        value: addr,
        index: (block as u16) << 8 | 0x10,
        data,
    }
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
#[expect(dead_code)]
pub fn read_demod_reg(
    interface: &nusb::Interface,
    page: u8,
    addr: u8,
    length: u16,
) -> Result<Vec<u8>, TransferError> {
    interface
        .control_in(
            encode_read_demod(page, addr, length),
            Duration::from_millis(300),
        )
        .wait()
}

fn encode_read_demod(page: u8, addr: u8, length: u16) -> ControlIn {
    ControlIn {
        control_type: ControlType::Vendor,
        recipient: Recipient::Device,
        request: 0,
        value: u16::from(addr) << 8 | 0x20,
        index: u16::from(page),
        length,
    }
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
            encode_write_demod(page, addr, data),
            Duration::from_millis(300),
        )
        .wait()
}

fn encode_write_demod(page: u8, addr: u8, data: &[u8]) -> ControlOut<'_> {
    ControlOut {
        control_type: ControlType::Vendor,
        recipient: Recipient::Device,
        request: 0,
        value: u16::from(addr) << 8 | 0x20,
        index: u16::from(page) | 0x10,
        data,
    }
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
            encode_read_i2c(dev_addr, reg_addr, length),
            Duration::from_millis(300),
        )
        .wait()
}

fn encode_read_i2c(dev_addr: u8, reg_addr: u8, length: u16) -> ControlIn {
    ControlIn {
        control_type: ControlType::Vendor,
        recipient: Recipient::Device,
        request: 0,
        value: u16::from(reg_addr) << 8 | u16::from(dev_addr),
        index: (Block::I2c as u16) << 8,
        length,
    }
}

/// Writes `data` to an I2C device via the RTL2832U's I2C bridge.
///
/// Uses block 6 (`IICB`) with the same `wValue` encoding as [`read_i2c`].
/// Maximum 8 bytes per transfer — larger writes need chunking.
///
/// # Prerequisites
///
/// The I2C repeater must be enabled first (demod page 1, reg 0x01, bit 3).
#[expect(dead_code)]
pub fn write_i2c(
    interface: &nusb::Interface,
    dev_addr: u8,
    reg_addr: u8,
    data: &[u8],
) -> Result<(), TransferError> {
    interface
        .control_out(
            encode_write_i2c(dev_addr, reg_addr, data),
            Duration::from_millis(300),
        )
        .wait()
}

fn encode_write_i2c(dev_addr: u8, reg_addr: u8, data: &[u8]) -> ControlOut<'_> {
    ControlOut {
        control_type: ControlType::Vendor,
        recipient: Recipient::Device,
        request: 0,
        value: u16::from(reg_addr) << 8 | u16::from(dev_addr),
        index: (Block::I2c as u16) << 8 | 0x10,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_reg_block_encoding() {
        let cases = [
            (Block::Usb, 0x0100),
            (Block::Sys, 0x0200),
            (Block::I2c, 0x0600),
        ];
        for (block, expected_index) in cases {
            let ctrl = encode_read_reg(block, 0x0000, 1);
            assert_eq!(ctrl.index, expected_index, "block {block:?}");
        }
    }

    #[test]
    fn write_reg_block_encoding() {
        let cases = [
            (Block::Usb, 0x0110),
            (Block::Sys, 0x0210),
            (Block::I2c, 0x0610),
        ];
        for (block, expected_index) in cases {
            let ctrl = encode_write_reg(block, 0x0000, &[0x00]);
            assert_eq!(ctrl.index, expected_index, "block {block:?}");
        }
    }

    #[test]
    fn read_demod_encoding() {
        let cases = [
            (0, 0x00, 0x0020, 0),
            (2, 0x03, 0x0320, 2),
            (4, 0xff, 0xff20, 4),
        ];
        for (page, addr, expected_value, expected_index) in cases {
            let ctrl = encode_read_demod(page, addr, 1);
            assert_eq!(ctrl.index, expected_index, "page {page:?}");
            assert_eq!(ctrl.value, expected_value, "register {addr:?}");
        }
    }

    #[test]
    fn write_demod_encoding() {
        let ctrl = encode_write_demod(2, 0x03, &[0x00]);
        assert_eq!(ctrl.index, 0x12);
        assert_eq!(ctrl.value, 0x0320);
        let cases = [
            (0, 0x00, 0x0020, 0x10),
            (2, 0x03, 0x0320, 0x12),
            (4, 0xff, 0xff20, 0x14),
        ];
        for (page, addr, expected_value, expected_index) in cases {
            let ctrl = encode_write_demod(page, addr, &[0x00]);
            assert_eq!(ctrl.index, expected_index, "page {page:?}");
            assert_eq!(ctrl.value, expected_value, "register {addr:?}");
        }
    }

    #[test]
    fn read_i2c_encoding() {
        assert_eq!(encode_read_i2c(0x34, 0x56, 1).value, 0x5634);
    }

    #[test]
    fn write_i2c_encoding() {
        assert_eq!(encode_write_i2c(0x34, 0x56, &[0x00]).value, 0x5634);
    }
}
