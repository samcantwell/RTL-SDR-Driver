mod regs;
mod rtlsdr;
mod tuner;

fn main() {
    let interface = match rtlsdr::interface() {
        Ok(Some(interface)) => interface,
        Ok(None) => { eprintln!("Could not find RTL-SDR"); return; }
        Err(e) => { eprintln!("USB error: {e}"); return; }
    };

    let interface = match rtlsdr::init(interface) {
        Ok(interface) => interface,
        Err(e) => { eprintln!("Initilisation failed: {e}"); return; }
    };

    let _tuner = match tuner::detect_tuner(&interface) {
        Ok(Some(tuner)) => tuner,
        Ok(None) => { eprintln!("Tuner not found"); return; }
        Err(e) => { eprintln!("Tuner detection failed: {e}"); return; }
    };
}
