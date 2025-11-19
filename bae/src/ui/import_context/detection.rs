use super::state::ImportContext;
use crate::import::{
    cover_art, detect_folder_contents, MatchCandidate, MatchSource, TorrentSource,
};
use crate::musicbrainz::{lookup_by_discid, ExternalUrls, MbRelease};
use crate::torrent::parse_torrent_info;
use crate::ui::components::import::FileInfo;
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
    pub files: Vec<FileInfo>,
    pub discid_result: Option<DiscIdLookupResult>,
}

/// Handle DiscID lookup result: process 0/1/multiple matches and return result
async fn handle_discid_lookup_result(
    ctx: &ImportContext,
    releases: Vec<MbRelease>,
    external_urls: ExternalUrls,
) -> DiscIdLookupResult {
    if releases.is_empty() {
        info!("No exact matches found");
        DiscIdLookupResult::NoMatches
    } else if releases.len() == 1 {
        info!("✅ Single exact match found");
        let mb_release = releases[0].clone();
        let cover_art_url = cover_art::fetch_cover_art_for_mb_release(
            &mb_release,
            &external_urls,
            Some(&ctx.discogs_client),
        )
        .await;
        let candidate = MatchCandidate {
            source: MatchSource::MusicBrainz(mb_release),
            confidence: 100.0,
            match_reasons: vec!["Exact DiscID match".to_string()],
            cover_art_url,
        };
        DiscIdLookupResult::SingleMatch(Box::new(candidate))
    } else {
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

        DiscIdLookupResult::MultipleMatches(candidates)
    }
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

    // Convert file list to UI FileInfo format (before storing torrent_info)
    let mut files: Vec<FileInfo> = torrent_info
        .files
        .iter()
        .map(|tf| {
            let path_buf = PathBuf::from(&tf.path);
            let name = path_buf
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            let format = path_buf
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
    ctx.set_folder_files(files);

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
    ctx.set_is_detecting(true);

    let folder_contents = detect_folder_contents(PathBuf::from(path.clone()))
        .map_err(|e| format!("Failed to detect folder contents: {}", e))?;

    ctx.set_is_detecting(false);

    // Convert FileEntry to UI FileInfo
    let files: Vec<FileInfo> = folder_contents
        .files
        .into_iter()
        .map(|entry| FileInfo {
            name: entry.name,
            size: entry.size,
            format: entry.extension.to_uppercase(),
        })
        .collect();

    let metadata = folder_contents.metadata;

    info!(
        "Detected metadata: artist={:?}, album={:?}, year={:?}, mb_discid={:?}",
        metadata.artist, metadata.album, metadata.year, metadata.mb_discid
    );

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
