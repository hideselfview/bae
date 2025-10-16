use crate::playback::PlaybackHandle;
use crate::AppContext;
use dioxus::prelude::*;

/// Hook to access the playback service
pub fn use_playback_service() -> PlaybackHandle {
    let app_context = use_context::<AppContext>();
    app_context.playback_handle.clone()
}
