use super::state::ImportContext;
use super::types::ImportPhase;
use crate::import::{
    cover_art, detect_folder_contents, FolderMetadata, MatchCandidate, MatchSource, TorrentSource,
};
use crate::musicbrainz::{lookup_by_discid, ExternalUrls, MbRelease};
use crate::torrent::parse_torrent_info;
use crate::ui::components::import::{CategorizedFileInfo, FileInfo};
use dioxus::prelude::*;
use std::path::PathBuf;
use tracing::{info, warn};

pub enum DiscIdLookupResult {
    NoMatches,
    SingleMatch(Box<MatchCandidate>),
    MultipleMatches(Vec<MatchCandidate>),
}

pub struct FolderDetectionResult {
    pub metadata: crate::import::FolderMetadata,
    pub files: CategorizedFileInfo,
    pub discid_result: Option<DiscIdLookupResult>,
}

/// Load torrent for import: parse torrent file and extract info
pub async fn load_torrent_for_import(
    ctx: &ImportContext,
    path: PathBuf,
    seed_flag: bool,
) -> Result<(), String> {
    info!(
        "Loading torrent for import: {:?}, seed_flag: {}",
        path, seed_flag
    );

    // Reset state for new torrent selection
    ctx.select_torrent_file(
        path.to_string_lossy().to_string(),
        TorrentSource::File(path.clone()),
        seed_flag,
    );
    info!("Torrent file selected");

    // Parse torrent file to extract info
    info!("Parsing torrent file...");
    let torrent_info = match parse_torrent_info(&path) {
        Ok(info) => {
            info!("Torrent parsing successful");
            info
        }
        Err(e) => {
            let error_msg = format!("Failed to parse torrent file: {}", e);
            warn!("{}", error_msg);
            ctx.set_import_error_message(Some(error_msg.clone()));
            ctx.reset_to_folder_selection();
            return Err(error_msg);
        }
    };

    // Convert file list to categorized UI format (before storing torrent_info)
    let categorized = categorize_torrent_files(&torrent_info.files);
    ctx.set_folder_files(categorized);

    info!(
        "Torrent loaded: {} ({} files)",
        torrent_info.name,
        torrent_info.files.len()
    );

    // Store torrent info (move ownership into signal)
    ctx.set_torrent_info(Some(torrent_info));

    Ok(())
}

/// Retry metadata detection for the current torrent.
pub async fn retry_torrent_metadata_detection(ctx: &ImportContext) -> Result<(), String> {
    let path = ctx.folder_path().read().clone();
    let seed_flag = *ctx.seed_after_download().read();
    let path_buf = PathBuf::from(&path);
    load_torrent_for_import(ctx, path_buf, seed_flag).await
}

/// Load a folder for import: read files, detect metadata, and optionally lookup by DiscID
pub async fn load_folder_for_import(
    ctx: &ImportContext,
    path: String,
) -> Result<FolderDetectionResult, String> {
    use crate::import::folder_scanner;

    // First, detect if this folder contains multiple releases
    let releases = folder_scanner::scan_for_releases(PathBuf::from(path.clone()))?;

    info!("Detected {} release(s) in folder", releases.len());

    // Store all detected releases
    ctx.set_detected_releases(releases.clone());

    // If multiple releases, show selection UI
    if releases.len() > 1 {
        info!("Multiple releases detected, showing release selector");
        ctx.set_import_phase(ImportPhase::ReleaseSelection);
        // Return early - user needs to select releases
        return Ok(FolderDetectionResult {
            metadata: FolderMetadata {
                artist: None,
                album: None,
                year: None,
                discid: None,
                mb_discid: None,
                track_count: None,
                confidence: 0.0,
            },
            files: CategorizedFileInfo::default(),
            discid_result: None,
        });
    }

    // Single release - auto-select and proceed
    let release = if releases.len() == 1 {
        ctx.set_selected_release_indices(vec![0]);
        ctx.set_current_release_index(0);
        &releases[0]
    } else {
        // No releases detected - shouldn't happen, but handle gracefully
        return Ok(FolderDetectionResult {
            metadata: FolderMetadata {
                artist: None,
                album: None,
                year: None,
                discid: None,
                mb_discid: None,
                track_count: None,
                confidence: 0.0,
            },
            files: CategorizedFileInfo::default(),
            discid_result: None,
        });
    };

    let folder_contents = detect_folder_contents(release.path.clone())
        .map_err(|e| format!("Failed to detect folder contents: {}", e))?;

    let metadata = folder_contents.metadata;

    // Get categorized files from the release (already populated by folder_scanner)
    let files = CategorizedFileInfo::from_scanned(&release.files);

    info!(
        "Detected metadata: artist={:?}, album={:?}, year={:?}, mb_discid={:?}",
        metadata.artist, metadata.album, metadata.year, metadata.mb_discid
    );

    // Set files and metadata immediately so UI can show them
    ctx.set_folder_files(files.clone());
    ctx.set_detected_metadata(Some(metadata.clone()));

    let discid_result = if let Some(ref mb_discid) = metadata.mb_discid {
        ctx.set_is_looking_up(true);
        info!("🎵 Found MB DiscID: {}, performing exact lookup", mb_discid);

        let result = match lookup_by_discid(mb_discid).await {
            Ok((releases, external_urls)) => {
                handle_discid_lookup_result(ctx, releases, external_urls).await
            }
            Err(e) => {
                info!("MB DiscID lookup failed: {}", e);
                DiscIdLookupResult::NoMatches
            }
        };

        ctx.set_is_looking_up(false);
        Some(result)
    } else {
        info!("No MB DiscID found");
        None
    };

    Ok(FolderDetectionResult {
        metadata,
        files,
        discid_result,
    })
}

