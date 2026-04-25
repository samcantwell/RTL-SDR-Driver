use crate::regs::{read_i2c, write_demod_reg};

use nusb::transfer::TransferError;

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

pub enum Tuner {
    R820T,
    R828D,
}

struct TunerProbe {
    addr: u8,
    chip_id: u8,
    tuner: Tuner,
}

pub fn detect_tuner(interface: &nusb::Interface) -> Result<Option<Tuner>, TransferError> {
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
