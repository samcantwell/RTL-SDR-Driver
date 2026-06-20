use nusb::transfer;

#[derive(Debug)]
pub enum Error {
    DeviceNotFound,
    Usb(nusb::Error),
    Transfer(nusb::transfer::TransferError),
    TunerNotFound,
    FileError(std::io::Error),
}

impl From<nusb::Error> for Error {
    fn from(e: nusb::Error) -> Self {
        Self::Usb(e)
    }
}

impl From<transfer::TransferError> for Error {
    fn from(e: transfer::TransferError) -> Self {
        Self::Transfer(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::FileError(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::DeviceNotFound => write!(f, "could not find RTL-SDR USB device"),
            Error::Transfer(e) => write!(f, "transfer error: {e}"),
            Error::TunerNotFound => write!(f, "correct tuner not found"),
            Error::Usb(e) => write!(f, "USB error: {e}"),
            Error::FileError(e) => write!(f, "File I/O error: {e}"),
        }
    }
}
