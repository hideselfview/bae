mod cpal_output;
pub mod progress;
pub mod reassembly; // Public for tests and internal use
pub mod service;
pub mod symphonia_decoder;

pub use progress::PlaybackProgress;
#[cfg(feature = "test-utils")]
#[allow(unused_imports)] // Used in tests
pub use reassembly::reassemble_track;
pub use service::{PlaybackHandle, PlaybackService, PlaybackState};
