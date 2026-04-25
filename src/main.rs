mod regs;
mod rtlsdr;
mod tuner;

fn main() {
    let interface = match rtlsdr::interface() {
        Ok(interface) => {
            if let Some(interface) = interface {
                interface
            } else {
                eprintln!("Could not find RTL-SDR");
                return;
            }
        }
        Err(e) => {
            eprintln!("Failed to return interface: {e}");
            return;
        }
    };

    let interface = match rtlsdr::init(interface) {
        Ok(interface) => interface,
        Err(e) => {
            eprintln!("Failed to initialise rtlsdr: {e}");
            return;
        }
    };

    let _tuner = match tuner::detect_tuner(&interface) {
        Ok(tuner) => {
            if let Some(tuner) = tuner {
                tuner
            } else {
                eprintln!("Tuner not found");
                return;
            }
        }
        Err(e) => {
            eprintln!("Tuner detection failed: {e}");
            return;
        }
    };
}
