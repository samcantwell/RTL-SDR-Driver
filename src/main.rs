use crate::error::Error;

mod device;
mod error;

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Error> {
    let _device = device::Device::open()?;
    Ok(())
}
