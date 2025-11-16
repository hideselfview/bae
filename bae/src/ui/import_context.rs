use crate::config::use_config;
use crate::discogs::client::DiscogsSearchResult;
use crate::discogs::{DiscogsClient, DiscogsRelease};
use crate::import::{
    detect_metadata, FolderMetadata, MatchCandidate, TorrentImportMetadata, TorrentSource,
};
use crate::musicbrainz::{lookup_by_discid, search_releases, MbRelease};
use crate::torrent::TorrentManagerHandle;
use crate::ui::components::import::FileInfo;
use dioxus::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub enum ImportStep {
    FolderIdentification,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportPhase {
    FolderSelection,
    MetadataDetection,
    ExactLookup,
    ManualSearch,
    Confirmation,
}

pub struct ImportContext {
    search_query: Signal<String>,
    search_results: Signal<Vec<DiscogsSearchResult>>,
    is_searching_masters: Signal<bool>,
    is_loading_versions: Signal<bool>,
    error_message: Signal<Option<String>>,
    navigation_stack: Signal<Vec<ImportStep>>,
    // MusicBrainz search state
    mb_search_results: Signal<Vec<MbRelease>>,
    is_searching_mb: Signal<bool>,
    mb_error_message: Signal<Option<String>>,
    // Folder detection import state (persists across navigation)
    folder_path: Signal<String>,
    detected_metadata: Signal<Option<FolderMetadata>>,
    import_phase: Signal<ImportPhase>,
    exact_match_candidates: Signal<Vec<MatchCandidate>>,
    selected_match_index: Signal<Option<usize>>,
    confirmed_candidate: Signal<Option<MatchCandidate>>,
    is_detecting: Signal<bool>,
    is_looking_up: Signal<bool>,
    import_error_message: Signal<Option<String>>,
    duplicate_album_id: Signal<Option<String>>,
    folder_files: Signal<Vec<FileInfo>>,
    // Torrent-specific state
    torrent_source: Signal<Option<TorrentSource>>,
    seed_after_download: Signal<bool>,
    torrent_metadata: Signal<Option<TorrentImportMetadata>>,
    discogs_client: DiscogsClient,
    /// Handle to torrent manager service for all torrent operations
    torrent_manager: TorrentManagerHandle,
}

impl ImportContext {
    pub fn new(config: &crate::config::Config, torrent_manager: TorrentManagerHandle) -> Self {
        use dioxus::prelude::*;
        Self {
            search_query: Signal::new(String::new()),
            search_results: Signal::new(Vec::new()),
            is_searching_masters: Signal::new(false),
            is_loading_versions: Signal::new(false),
            error_message: Signal::new(None),
            navigation_stack: Signal::new(vec![ImportStep::FolderIdentification]),
            mb_search_results: Signal::new(Vec::new()),
            is_searching_mb: Signal::new(false),
            mb_error_message: Signal::new(None),
            // Folder detection import state
            folder_path: Signal::new(String::new()),
            detected_metadata: Signal::new(None),
            import_phase: Signal::new(ImportPhase::FolderSelection),
            exact_match_candidates: Signal::new(Vec::new()),
            selected_match_index: Signal::new(None),
            confirmed_candidate: Signal::new(None),
            is_detecting: Signal::new(false),
            is_looking_up: Signal::new(false),
            import_error_message: Signal::new(None),
            duplicate_album_id: Signal::new(None),
            folder_files: Signal::new(Vec::new()),
            torrent_source: Signal::new(None),
            seed_after_download: Signal::new(true),
            torrent_metadata: Signal::new(None),
            discogs_client: DiscogsClient::new(config.discogs_api_key.clone()),
            torrent_manager,
        }
    }

    pub fn torrent_manager(&self) -> TorrentManagerHandle {
        self.torrent_manager.clone()
    }

    // Getters - return Signal (which can be used as ReadSignal)
    pub fn search_query(&self) -> Signal<String> {
        self.search_query
    }

    pub fn folder_path(&self) -> Signal<String> {
        self.folder_path
    }

    pub fn detected_metadata(&self) -> Signal<Option<FolderMetadata>> {
        self.detected_metadata
    }

    pub fn import_phase(&self) -> Signal<ImportPhase> {
        self.import_phase
    }

    pub fn exact_match_candidates(&self) -> Signal<Vec<MatchCandidate>> {
        self.exact_match_candidates
    }

    pub fn selected_match_index(&self) -> Signal<Option<usize>> {
        self.selected_match_index
    }

    pub fn confirmed_candidate(&self) -> Signal<Option<MatchCandidate>> {
        self.confirmed_candidate
    }

    pub fn is_detecting(&self) -> Signal<bool> {
        self.is_detecting
    }

    pub fn is_looking_up(&self) -> Signal<bool> {
        self.is_looking_up
    }

    pub fn import_error_message(&self) -> Signal<Option<String>> {
        self.import_error_message
    }

    pub fn duplicate_album_id(&self) -> Signal<Option<String>> {
        self.duplicate_album_id
    }

    pub fn folder_files(&self) -> Signal<Vec<FileInfo>> {
        self.folder_files
    }

    pub fn torrent_source(&self) -> Signal<Option<TorrentSource>> {
        self.torrent_source
    }

    pub fn seed_after_download(&self) -> Signal<bool> {
        self.seed_after_download
    }

    pub fn torrent_metadata(&self) -> Signal<Option<TorrentImportMetadata>> {
        self.torrent_metadata
    }

    pub fn set_search_query(&self, value: String) {
        let mut signal = self.search_query;
        signal.set(value);
    }

    pub fn set_search_results(&self, value: Vec<DiscogsSearchResult>) {
        let mut signal = self.search_results;
        signal.set(value);
    }

    pub fn set_is_searching_masters(&self, value: bool) {
        let mut signal = self.is_searching_masters;
        signal.set(value);
    }

    pub fn set_is_loading_versions(&self, value: bool) {
        let mut signal = self.is_loading_versions;
        signal.set(value);
    }

    pub fn set_error_message(&self, value: Option<String>) {
        let mut signal = self.error_message;
        signal.set(value);
    }

    pub fn set_navigation_stack(&self, value: Vec<ImportStep>) {
        let mut signal = self.navigation_stack;
        signal.set(value);
    }

    pub fn set_mb_search_results(&self, value: Vec<MbRelease>) {
        let mut signal = self.mb_search_results;
        signal.set(value);
    }

    pub fn set_is_searching_mb(&self, value: bool) {
        let mut signal = self.is_searching_mb;
        signal.set(value);
    }

    pub fn set_mb_error_message(&self, value: Option<String>) {
        let mut signal = self.mb_error_message;
        signal.set(value);
    }

    pub fn set_folder_path(&self, value: String) {
        let mut signal = self.folder_path;
        signal.set(value);
    }

    pub fn set_detected_metadata(&self, value: Option<FolderMetadata>) {
        let mut signal = self.detected_metadata;
        signal.set(value);
    }

    pub fn set_import_phase(&self, value: ImportPhase) {
        let mut signal = self.import_phase;
        signal.set(value);
    }

    pub fn set_exact_match_candidates(&self, value: Vec<MatchCandidate>) {
        let mut signal = self.exact_match_candidates;
        signal.set(value);
    }

    pub fn set_selected_match_index(&self, value: Option<usize>) {
        let mut signal = self.selected_match_index;
        signal.set(value);
    }

    pub fn set_confirmed_candidate(&self, value: Option<MatchCandidate>) {
        let mut signal = self.confirmed_candidate;
        signal.set(value);
    }

    pub fn set_is_detecting(&self, value: bool) {
        let mut signal = self.is_detecting;
        signal.set(value);
    }

    pub fn set_is_looking_up(&self, value: bool) {
        let mut signal = self.is_looking_up;
        signal.set(value);
    }

    pub fn set_import_error_message(&self, value: Option<String>) {
        let mut signal = self.import_error_message;
        signal.set(value);
    }

    pub fn set_duplicate_album_id(&self, value: Option<String>) {
        let mut signal = self.duplicate_album_id;
        signal.set(value);
    }

    pub fn set_folder_files(&self, value: Vec<FileInfo>) {
        let mut signal = self.folder_files;
        signal.set(value);
    }

    pub fn set_torrent_source(&self, value: Option<TorrentSource>) {
        let mut signal = self.torrent_source;
        signal.set(value);
    }

    pub fn set_seed_after_download(&self, value: bool) {
        let mut signal = self.seed_after_download;
        signal.set(value);
    }

    pub fn set_torrent_metadata(&self, value: Option<TorrentImportMetadata>) {
        let mut signal = self.torrent_metadata;
        signal.set(value);
    }

    /// Reset state for a new torrent selection
    pub fn select_torrent_file(
        &self,
        path: String,
        torrent_source: TorrentSource,
        seed_after_download: bool,
    ) {
        // Store torrent source and seed flag
        self.set_torrent_source(Some(torrent_source));
        self.set_seed_after_download(seed_after_download);

        // Reset state for new selection
        self.set_folder_path(path);
        self.set_detected_metadata(None);
        self.set_exact_match_candidates(Vec::new());
        self.set_selected_match_index(None);
        self.set_confirmed_candidate(None);
        self.set_import_error_message(None);
        self.set_duplicate_album_id(None);
        self.set_import_phase(ImportPhase::MetadataDetection);
        self.set_is_detecting(true);
    }

    pub fn reset(&self) {
        self.set_search_query(String::new());
        self.set_search_results(Vec::new());
        self.set_is_searching_masters(false);
        self.set_is_loading_versions(false);
        self.set_error_message(None);
        self.set_mb_search_results(Vec::new());
        self.set_is_searching_mb(false);
        self.set_mb_error_message(None);
        self.set_navigation_stack(vec![ImportStep::FolderIdentification]);

        // Also reset folder detection import state
        self.set_folder_path(String::new());
        self.set_detected_metadata(None);
        self.set_import_phase(ImportPhase::FolderSelection);
        self.set_exact_match_candidates(Vec::new());
        self.set_selected_match_index(None);
        self.set_confirmed_candidate(None);
        self.set_is_detecting(false);
        self.set_is_looking_up(false);
        self.set_import_error_message(None);
        self.set_duplicate_album_id(None);
        self.set_folder_files(Vec::new());
        self.set_torrent_source(None);
        self.set_seed_after_download(true);
        self.set_torrent_metadata(None);
    }

    pub async fn detect_folder_metadata(
        &self,
        folder_path: String,
    ) -> Result<FolderMetadata, String> {
        let path = PathBuf::from(&folder_path);
        detect_metadata(path).map_err(|e| format!("Failed to detect metadata: {}", e))
    }

    pub async fn search_discogs_by_metadata(
        &self,
        metadata: &FolderMetadata,
    ) -> Result<Vec<DiscogsSearchResult>, String> {
        use tracing::{info, warn};

        info!("🔍 Starting Discogs search with metadata:");
        info!(
            "   Artist: {:?}, Album: {:?}, Year: {:?}, DISCID: {:?}",
            metadata.artist, metadata.album, metadata.year, metadata.discid
        );

        // Try DISCID search first if available
        if let Some(ref discid) = metadata.discid {
            info!("🎯 Attempting DISCID search: {}", discid);
            match self.discogs_client.search_by_discid(discid).await {
                Ok(results) if !results.is_empty() => {
                    info!("✓ DISCID search returned {} result(s)", results.len());
                    return Ok(results);
                }
                Ok(_) => {
                    warn!("✗ DISCID search returned 0 results, falling back to text search");
                }
                Err(e) => {
                    warn!("✗ DISCID search failed: {}, falling back to text search", e);
                }
            }
        } else {
            info!("No DISCID available, using text search");
        }

        // Fall back to metadata search
        if let (Some(ref artist), Some(ref album)) = (&metadata.artist, &metadata.album) {
            info!(
                "🔎 Searching Discogs by text: artist='{}', album='{}', year={:?}",
                artist, album, metadata.year
            );

            match self
                .discogs_client
                .search_by_metadata(artist, album, metadata.year)
                .await
            {
                Ok(results) => {
                    info!("✓ Text search returned {} result(s)", results.len());
                    for (i, result) in results.iter().enumerate().take(5) {
                        info!(
                            "   {}. {} (master_id: {:?}, year: {:?})",
                            i + 1,
                            result.title,
                            result.master_id,
                            result.year
                        );
                    }
                    Ok(results)
                }
                Err(e) => {
                    warn!("✗ Text search failed: {}", e);
                    Err(format!("Discogs search failed: {}", e))
                }
            }
        } else {
            warn!("✗ Insufficient metadata for search (missing artist or album)");
            Err("Insufficient metadata for search".to_string())
        }
    }

    pub async fn search_musicbrainz_by_metadata(
        &self,
        metadata: &FolderMetadata,
    ) -> Result<Vec<MbRelease>, String> {
        use tracing::{info, warn};

        info!("🎵 Starting MusicBrainz search with metadata:");
        info!(
            "   Artist: {:?}, Album: {:?}, Year: {:?}, MB DiscID: {:?}",
            metadata.artist, metadata.album, metadata.year, metadata.mb_discid
        );

        // Try MB DiscID search first if available
        if let Some(ref mb_discid) = metadata.mb_discid {
            info!("🎯 Attempting MusicBrainz DiscID search: {}", mb_discid);
            match lookup_by_discid(mb_discid).await {
                Ok((releases, _external_urls)) => {
                    if !releases.is_empty() {
                        info!(
                            "✓ MusicBrainz DiscID search returned {} result(s)",
                            releases.len()
                        );
                        return Ok(releases);
                    } else {
                        warn!("✗ MusicBrainz DiscID search returned 0 results, falling back to text search");
                    }
                }
                Err(e) => {
                    warn!(
                        "✗ MusicBrainz DiscID search failed: {}, falling back to text search",
                        e
                    );
                }
            }
        } else {
            info!("No MusicBrainz DiscID available, using text search");
        }

        // Fall back to metadata search
        if let (Some(ref artist), Some(ref album)) = (&metadata.artist, &metadata.album) {
            info!(
                "🔎 Searching MusicBrainz by text: artist='{}', album='{}', year={:?}",
                artist, album, metadata.year
            );

            match search_releases(artist, album, metadata.year).await {
                Ok(releases) => {
                    info!(
                        "✓ MusicBrainz text search returned {} result(s)",
                        releases.len()
                    );
                    for (i, release) in releases.iter().enumerate().take(5) {
                        info!(
                            "   {}. {} - {} (release_id: {}, release_group_id: {})",
                            i + 1,
                            release.artist,
                            release.title,
                            release.release_id,
                            release.release_group_id
                        );
                    }
                    Ok(releases)
                }
                Err(e) => {
                    warn!("✗ MusicBrainz text search failed: {}", e);
                    Err(format!("MusicBrainz search failed: {}", e))
                }
            }
        } else {
            warn!("✗ Insufficient metadata for search (missing artist or album)");
            Err("Insufficient metadata for search".to_string())
        }
    }

    pub async fn import_release(
        &self,
        release_id: String,
        master_id: String,
    ) -> Result<DiscogsRelease, String> {
        self.set_error_message(None);

        match self.discogs_client.get_release(&release_id).await {
            Ok(release) => {
                // The release from API already has master_id, but we use the one passed to us
                // (which might differ if we're importing via master vs specific release)
                let mut release = release;
                release.master_id = master_id;
                Ok(release)
            }
            Err(e) => {
                let error = format!("Failed to fetch release details: {}", e);
                self.set_error_message(Some(error.clone()));
                Err(error)
            }
        }
    }
}

/// Provider component to make search context available throughout the app
#[component]
pub fn AlbumImportContextProvider(children: Element) -> Element {
    let config = use_config();
    let app_context = use_context::<crate::ui::AppContext>();
    let album_import_ctx = ImportContext::new(&config, app_context.torrent_manager.clone());

    use_context_provider(move || Rc::new(album_import_ctx));

    rsx! {
        {children}
    }
}
