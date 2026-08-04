//! Integration smoke tests for the `akroasis_lib` public API.
//!
//! Unit tests live alongside each module. These exercise the library
//! boundary end-to-end (required by TESTING/no-tests, which only inspects
//! `lib.rs` and this directory — module-local `#[cfg(test)]` blocks don't
//! satisfy it).

use akroasis_lib::radio::errors::RadioError;
use akroasis_lib::radio::{Hardware, RadioVariant, StubHardware, resolve_target};

#[test]
fn radio_variant_display_names_are_stable() {
    assert_eq!(RadioVariant::Uv5r.display_name(), "Baofeng UV-5R");
    assert_eq!(RadioVariant::BfF8hp.display_name(), "Baofeng BF-F8HP");
}

#[test]
fn resolve_target_reports_hardware_unavailable_on_stub_backend() {
    let hw: &dyn Hardware = &StubHardware;
    let result = resolve_target(None, hw);
    assert!(matches!(result, Err(RadioError::HardwareNotAvailable)));
}
