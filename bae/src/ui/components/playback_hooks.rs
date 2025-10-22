use crate::playback::PlaybackHandle;
use crate::AppContext;
use dioxus::prelude::*;

/// Hook to access the playback service
pub fn use_playback_service() -> PlaybackHandle {
    let context = use_context::<AppContext>();
    context.playback_handle.clone()
}
