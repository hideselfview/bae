use crate::config::use_config;
use crate::discogs::client::DiscogsSearchResult;
use crate::discogs::{DiscogsClient, DiscogsRelease};
use crate::import::{detect_metadata, FolderMetadata, MatchCandidate};
use crate::musicbrainz::{lookup_by_discid, search_releases, MbRelease};
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
    pub search_query: Signal<String>,
    pub search_results: Signal<Vec<DiscogsSearchResult>>,
    pub is_searching_masters: Signal<bool>,
    pub is_loading_versions: Signal<bool>,
    pub error_message: Signal<Option<String>>,
    pub navigation_stack: Signal<Vec<ImportStep>>,
    // MusicBrainz search state
    pub mb_search_results: Signal<Vec<MbRelease>>,
    pub is_searching_mb: Signal<bool>,
    pub mb_error_message: Signal<Option<String>>,
    // Folder detection import state (persists across navigation)
    pub folder_path: Signal<String>,
    pub detected_metadata: Signal<Option<FolderMetadata>>,
    pub import_phase: Signal<ImportPhase>,
    pub exact_match_candidates: Signal<Vec<MatchCandidate>>,
    pub selected_match_index: Signal<Option<usize>>,
    pub confirmed_candidate: Signal<Option<MatchCandidate>>,
    pub is_detecting: Signal<bool>,
    pub is_looking_up: Signal<bool>,
    pub import_error_message: Signal<Option<String>>,
    pub duplicate_album_id: Signal<Option<String>>,
    pub folder_files: Signal<Vec<FileInfo>>,
    client: DiscogsClient,
}

impl ImportContext {
    pub fn new(config: &crate::config::Config) -> Self {
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
            client: DiscogsClient::new(config.discogs_api_key.clone()),
        }
    }

    pub fn reset(&self) {
        let mut search_query = self.search_query;
        let mut search_results = self.search_results;
        let mut is_searching_masters = self.is_searching_masters;
        let mut is_loading_versions = self.is_loading_versions;
        let mut error_message = self.error_message;
        let mut navigation_stack = self.navigation_stack;
        let mut mb_search_results = self.mb_search_results;
        let mut is_searching_mb = self.is_searching_mb;
        let mut mb_error_message = self.mb_error_message;

        search_query.set(String::new());
        search_results.set(Vec::new());
        is_searching_masters.set(false);
        is_loading_versions.set(false);
        error_message.set(None);
        mb_search_results.set(Vec::new());
        is_searching_mb.set(false);
        mb_error_message.set(None);
        navigation_stack.set(vec![ImportStep::FolderIdentification]);

        // Also reset folder detection import state
        let mut folder_path = self.folder_path;
        let mut detected_metadata = self.detected_metadata;
        let mut import_phase = self.import_phase;
        let mut exact_match_candidates = self.exact_match_candidates;
        let mut selected_match_index = self.selected_match_index;
        let mut confirmed_candidate = self.confirmed_candidate;
        let mut is_detecting = self.is_detecting;
        let mut is_looking_up = self.is_looking_up;
        let mut import_error_message = self.import_error_message;
        let mut duplicate_album_id = self.duplicate_album_id;
        let mut folder_files = self.folder_files;

        folder_path.set(String::new());
        detected_metadata.set(None);
        import_phase.set(ImportPhase::FolderSelection);
        exact_match_candidates.set(Vec::new());
        selected_match_index.set(None);
        confirmed_candidate.set(None);
        is_detecting.set(false);
        is_looking_up.set(false);
        import_error_message.set(None);
        duplicate_album_id.set(None);
        folder_files.set(Vec::new());
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
            match self.client.search_by_discid(discid).await {
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
                .client
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
        let mut error_message = self.error_message;
        error_message.set(None);

        match self.client.get_release(&release_id).await {
            Ok(release) => {
                // The release from API already has master_id, but we use the one passed to us
                // (which might differ if we're importing via master vs specific release)
                let mut release = release;
                release.master_id = master_id;
                Ok(release)
            }
            Err(e) => {
                let error = format!("Failed to fetch release details: {}", e);
                error_message.set(Some(error.clone()));
                Err(error)
            }
        }
    }
}

/// Provider component to make search context available throughout the app
#[component]
pub fn AlbumImportContextProvider(children: Element) -> Element {
    let config = use_config();
    let album_import_ctx = ImportContext::new(&config);

    use_context_provider(move || Rc::new(album_import_ctx));

    rsx! {
        {children}
    }
}
