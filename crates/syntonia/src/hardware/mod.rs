//! Hardware detection, cable identification, and radio probing.

pub mod cables;
pub mod detect;
pub mod usb;
pub mod warnings;

pub use cables::{CableChip, KNOWN_CABLES, KnownCable, classify_cable, lookup_cable};
pub use detect::{
    DetectError, DetectedRadio, RadioIdent, RadioProber, VariantConfig, detect_radio_on_port,
    detect_radios,
};
pub use usb::{ScanError, UsbCable, scan_usb_cables};
pub use warnings::HardwareWarning;
