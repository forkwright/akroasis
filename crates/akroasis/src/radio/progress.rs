//! Progress bar wrappers for EEPROM transfer operations.

use indicatif::{ProgressBar, ProgressStyle};

/// Creates a progress bar for EEPROM download (radio → host).
pub(crate) fn download_bar(total_blocks: u64) -> ProgressBar {
    let pb = ProgressBar::new(total_blocks);
    let style = ProgressStyle::with_template(
        "{spinner:.green} Reading EEPROM [{bar:40.cyan/blue}] {pos}/{len} blocks",
    )
    .unwrap_or_else(|_| ProgressStyle::default_bar());
    pb.set_style(style);
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb
}

/// Creates a progress bar for EEPROM upload (host → radio).
pub(crate) fn upload_bar(total_blocks: u64) -> ProgressBar {
    let pb = ProgressBar::new(total_blocks);
    let style = ProgressStyle::with_template(
        "{spinner:.green} Writing EEPROM [{bar:40.cyan/blue}] {pos}/{len} blocks",
    )
    .unwrap_or_else(|_| ProgressStyle::default_bar());
    pb.set_style(style);
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb
}
