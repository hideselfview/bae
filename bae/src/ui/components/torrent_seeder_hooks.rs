use crate::torrent::TorrentManagerHandle;
use crate::AppContext;
use dioxus::prelude::*;

/// Hook to access the torrent manager service
pub fn use_torrent_manager() -> TorrentManagerHandle {
    let context = use_context::<AppContext>();
    context.torrent_handle.clone()
}
