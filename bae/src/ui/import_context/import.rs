use super::state::ImportContext;
use crate::discogs::DiscogsRelease;
use crate::import::{ImportRequest, MatchCandidate, MatchSource};
use crate::ui::components::import::ImportSource;
use crate::ui::Route;
use dioxus::prelude::*;
use dioxus::router::Navigator;
use std::path::PathBuf;
use tracing::{error, info};

pub async fn import_release(
    ctx: &ImportContext,
    release_id: String,
    master_id: String,
) -> Result<DiscogsRelease, String> {
    ctx.set_error_message(None);

    match ctx.discogs_client.get_release(&release_id).await {
        Ok(release) => {
            // The release from API already has master_id, but we use the one passed to us
            // (which might differ if we're importing via master vs specific release)
            let mut release = release;
            release.master_id = master_id;
            Ok(release)
        }
        Err(e) => {
            let error = format!("Failed to fetch release details: {}", e);
            ctx.set_error_message(Some(error.clone()));
            Err(error)
        }
    }
}

/// Confirm a match candidate and start the import workflow.
pub async fn confirm_and_start_import(
    ctx: &ImportContext,
    candidate: MatchCandidate,
    import_source: ImportSource,
    navigator: Navigator,
) -> Result<(), String> {
    // Check for duplicates before importing
    match &candidate.source {
        MatchSource::Discogs(discogs_result) => {
            let master_id = discogs_result.master_id.map(|id| id.to_string());
            let release_id = Some(discogs_result.id.to_string());

            if let Ok(Some(duplicate)) = ctx
                .library_manager
                .get()
                .find_duplicate_by_discogs(master_id.as_deref(), release_id.as_deref())
                .await
            {
                ctx.set_duplicate_album_id(Some(duplicate.id));
                ctx.set_import_error_message(Some(format!(
                    "This release already exists in your library: {}",
                    duplicate.title
                )));
                return Err("Duplicate album found".to_string());
            }
        }
        MatchSource::MusicBrainz(mb_release) => {
            let release_id = Some(mb_release.release_id.clone());
            let release_group_id = Some(mb_release.release_group_id.clone());

            if let Ok(Some(duplicate)) = ctx
                .library_manager
                .get()
                .find_duplicate_by_musicbrainz(release_id.as_deref(), release_group_id.as_deref())
                .await
            {
                ctx.set_duplicate_album_id(Some(duplicate.id));
                ctx.set_import_error_message(Some(format!(
                    "This release already exists in your library: {}",
                    duplicate.title
                )));
                return Err("Duplicate album found".to_string());
            }
        }
    }

    // Extract master_year from metadata or release date
    let metadata = ctx.detected_metadata().read().clone();
    let master_year = metadata.as_ref().and_then(|m| m.year).unwrap_or(1970);

    // Build import request based on source
    let request = match import_source {
        ImportSource::Folder => {
            let folder_path = ctx.folder_path().read().clone();
            match candidate.source.clone() {
                MatchSource::Discogs(discogs_result) => {
                    let master_id = match discogs_result.master_id {
                        Some(id) => id.to_string(),
                        None => {
                            return Err("Discogs result has no master_id".to_string());
                        }
                    };
                    let release_id = discogs_result.id.to_string();

                    let discogs_release = import_release(ctx, release_id, master_id).await?;

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
            let torrent_source = ctx
                .torrent_source()
                .read()
                .clone()
                .ok_or_else(|| "No torrent source available".to_string())?;
            let seed_after_download = *ctx.seed_after_download().read();
            let torrent_metadata = ctx
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

                    let discogs_release = import_release(ctx, release_id, master_id).await?;

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
            let folder_path = ctx.folder_path().read().clone();
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
    match ctx.import_service.send_request(request).await {
        Ok((album_id, _release_id)) => {
            info!("Import started, navigating to album: {}", album_id);
            // Reset import state before navigating
            // We need to call reset() here. Since reset() is on ImportContext, we can call it if it's public.
            // But reset() is in navigation.rs (planned) or state.rs?
            // I'll put reset() in navigation.rs and call it here.
            // Or I can put reset() in state.rs as it just resets state.
            // Let's assume reset() is available on ctx (I'll add it to state.rs or navigation.rs).
            // If it's in navigation.rs, I need to import it.
            // For now, I'll assume I can call a reset function.
            // I'll use a placeholder and fix it later.
            // Actually, I'll put reset() in state.rs because it touches almost all fields.
            ctx.reset();
            navigator.push(Route::AlbumDetail {
                album_id,
                release_id: String::new(),
            });
            Ok(())
        }
        Err(e) => {
            let error_msg = format!("Failed to start import: {}", e);
            error!("{}", error_msg);
            ctx.set_import_error_message(Some(error_msg.clone()));
            Err(error_msg)
        }
    }
}
