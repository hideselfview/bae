//! CD ripping module
//!
//! Provides functionality for ripping audio CDs using libcdio-paranoia
//! for accurate audio extraction with error correction.

pub mod cue_generator;
pub mod drive;
pub mod ffi;
pub mod log_generator;
pub mod paranoia;
pub mod ripper;

pub use cue_generator::CueGenerator;
#[allow(dead_code)] // Public API exports for external use
pub use drive::{CdDrive, CdDriveError, CdToc};
#[allow(dead_code)] // Public API exports for external use
pub use ffi::{detect_drives, LibcdioDrive, LibcdioError};
pub use log_generator::LogGenerator;
#[allow(dead_code)] // Public API exports for external use
pub use paranoia::{ParanoiaError, ParanoiaReader};
#[allow(dead_code)] // Public API exports for external use
pub use ripper::{CdRipper, RipProgress, RipResult};
