use super::state::ImportContext;
use super::types::ImportPhase;
use crate::import::{cover_art, detect_metadata, MatchCandidate, MatchSource, TorrentSource};
use crate::musicbrainz::{lookup_by_discid, ExternalUrls, MbRelease};
use crate::torrent::parse_torrent_info;
use crate::ui::components::import::FileInfo;
use dioxus::prelude::*;
use std::path::PathBuf;
use tracing::{info, warn};

/// Handle DiscID lookup result: process 0/1/multiple matches and update context
async fn handle_discid_lookup_result(
    ctx: &ImportContext,
    releases: Vec<MbRelease>,
    external_urls: ExternalUrls,
    fallback_search_query: String,
) {
    ctx.set_is_looking_up(false);

    if releases.is_empty() {
        info!("No exact matches found, proceeding to manual search");
        ctx.set_search_query(fallback_search_query);
        ctx.set_import_phase(ImportPhase::ManualSearch);
    } else if releases.len() == 1 {
        info!("✅ Single exact match found, auto-proceeding");
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
        ctx.set_confirmed_candidate(Some(candidate));
        ctx.set_import_phase(ImportPhase::Confirmation);
    } else {
        info!(
            "Found {} exact matches, showing for selection",
            releases.len()
        );

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

        ctx.set_exact_match_candidates(candidates);
        ctx.set_import_phase(ImportPhase::ExactLookup);
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

/// Load a folder for import: read files, detect metadata, and start lookup flow.
pub async fn load_folder_for_import(ctx: &ImportContext, path: String) -> Result<(), String> {
    // Reset state for new folder selection
    ctx.set_folder_path(path.clone());
    ctx.set_detected_metadata(None);
    ctx.set_exact_match_candidates(Vec::new());
    ctx.set_selected_match_index(None);
    ctx.set_confirmed_candidate(None);
    ctx.set_import_error_message(None);
    ctx.set_duplicate_album_id(None);
    ctx.set_import_phase(ImportPhase::MetadataDetection);
    ctx.set_is_detecting(true);

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
    ctx.set_folder_files(files);

    let metadata = detect_metadata(PathBuf::from(path.clone()))
        .map_err(|e| format!("Failed to detect metadata: {}", e))?;

    ctx.set_is_detecting(false);

    info!(
        "Detected metadata: artist={:?}, album={:?}, year={:?}, mb_discid={:?}",
        metadata.artist, metadata.album, metadata.year, metadata.mb_discid
    );
    ctx.set_detected_metadata(Some(metadata.clone()));

    let Some(mb_discid) = &metadata.mb_discid else {
        info!("No MB DiscID found, proceeding to manual search");
        ctx.init_search_query_from_metadata(&metadata);
        ctx.set_import_phase(ImportPhase::ManualSearch);
        return Ok(());
    };

    ctx.set_is_looking_up(true);
    info!("🎵 Found MB DiscID: {}, performing exact lookup", mb_discid);

    ctx.init_search_query_from_metadata(&metadata);
    let fallback_query = ctx.search_query().read().clone();

    match lookup_by_discid(mb_discid).await {
        Ok((releases, external_urls)) => {
            handle_discid_lookup_result(ctx, releases, external_urls, fallback_query).await;
        }
        Err(e) => {
            info!(
                "MB DiscID lookup failed: {}, proceeding to manual search",
                e
            );
            ctx.set_is_looking_up(false);
            ctx.set_search_query(fallback_query);
            ctx.set_import_phase(ImportPhase::ManualSearch);
        }
    }

    Ok(())
}

/// Load a CD for import: detect TOC, lookup by DiscID, and start import flow.
pub async fn load_cd_for_import(
    ctx: &ImportContext,
    drive_path: String,
    disc_id: String,
) -> Result<(), String> {
    // Reset state for new CD selection
    ctx.set_folder_path(drive_path.clone());
    ctx.set_detected_metadata(None);
    ctx.set_exact_match_candidates(Vec::new());
    ctx.set_selected_match_index(None);
    ctx.set_confirmed_candidate(None);
    ctx.set_import_error_message(None);
    ctx.set_duplicate_album_id(None);
    ctx.set_import_phase(ImportPhase::MetadataDetection);
    ctx.set_is_looking_up(true);

    match lookup_by_discid(&disc_id).await {
        Ok((releases, external_urls)) => {
            handle_discid_lookup_result(ctx, releases, external_urls, drive_path.clone()).await;
            Ok(())
        }
        Err(e) => {
            ctx.set_is_looking_up(false);
            ctx.set_search_query(drive_path.clone());
            ctx.set_import_phase(ImportPhase::ManualSearch);
            Err(format!("Failed to lookup by DiscID: {}", e))
        }
    }
}
