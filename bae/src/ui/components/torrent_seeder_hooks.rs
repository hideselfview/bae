use crate::torrent::TorrentSeederHandle;
use crate::AppContext;
use dioxus::prelude::*;

/// Hook to access the torrent seeder service
pub fn use_torrent_seeder() -> TorrentSeederHandle {
    let context = use_context::<AppContext>();
    context.torrent_seeder.clone()
}
