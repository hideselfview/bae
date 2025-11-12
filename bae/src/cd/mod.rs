//! CD ripping module
//!
//! Provides functionality for ripping audio CDs using libcdio-paranoia
//! for accurate audio extraction with error correction.

pub mod drive;
pub mod ripper;
pub mod cue_generator;
pub mod log_generator;

pub use drive::{CdDrive, CdDriveError, CdToc};
pub use ripper::{CdRipper, RipProgress, RipResult};
pub use cue_generator::CueGenerator;
pub use log_generator::LogGenerator;

