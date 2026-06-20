use rtlsdrs::{Config, Device};
use std::fs;
use std::time::Duration;

fn main() -> Result<(), rtlsdrs::Error> {
    let device = Device::open()?;
    device.configure(Config {
        frequency: 100_000_000,
        sample_rate: 2_048_000,
    })?;
    device.init()?;
    let samples = device.sample(Duration::from_secs(10))?;

    fs::write("output.bin", samples)?;
    Ok(())
}
