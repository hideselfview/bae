use crate::import::{ImportHandle, ImportRequest, MatchCandidate, MatchSource};
use crate::library::SharedLibraryManager;
use crate::ui::import_context::ImportContext;
use crate::ui::Route;
use dioxus::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;
use tracing::{error, info};

pub async fn handle_confirmation(
    candidate: MatchCandidate,
    folder_path: String,
    metadata: Option<crate::import::FolderMetadata>,
    import_source: crate::ui::components::import::ImportSource,
    torrent_source: Option<crate::import::TorrentSource>,
    seed_after_download: bool,
    import_context: Rc<ImportContext>,
    library_manager: SharedLibraryManager,
    import_service: ImportHandle,
    navigator: dioxus::router::Navigator,
    mut duplicate_album_id: Signal<Option<String>>,
    mut import_error_message: Signal<Option<String>>,
) {
    // Check for duplicates before importing
    match &candidate.source {
        MatchSource::Discogs(discogs_result) => {
            let master_id = discogs_result.master_id.map(|id| id.to_string());
            let release_id = Some(discogs_result.id.to_string());

            if let Ok(Some(duplicate)) = library_manager
                .get()
                .find_duplicate_by_discogs(master_id.as_deref(), release_id.as_deref())
                .await
            {
                duplicate_album_id.set(Some(duplicate.id));
                import_error_message.set(Some(format!(
                    "This release already exists in your library: {}",
                    duplicate.title
                )));
                return;
            }
        }
        MatchSource::MusicBrainz(mb_release) => {
            let release_id = Some(mb_release.release_id.clone());
            let release_group_id = Some(mb_release.release_group_id.clone());

            if let Ok(Some(duplicate)) = library_manager
                .get()
                .find_duplicate_by_musicbrainz(release_id.as_deref(), release_group_id.as_deref())
                .await
            {
                duplicate_album_id.set(Some(duplicate.id));
                import_error_message.set(Some(format!(
                    "This release already exists in your library: {}",
                    duplicate.title
                )));
                return;
            }
        }
    }

    // Extract master_year from metadata or release date
    let master_year = metadata.as_ref().and_then(|m| m.year).unwrap_or(1970);

    // Check if this is a CD import
    if import_source == crate::ui::components::import::ImportSource::Cd {
        match candidate.source.clone() {
            MatchSource::Discogs(_discogs_result) => {
                // CD imports currently only support MusicBrainz
                import_error_message
                    .set(Some("CD imports require MusicBrainz metadata".to_string()));
            }
            MatchSource::MusicBrainz(mb_release) => {
                info!(
                    "Starting CD import for MusicBrainz release: {}",
                    mb_release.title
                );

                let request = ImportRequest::CD {
                    discogs_release: None,
                    mb_release: Some(mb_release.clone()),
                    drive_path: PathBuf::from(folder_path),
                    master_year,
                };

                match import_service.send_request(request).await {
                    Ok((album_id, _release_id)) => {
                        info!("Import started, navigating to album: {}", album_id);
                        // Reset import state before navigating
                        import_context.reset();
                        navigator.push(Route::AlbumDetail {
                            album_id,
                            release_id: String::new(),
                        });
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to start import: {}", e);
                        error!("{}", error_msg);
                        import_error_message.set(Some(error_msg));
                    }
                }
            }
        }
    }
    // Check if this is a torrent import
    else if let Some(torrent_source) = torrent_source {
        match candidate.source.clone() {
            MatchSource::Discogs(discogs_result) => {
                let master_id = match discogs_result.master_id {
                    Some(id) => id.to_string(),
                    None => {
                        import_error_message
                            .set(Some("Discogs result has no master_id".to_string()));
                        return;
                    }
                };
                let release_id = discogs_result.id.to_string();

                match import_context.import_release(release_id, master_id).await {
                    Ok(discogs_release) => {
                        info!(
                            "Starting torrent import for Discogs release: {}",
                            discogs_release.title
                        );

                        let request = ImportRequest::Torrent {
                            torrent_source,
                            discogs_release: Some(discogs_release),
                            mb_release: None,
                            master_year,
                            seed_after_download,
                        };

                        match import_service.send_request(request).await {
                            Ok((album_id, _release_id)) => {
                                info!("Import started, navigating to album: {}", album_id);
                                // Reset import state before navigating
                                import_context.reset();
                                navigator.push(Route::AlbumDetail {
                                    album_id,
                                    release_id: String::new(),
                                });
                            }
                            Err(e) => {
                                let error_msg = format!("Failed to start import: {}", e);
                                error!("{}", error_msg);
                                import_error_message.set(Some(error_msg));
                            }
                        }
                    }
                    Err(e) => {
                        import_error_message
                            .set(Some(format!("Failed to fetch Discogs release: {}", e)));
                    }
                }
            }
            MatchSource::MusicBrainz(mb_release) => {
                info!(
                    "Starting torrent import for MusicBrainz release: {}",
                    mb_release.title
                );

                let request = ImportRequest::Torrent {
                    torrent_source,
                    discogs_release: None,
                    mb_release: Some(mb_release.clone()),
                    master_year,
                    seed_after_download,
                };

                match import_service.send_request(request).await {
                    Ok((album_id, _release_id)) => {
                        info!("Import started, navigating to album: {}", album_id);
                        // Reset import state before navigating
                        import_context.reset();
                        navigator.push(Route::AlbumDetail {
                            album_id,
                            release_id: String::new(),
                        });
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to start import: {}", e);
                        error!("{}", error_msg);
                        import_error_message.set(Some(error_msg));
                    }
                }
            }
        }
    } else {
        // Folder import
        match candidate.source.clone() {
            MatchSource::Discogs(discogs_result) => {
                let master_id = match discogs_result.master_id {
                    Some(id) => id.to_string(),
                    None => {
                        import_error_message
                            .set(Some("Discogs result has no master_id".to_string()));
                        return;
                    }
                };
                let release_id = discogs_result.id.to_string();

                match import_context.import_release(release_id, master_id).await {
                    Ok(discogs_release) => {
                        info!(
                            "Starting import for Discogs release: {}",
                            discogs_release.title
                        );

                        let request = ImportRequest::Folder {
                            discogs_release: Some(discogs_release),
                            mb_release: None,
                            folder: PathBuf::from(folder_path),
                            master_year,
                        };

                        match import_service.send_request(request).await {
                            Ok((album_id, _release_id)) => {
                                info!("Import started, navigating to album: {}", album_id);
                                // Reset import state before navigating
                                import_context.reset();
                                navigator.push(Route::AlbumDetail {
                                    album_id,
                                    release_id: String::new(),
                                });
                            }
                            Err(e) => {
                                let error_msg = format!("Failed to start import: {}", e);
                                error!("{}", error_msg);
                                import_error_message.set(Some(error_msg));
                            }
                        }
                    }
                    Err(e) => {
                        import_error_message
                            .set(Some(format!("Failed to fetch Discogs release: {}", e)));
                    }
                }
            }
            MatchSource::MusicBrainz(mb_release) => {
                info!(
                    "Starting import for MusicBrainz release: {}",
                    mb_release.title
                );

                let request = ImportRequest::Folder {
                    discogs_release: None,
                    mb_release: Some(mb_release.clone()),
                    folder: PathBuf::from(folder_path),
                    master_year,
                };

                match import_service.send_request(request).await {
                    Ok((album_id, _release_id)) => {
                        info!("Import started, navigating to album: {}", album_id);
                        // Reset import state before navigating
                        import_context.reset();
                        navigator.push(Route::AlbumDetail {
                            album_id,
                            release_id: String::new(),
                        });
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to start import: {}", e);
                        error!("{}", error_msg);
                        import_error_message.set(Some(error_msg));
                    }
                }
            }
        }
    }
}