/// Load a specific release by index from the detected releases
pub async fn load_selected_release(
    ctx: &ImportContext,
    release_index: usize,
) -> Result<FolderDetectionResult, String> {
    let releases = ctx.detected_releases.read();
    let release = releases
        .get(release_index)
        .ok_or_else(|| format!("Invalid release index: {}", release_index))?;

    info!("Loading release: {} ({:?})", release.name, release.path);

    let folder_contents = detect_folder_contents(release.path.clone())
        .map_err(|e| format!("Failed to detect folder contents: {}", e))?;

    let metadata = folder_contents.metadata;

    // Get categorized files from the release (already populated by folder_scanner)
    let files = CategorizedFileInfo::from_scanned(&release.files);

    info!(
        "Detected metadata: artist={:?}, album={:?}, year={:?}, mb_discid={:?}",
        metadata.artist, metadata.album, metadata.year, metadata.mb_discid
    );

    // Set files and metadata immediately so UI can show them
    ctx.set_folder_files(files.clone());
    ctx.set_detected_metadata(Some(metadata.clone()));

    let discid_result = if let Some(ref mb_discid) = metadata.mb_discid {
        ctx.set_is_looking_up(true);
        info!("🎵 Found MB DiscID: {}, performing exact lookup", mb_discid);

        let result = match lookup_by_discid(mb_discid).await {
            Ok((releases, external_urls)) => {
                handle_discid_lookup_result(ctx, releases, external_urls).await
            }
            Err(e) => {
                info!("MB DiscID lookup failed: {}", e);
                DiscIdLookupResult::NoMatches
            }
        };

        ctx.set_is_looking_up(false);
        Some(result)
    } else {
        info!("No MB DiscID found");
        None
    };

    Ok(FolderDetectionResult {
        metadata,
        files,
        discid_result,
    })
}

/// Load a CD for import: lookup by DiscID
pub async fn load_cd_for_import(
    ctx: &ImportContext,
    disc_id: String,
) -> Result<DiscIdLookupResult, String> {
    ctx.set_is_looking_up(true);

    let result = match lookup_by_discid(&disc_id).await {
        Ok((releases, external_urls)) => {
            handle_discid_lookup_result(ctx, releases, external_urls).await
        }
        Err(e) => {
            info!("MB DiscID lookup failed: {}", e);
            DiscIdLookupResult::NoMatches
        }
    };

    ctx.set_is_looking_up(false);

    Ok(result)
}

/// Handle DiscID lookup result: process 0/1/multiple matches and return result
async fn handle_discid_lookup_result(
    ctx: &ImportContext,
    releases: Vec<MbRelease>,
    external_urls: ExternalUrls,
) -> DiscIdLookupResult {
    if releases.is_empty() {
        info!("No exact matches found");
        return DiscIdLookupResult::NoMatches;
    }

    info!("Found {} exact matches", releases.len());

    let cover_art_futures: Vec<_> = releases
        .iter()
        .map(|mb_release| {
            cover_art::fetch_cover_art_for_mb_release(
                mb_release,
                &external_urls,
                Some(&ctx.discogs_client),
            )
        })
        .collect();
    let cover_art_urls: Vec<_> = futures::future::join_all(cover_art_futures).await;

    let candidates: Vec<MatchCandidate> = releases
        .into_iter()
        .zip(cover_art_urls.into_iter())
        .map(|(mb_release, cover_art_url)| MatchCandidate {
            source: MatchSource::MusicBrainz(mb_release),
            confidence: 100.0,
            match_reasons: vec!["Exact DiscID match".to_string()],
            cover_art_url,
        })
        .collect();

    if candidates.len() == 1 {
        DiscIdLookupResult::SingleMatch(Box::new(candidates[0].clone()))
    } else {
        DiscIdLookupResult::MultipleMatches(candidates)
    }
}

/// Categorize torrent files into tracks, artwork, documents, and other
fn categorize_torrent_files(files: &[crate::torrent::ffi::TorrentFileInfo]) -> CategorizedFileInfo {
    use crate::ui::components::import::AudioContentInfo;

    let audio_extensions = ["flac", "mp3", "wav", "m4a", "aac", "ogg"];
    let image_extensions = ["jpg", "jpeg", "png", "webp", "gif", "bmp"];
    let document_extensions = ["cue", "log", "txt", "nfo", "m3u", "m3u8"];

    let mut tracks = Vec::new();
    let mut artwork = Vec::new();
    let mut documents = Vec::new();
    let mut other = Vec::new();

    for tf in files {
        let path_buf = PathBuf::from(&tf.path);
        let name = tf.path.clone();
        let format = path_buf
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_uppercase();
        let ext_lower = format.to_lowercase();

        let file_info = FileInfo {
            name,
            size: tf.size as u64,
            format,
        };

        if audio_extensions.contains(&ext_lower.as_str()) {
            tracks.push(file_info);
        } else if image_extensions.contains(&ext_lower.as_str()) {
            artwork.push(file_info);
        } else if document_extensions.contains(&ext_lower.as_str()) {
            documents.push(file_info);
        } else {
            other.push(file_info);
        }
    }

    // Sort each category
    tracks.sort_by(|a, b| a.name.cmp(&b.name));
    artwork.sort_by(|a, b| a.name.cmp(&b.name));
    documents.sort_by(|a, b| a.name.cmp(&b.name));
    other.sort_by(|a, b| a.name.cmp(&b.name));

    // For torrents, we don't detect CUE/FLAC pairs yet - just use track files
    // TODO: Could add CUE/FLAC pair detection for torrents
    CategorizedFileInfo {
        audio: AudioContentInfo::TrackFiles(tracks),
        artwork,
        documents,
        other,
    }
}
