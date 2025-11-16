use crate::config::use_config;
use crate::discogs::client::DiscogsSearchResult;
use crate::discogs::{DiscogsClient, DiscogsRelease};
use crate::import::{
    detect_metadata, FolderMetadata, ImportRequest, ImportServiceHandle, MatchCandidate,
    MatchSource, TorrentImportMetadata, TorrentSource,
};
use crate::library::SharedLibraryManager;
use crate::musicbrainz::{lookup_by_discid, search_releases, MbRelease};
use crate::torrent::TorrentManagerHandle;
use crate::ui::components::import::{FileInfo, ImportSource, SearchSource};
use crate::ui::Route;
use dioxus::prelude::*;
use dioxus::router::Navigator;
use std::path::PathBuf;
use std::rc::Rc;
use tracing::{error, info};

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
    /// Handle to library manager for duplicate checking and import operations
    library_manager: SharedLibraryManager,
    /// Handle to import service for submitting import requests
    import_service: ImportServiceHandle,
}

impl ImportContext {
    pub fn new(
        config: &crate::config::Config,
        torrent_manager: TorrentManagerHandle,
        library_manager: SharedLibraryManager,
        import_service: ImportServiceHandle,
    ) -> Self {
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
            library_manager,
            import_service,
        }
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

    /// Reset detection state and return to folder selection phase
    fn reset_to_folder_selection(&self) {
        self.set_is_detecting(false);
        self.set_import_phase(ImportPhase::FolderSelection);
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

    /// Process detected metadata and trigger appropriate lookup/search flow
    pub async fn process_detected_metadata(
        &self,
        metadata: Option<FolderMetadata>,
        fallback_query: String,
    ) {
        use tracing::info;

        match metadata {
            Some(metadata) => {
                info!("Detected metadata: {:?}", metadata);
                self.set_detected_metadata(Some(metadata.clone()));

                // Try exact lookup if MB DiscID available
                if let Some(ref mb_discid) = metadata.mb_discid {
                    self.set_is_looking_up(true);
                    info!("🎵 Found MB DiscID: {}, performing exact lookup", mb_discid);

                    match lookup_by_discid(mb_discid).await {
                        Ok((releases, _external_urls)) => {
                            if releases.is_empty() {
                                info!("No exact matches found, proceeding to manual search");
                                self.init_search_query_from_metadata(&metadata);
                                self.set_import_phase(ImportPhase::ManualSearch);
                            } else if releases.len() == 1 {
                                // Single exact match - auto-proceed to confirmation
                                info!("✅ Single exact match found, auto-proceeding");
                                let mb_release = releases[0].clone();
                                let candidate = MatchCandidate {
                                    source: MatchSource::MusicBrainz(mb_release),
                                    confidence: 100.0,
                                    match_reasons: vec!["Exact DiscID match".to_string()],
                                };
                                self.set_confirmed_candidate(Some(candidate));
                                self.set_import_phase(ImportPhase::Confirmation);
                            } else {
                                // Multiple exact matches - show for selection
                                info!(
                                    "Found {} exact matches, showing for selection",
                                    releases.len()
                                );
                                let candidates: Vec<MatchCandidate> = releases
                                    .into_iter()
                                    .map(|mb_release| MatchCandidate {
                                        source: MatchSource::MusicBrainz(mb_release),
                                        confidence: 100.0,
                                        match_reasons: vec!["Exact DiscID match".to_string()],
                                    })
                                    .collect();
                                self.set_exact_match_candidates(candidates);
                                self.set_import_phase(ImportPhase::ExactLookup);
                            }
                            self.set_is_looking_up(false);
                        }
                        Err(e) => {
                            info!(
                                "MB DiscID lookup failed: {}, proceeding to manual search",
                                e
                            );
                            self.set_is_looking_up(false);
                            self.init_search_query_from_metadata(&metadata);
                            self.set_import_phase(ImportPhase::ManualSearch);
                        }
                    }
                } else {
                    // No MB DiscID, proceed to manual search with detected metadata
                    info!("No MB DiscID found, proceeding to manual search");
                    self.init_search_query_from_metadata(&metadata);
                    self.set_import_phase(ImportPhase::ManualSearch);
                }
            }
            None => {
                // No metadata detected, proceed with fallback query
                info!(
                    "No metadata detected, using fallback query: {}",
                    fallback_query
                );
                self.set_search_query(fallback_query);
                self.set_import_phase(ImportPhase::ManualSearch);
            }
        }
    }

    /// Initialize search query from metadata
    fn init_search_query_from_metadata(&self, metadata: &FolderMetadata) {
        let mut query_parts = Vec::new();
        if let Some(ref artist) = metadata.artist {
            query_parts.push(artist.clone());
        }
        if let Some(ref album) = metadata.album {
            query_parts.push(album.clone());
        }
        self.set_search_query(query_parts.join(" "));
    }

    /// Load torrent for import: prepare torrent, extract info, process metadata
    pub async fn load_torrent_for_import(
        &self,
        path: PathBuf,
        seed_flag: bool,
    ) -> Result<(), String> {
        use tracing::info;

        // Reset state for new torrent selection
        self.select_torrent_file(
            path.to_string_lossy().to_string(),
            TorrentSource::File(path.clone()),
            seed_flag,
        );

        // Prepare torrent via TorrentManager
        let torrent_info = match self
            .torrent_manager
            .prepare_import_torrent(TorrentSource::File(path))
            .await
        {
            Ok(info) => info,
            Err(e) => {
                let error_msg = format!("Failed to prepare torrent: {}", e);
                self.set_import_error_message(Some(error_msg.clone()));
                self.reset_to_folder_selection();
                return Err(error_msg);
            }
        };

        // Convert file list to UI FileInfo format
        let mut files: Vec<FileInfo> = torrent_info
            .file_list
            .iter()
            .map(|tf| {
                let name = tf
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                let format = tf
                    .path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_uppercase();
                FileInfo {
                    name,
                    size: tf.size as u64,
                    format,
                }
            })
            .collect();

        files.sort_by(|a, b| a.name.cmp(&b.name));
        self.set_folder_files(files);

        // Store torrent metadata
        let torrent_metadata = TorrentImportMetadata {
            info_hash: torrent_info.info_hash,
            magnet_link: None,
            torrent_name: torrent_info.torrent_name.clone(),
            total_size_bytes: torrent_info.total_size_bytes as i64,
            piece_length: torrent_info.piece_length as i32,
            num_pieces: torrent_info.num_pieces as i32,
            seed_after_download: seed_flag,
            file_list: torrent_info.file_list,
        };
        self.set_torrent_metadata(Some(torrent_metadata));

        info!(
            "Torrent loaded: {} ({} files)",
            torrent_info.torrent_name,
            self.folder_files().read().len()
        );

        // Process detected metadata
        self.process_detected_metadata(torrent_info.detected_metadata, torrent_info.torrent_name)
            .await;

        // Mark detection as complete
        self.set_is_detecting(false);

        Ok(())
    }

    /// Retry metadata detection for the current torrent.
    ///
    /// Uses the current folder_path and seed_flag from context to reload
    /// the torrent and detect metadata. Useful for retrying detection when
    /// CUE/log files are detected in manual search phase.
    pub async fn retry_torrent_metadata_detection(&self) -> Result<(), String> {
        let path = self.folder_path().read().clone();
        let seed_flag = *self.seed_after_download().read();
        let path_buf = PathBuf::from(&path);
        self.load_torrent_for_import(path_buf, seed_flag).await
    }

    /// Confirm a match candidate and start the import workflow.
    ///
    /// This handles the entire confirmation-to-import flow:
    /// - Checks for duplicate albums (Discogs or MusicBrainz)
    /// - Fetches full release metadata if needed (for Discogs torrents)
    /// - Builds appropriate ImportRequest based on source (Folder/Torrent/CD)
    /// - Submits import request to ImportService
    /// - Navigates to album detail page on success
    /// - Sets error messages on failure
    pub async fn confirm_and_start_import(
        &self,
        candidate: MatchCandidate,
        import_source: ImportSource,
        navigator: Navigator,
    ) -> Result<(), String> {
        // Check for duplicates before importing
        match &candidate.source {
            MatchSource::Discogs(discogs_result) => {
                let master_id = discogs_result.master_id.map(|id| id.to_string());
                let release_id = Some(discogs_result.id.to_string());

                if let Ok(Some(duplicate)) = self
                    .library_manager
                    .get()
                    .find_duplicate_by_discogs(master_id.as_deref(), release_id.as_deref())
                    .await
                {
                    self.set_duplicate_album_id(Some(duplicate.id));
                    self.set_import_error_message(Some(format!(
                        "This release already exists in your library: {}",
                        duplicate.title
                    )));
                    return Err("Duplicate album found".to_string());
                }
            }
            MatchSource::MusicBrainz(mb_release) => {
                let release_id = Some(mb_release.release_id.clone());
                let release_group_id = Some(mb_release.release_group_id.clone());

                if let Ok(Some(duplicate)) = self
                    .library_manager
                    .get()
                    .find_duplicate_by_musicbrainz(
                        release_id.as_deref(),
                        release_group_id.as_deref(),
                    )
                    .await
                {
                    self.set_duplicate_album_id(Some(duplicate.id));
                    self.set_import_error_message(Some(format!(
                        "This release already exists in your library: {}",
                        duplicate.title
                    )));
                    return Err("Duplicate album found".to_string());
                }
            }
        }

        // Extract master_year from metadata or release date
        let metadata = self.detected_metadata().read().clone();
        let master_year = metadata.as_ref().and_then(|m| m.year).unwrap_or(1970);

        // Build import request based on source
        let request = match import_source {
            ImportSource::Folder => {
                let folder_path = self.folder_path().read().clone();
                match candidate.source.clone() {
                    MatchSource::Discogs(discogs_result) => {
                        let master_id = match discogs_result.master_id {
                            Some(id) => id.to_string(),
                            None => {
                                return Err("Discogs result has no master_id".to_string());
                            }
                        };
                        let release_id = discogs_result.id.to_string();

                        let discogs_release = self.import_release(release_id, master_id).await?;

                        ImportRequest::Folder {
                            discogs_release: Some(discogs_release),
                            mb_release: None,
                            folder: PathBuf::from(folder_path),
                            master_year,
                        }
                    }
                    MatchSource::MusicBrainz(mb_release) => {
                        info!(
                            "Starting import for MusicBrainz release: {}",
                            mb_release.title
                        );

                        ImportRequest::Folder {
                            discogs_release: None,
                            mb_release: Some(mb_release.clone()),
                            folder: PathBuf::from(folder_path),
                            master_year,
                        }
                    }
                }
            }
            ImportSource::Torrent => {
                let torrent_source = self
                    .torrent_source()
                    .read()
                    .clone()
                    .ok_or_else(|| "No torrent source available".to_string())?;
                let seed_after_download = *self.seed_after_download().read();
                let torrent_metadata = self
                    .torrent_metadata()
                    .read()
                    .clone()
                    .ok_or_else(|| "No torrent metadata available".to_string())?;

                match candidate.source.clone() {
                    MatchSource::Discogs(discogs_result) => {
                        let master_id = match discogs_result.master_id {
                            Some(id) => id.to_string(),
                            None => {
                                return Err("Discogs result has no master_id".to_string());
                            }
                        };
                        let release_id = discogs_result.id.to_string();

                        let discogs_release = self.import_release(release_id, master_id).await?;

                        ImportRequest::Torrent {
                            torrent_source,
                            discogs_release: Some(discogs_release),
                            mb_release: None,
                            master_year,
                            seed_after_download,
                            torrent_metadata,
                        }
                    }
                    MatchSource::MusicBrainz(mb_release) => {
                        info!(
                            "Starting torrent import for MusicBrainz release: {}",
                            mb_release.title
                        );

                        ImportRequest::Torrent {
                            torrent_source,
                            discogs_release: None,
                            mb_release: Some(mb_release.clone()),
                            master_year,
                            seed_after_download,
                            torrent_metadata,
                        }
                    }
                }
            }
            ImportSource::Cd => {
                let folder_path = self.folder_path().read().clone();
                match candidate.source.clone() {
                    MatchSource::Discogs(_discogs_result) => {
                        return Err("CD imports require MusicBrainz metadata".to_string());
                    }
                    MatchSource::MusicBrainz(mb_release) => {
                        info!(
                            "Starting CD import for MusicBrainz release: {}",
                            mb_release.title
                        );

                        ImportRequest::CD {
                            discogs_release: None,
                            mb_release: Some(mb_release.clone()),
                            drive_path: PathBuf::from(folder_path),
                            master_year,
                        }
                    }
                }
            }
        };

        // Submit import request
        match self.import_service.send_request(request).await {
            Ok((album_id, _release_id)) => {
                info!("Import started, navigating to album: {}", album_id);
                // Reset import state before navigating
                self.reset();
                navigator.push(Route::AlbumDetail {
                    album_id,
                    release_id: String::new(),
                });
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("Failed to start import: {}", e);
                error!("{}", error_msg);
                self.set_import_error_message(Some(error_msg.clone()));
                Err(error_msg)
            }
        }
    }

    /// Load a folder for import: read files, detect metadata, and start lookup flow.
    ///
    /// This handles:
    /// - Reading and listing all files in the folder
    /// - Detecting metadata from CUE/FLAC files
    /// - Triggering MusicBrainz lookup if DiscID found
    /// - Setting up manual search if no exact match
    pub async fn load_folder_for_import(&self, path: String) -> Result<(), String> {
        // Reset state for new folder selection
        self.set_folder_path(path.clone());
        self.set_detected_metadata(None);
        self.set_exact_match_candidates(Vec::new());
        self.set_selected_match_index(None);
        self.set_confirmed_candidate(None);
        self.set_import_error_message(None);
        self.set_duplicate_album_id(None);
        self.set_import_phase(ImportPhase::MetadataDetection);
        self.set_is_detecting(true);

        // Read files from folder
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&path) {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if entry_path.is_file() {
                    let name = entry_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    let format = entry_path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_uppercase();
                    files.push(FileInfo { name, size, format });
                }
            }
            files.sort_by(|a, b| a.name.cmp(&b.name));
        }
        self.set_folder_files(files);

