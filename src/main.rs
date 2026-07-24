use rtlsdrs::{Config, Device};
use std::fs;
use std::time::Duration;

fn main() -> Result<(), rtlsdrs::Error> {
    let config = Config {
        frequency: 100_000_000,
        sample_rate: 2_048_000,
    };

    let device = Device::open()?;

    let device = device.configure(config)?;
    let samples = device.sample(Duration::from_secs(1))?;

    fs::write("output.bin", samples)?;
    Ok(())
}
