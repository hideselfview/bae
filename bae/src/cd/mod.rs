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
pub use drive::CdDrive;
pub use log_generator::LogGenerator;
pub use ripper::{CdRipper, RipProgress};
