use crate::playback::PlaybackHandle;
use crate::UIContext;
use dioxus::prelude::*;

/// Hook to access the playback service
pub fn use_playback_service() -> PlaybackHandle {
    let context = use_context::<UIContext>();
    context.playback_handle.clone()
}
