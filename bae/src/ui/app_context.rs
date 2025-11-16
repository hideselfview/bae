use crate::cache;
use crate::cloud_storage;
use crate::config;
use crate::encryption;
use crate::import;
use crate::library::SharedLibraryManager;
use crate::playback;
use crate::torrent;

#[derive(Clone)]
pub struct AppContext {
    pub library_manager: SharedLibraryManager,
    pub config: config::Config,
    pub import_handle: import::ImportHandle,
    pub playback_handle: playback::PlaybackHandle,
    pub cache: cache::CacheManager,
    pub encryption_service: encryption::EncryptionService,
    pub cloud_storage: cloud_storage::CloudStorageManager,
    pub torrent_handle: torrent::TorrentManagerHandle,
}
