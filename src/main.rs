use nusb::MaybeFuture;
use nusb::transfer::{ControlIn, ControlType, Recipient};
use std::time::Duration;

const RTL_VID: u16 = 0x0bda;
const RTL_PID: u16 = 0x2838;

fn main() {
    let device_info = nusb::list_devices().wait().unwrap()
        .find(|dev| dev.vendor_id() == RTL_VID && dev.product_id() == RTL_PID)
        .expect("RTL-SDR not connected");
    let device = device_info.open().wait().expect("failed to open device");
    let interface = device.claim_interface(1).wait().expect("failed to claim interface");

    let data = interface.control_in(ControlIn {
        control_type: ControlType::Vendor,
        recipient: Recipient::Device,
        request: 0,
        value: 0x2000,
        index: 0x0100,
        length: 1,
    }, Duration::from_millis(300)).wait();

    match data {
        Ok(bytes) => println!("USB_SYSCTL = 0x{:02x}", bytes[0]),
        Err(e) => eprintln!("Error reading USB_SYSCTL: {e}"),
    }
}
