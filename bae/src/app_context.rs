use crate::config;
use crate::import;
use crate::library_context::SharedLibraryManager;
use crate::playback;

#[derive(Clone)]
pub struct AppContext {
    pub library_manager: SharedLibraryManager,
    pub config: config::Config,
    pub import_service_handle: import::ImportHandle,
    pub playback_handle: playback::PlaybackHandle,
}
