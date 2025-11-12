//! CD ripping module
//!
//! Provides functionality for ripping audio CDs using libcdio-paranoia
//! for accurate audio extraction with error correction.

pub mod cue_generator;
pub mod drive;
pub mod ffi;
pub mod log_generator;
pub mod ripper;

pub use cue_generator::CueGenerator;
pub use drive::{CdDrive, CdDriveError, CdToc};
pub use ffi::{detect_drives, LibcdioDrive, LibcdioError};
pub use log_generator::LogGenerator;
pub use ripper::{CdRipper, RipProgress, RipResult};
