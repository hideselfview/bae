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
    SeekError {
        requested_position: Duration,
        track_duration: Duration,
    },
}
