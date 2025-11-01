pub mod handle;

use crate::playback::service::PlaybackState;
pub use handle::PlaybackProgressHandle;
use std::time::Duration;

/// Progress updates during playback
#[derive(Debug, Clone)]
pub enum PlaybackProgress {
    StateChanged {
        state: PlaybackState,
    },
    PositionUpdate {
        position: Duration,
        track_id: String,
    },
    TrackCompleted {
        track_id: String,
    },
    /// Seek completed successfully - position changed within the same track
    /// UI should update position and clear is_seeking flag
    Seeked {
        position: Duration,
        track_id: String,
        was_paused: bool,
    },
    SeekError {
        requested_position: Duration,
        track_duration: Duration,
    },
    /// Seek was skipped because position difference was too small (< 100ms)
    /// UI should clear is_seeking flag
    SeekSkipped {
        requested_position: Duration,
        current_position: Duration,
    },
}