        // Detect metadata
        match self.detect_folder_metadata(path.clone()).await {
            Ok(metadata) => {
                self.set_is_detecting(false);
                self.process_detected_metadata(Some(metadata), path).await;
                Ok(())
            }
            Err(e) => {
                self.set_import_error_message(Some(e.clone()));
                self.set_is_detecting(false);
                self.set_import_phase(ImportPhase::FolderSelection);
                Err(e)
            }
        }
    }

    /// Search for matches using the current search query and source.
    ///
    /// This consolidates search logic:
    /// - Sets the search query in context
    /// - Calls appropriate search API (MusicBrainz or Discogs)
    /// - Converts results to MatchCandidate format
    /// - Returns ranked candidates
    pub async fn search_for_matches(
        &self,
        query: String,
        source: SearchSource,
    ) -> Result<Vec<MatchCandidate>, String> {
        self.set_search_query(query.clone());

        let metadata = self.detected_metadata().read().clone();

        match source {
            SearchSource::MusicBrainz => {
                if let Some(ref meta) = metadata {
                    let results = self.search_musicbrainz_by_metadata(meta).await?;
                    use crate::import::rank_mb_matches;
                    Ok(rank_mb_matches(meta, results))
                } else {
                    Err("No metadata available for search".to_string())
                }
            }
            SearchSource::Discogs => {
                if let Some(ref meta) = metadata {
                    let results = self.search_discogs_by_metadata(meta).await?;
                    use crate::import::rank_discogs_matches;
                    Ok(rank_discogs_matches(meta, results))
                } else {
                    Err("No metadata available for search".to_string())
                }
            }
        }
    }

    /// Load a CD for import: detect TOC, lookup by DiscID, and start import flow.
    ///
    /// This handles:
    /// - Setting folder path to drive path
    /// - Resetting import state
    /// - Looking up release by MusicBrainz DiscID
    /// - Processing results (exact match, multiple matches, or manual search)
    /// - Setting appropriate import phase
    pub async fn load_cd_for_import(
        &self,
        drive_path: String,
        disc_id: String,
    ) -> Result<(), String> {
        // Reset state for new CD selection
        self.set_folder_path(drive_path.clone());
        self.set_detected_metadata(None);
        self.set_exact_match_candidates(Vec::new());
        self.set_selected_match_index(None);
        self.set_confirmed_candidate(None);
        self.set_import_error_message(None);
        self.set_duplicate_album_id(None);
        self.set_import_phase(ImportPhase::MetadataDetection);
        self.set_is_looking_up(true);

        // Lookup by disc_id via MusicBrainz
        match lookup_by_discid(&disc_id).await {
            Ok((releases, _external_urls)) => {
                self.set_is_looking_up(false);

                if releases.is_empty() {
                    // No matches - proceed to manual search
                    self.set_search_query(drive_path.clone());
                    self.set_import_phase(ImportPhase::ManualSearch);
                } else if releases.len() == 1 {
                    // Single exact match - auto-proceed to confirmation
                    let mb_release = releases[0].clone();
                    let candidate = MatchCandidate {
                        source: MatchSource::MusicBrainz(mb_release),
                        confidence: 100.0,
                        match_reasons: vec!["Exact DiscID match".to_string()],
                    };
                    self.set_confirmed_candidate(Some(candidate));
                    self.set_import_phase(ImportPhase::Confirmation);
                } else {
                    // Multiple exact matches - show for selection
                    let candidates: Vec<MatchCandidate> = releases
                        .into_iter()
                        .map(|mb_release| MatchCandidate {
                            source: MatchSource::MusicBrainz(mb_release),
                            confidence: 100.0,
                            match_reasons: vec!["Exact DiscID match".to_string()],
                        })
                        .collect();
                    self.set_exact_match_candidates(candidates);
                    self.set_import_phase(ImportPhase::ExactLookup);
                }
                Ok(())
            }
            Err(e) => {
                self.set_is_looking_up(false);
                self.set_search_query(drive_path.clone());
                self.set_import_phase(ImportPhase::ManualSearch);
                Err(format!("Failed to lookup by DiscID: {}", e))
            }
        }
    }

    /// Select an exact match candidate by index and move to confirmation.
    ///
    /// This transitions from ExactLookup phase to Confirmation phase.
    pub fn select_exact_match(&self, index: usize) {
        self.set_selected_match_index(Some(index));
        if let Some(candidate) = self.exact_match_candidates().read().get(index) {
            self.set_confirmed_candidate(Some(candidate.clone()));
            self.set_import_phase(ImportPhase::Confirmation);
        }
    }

    /// Confirm a match candidate and move to confirmation phase.
    ///
    /// This is used when confirming from manual search results.
    pub fn confirm_candidate(&self, candidate: MatchCandidate) {
        self.set_confirmed_candidate(Some(candidate));
        self.set_import_phase(ImportPhase::Confirmation);
    }

    /// Reject the current confirmation and go back to previous phase.
    ///
    /// This handles:
    /// - Clearing confirmed candidate and selection
    /// - Determining whether to go back to ExactLookup or ManualSearch
    /// - Initializing search query from detected metadata if going to ManualSearch
    pub fn reject_confirmation(&self) {
        self.set_confirmed_candidate(None);
        self.set_selected_match_index(None);

        if !self.exact_match_candidates().read().is_empty() {
            self.set_import_phase(ImportPhase::ExactLookup);
        } else {
            // Initialize search query from detected metadata when transitioning to manual search
            if let Some(metadata) = self.detected_metadata().read().as_ref() {
                let mut query_parts = Vec::new();
                if let Some(ref artist) = metadata.artist {
                    query_parts.push(artist.clone());
                }
                if let Some(ref album) = metadata.album {
                    query_parts.push(album.clone());
                }
                if !query_parts.is_empty() {
                    self.set_search_query(query_parts.join(" "));
                }
            }
            self.set_import_phase(ImportPhase::ManualSearch);
        }
    }

    /// Skip metadata detection and proceed to manual search.
    ///
    /// This handles:
    /// - Stopping the detection process
    /// - Initializing search query from torrent/folder name if empty
    /// - Transitioning to ManualSearch phase
    pub fn skip_metadata_detection(&self) {
        self.set_is_detecting(false);

        // Use current search query (already set to torrent name) or folder path
        if self.search_query().read().is_empty() {
            let path = self.folder_path().read().clone();
            if let Some(name) = std::path::Path::new(&path).file_name() {
                self.set_search_query(name.to_string_lossy().to_string());
            }
        }

        self.set_import_phase(ImportPhase::ManualSearch);
    }
}

/// Provider component to make search context available throughout the app
#[component]
pub fn AlbumImportContextProvider(children: Element) -> Element {
    let config = use_config();
    let app_context = use_context::<crate::ui::AppContext>();
    let album_import_ctx = ImportContext::new(
        &config,
        app_context.torrent_manager.clone(),
        app_context.library_manager.clone(),
        app_context.import_handle.clone(),
    );

    use_context_provider(move || Rc::new(album_import_ctx));

    rsx! {
        {children}
    }
}
