use super::file_list::FileInfo;
use super::{
    file_list::FileList, folder_selector::FolderSelector, manual_search_panel::ManualSearchPanel,
    match_list::MatchList,
};
use crate::import::{ImportRequest, MatchCandidate, MatchSource};
use crate::library::use_import_service;
use crate::library::use_library_manager;
use crate::musicbrainz::lookup_by_discid;
use crate::ui::components::import::{CdRipper, ImportSource, ImportSourceSelector, TorrentInput};
use crate::ui::import_context::ImportContext;
use crate::ui::Route;
use dioxus::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;
use tracing::{error, info};

#[component]
pub fn FolderDetectionPage() -> Element {
    let import_context = use_context::<Rc<ImportContext>>();
    let library_manager = use_library_manager();
    let import_service = use_import_service();
    let navigator = use_navigator();

    // Copy signals out of Rc (signals are Copy)
    let folder_path = import_context.folder_path;
    let detected_metadata = import_context.detected_metadata;
    let import_phase = import_context.import_phase;
    let exact_match_candidates = import_context.exact_match_candidates;
    let selected_match_index = import_context.selected_match_index;
    let confirmed_candidate = import_context.confirmed_candidate;
    let is_detecting = import_context.is_detecting;
    let is_looking_up = import_context.is_looking_up;
    let import_error_message = import_context.import_error_message;
    let duplicate_album_id = import_context.duplicate_album_id;
    let search_query = import_context.search_query;
    let folder_files = import_context.folder_files;
    let mut selected_source = use_signal(|| ImportSource::Folder);
    let torrent_source = use_signal(|| None::<crate::import::TorrentSource>);
    let seed_after_download = use_signal(|| true);
    let cd_toc_info: Signal<Option<(String, u8, u8)>> = use_signal(|| None); // (disc_id, first_track, last_track)

    let on_source_select = {
        let import_context = import_context.clone();
        let mut torrent_source_signal = torrent_source;
        move |source: ImportSource| {
            selected_source.set(source);
            // Reset import context and torrent source when switching sources
            import_context.reset();
            torrent_source_signal.set(None);
        }
    };

    let on_torrent_file_select = {
        let import_context_for_metadata = import_context.clone();
        let mut folder_path = folder_path;
        let mut detected_metadata = detected_metadata;
        let mut exact_match_candidates = exact_match_candidates;
        let mut selected_match_index = selected_match_index;
        let mut confirmed_candidate = confirmed_candidate;
        let mut import_error_message = import_error_message;
        let mut duplicate_album_id = duplicate_album_id;
        let mut import_phase = import_phase;
        let mut is_detecting = is_detecting;
        let mut torrent_source_signal = torrent_source;
        let mut seed_after_download_signal = seed_after_download;

        // Extract signals for metadata detection before the closure
        let mut detected_metadata_for_async = import_context_for_metadata.detected_metadata;
        let mut is_looking_up_for_async = import_context_for_metadata.is_looking_up;
        let mut exact_match_candidates_for_async =
            import_context_for_metadata.exact_match_candidates;
        let mut confirmed_candidate_for_async = import_context_for_metadata.confirmed_candidate;
        let mut import_phase_for_async = import_context_for_metadata.import_phase;
        let mut search_query_for_async = import_context_for_metadata.search_query;

        move |(path, seed_flag): (PathBuf, bool)| {
            // Store torrent source and seed flag
            torrent_source_signal.set(Some(crate::import::TorrentSource::File(path.clone())));
            seed_after_download_signal.set(seed_flag);

            // Reset state
            folder_path.set(path.to_string_lossy().to_string());
            detected_metadata.set(None);
            exact_match_candidates.set(Vec::new());
            selected_match_index.set(None);
            confirmed_candidate.set(None);
            import_error_message.set(None);
            duplicate_album_id.set(None);
            import_phase.set(crate::ui::import_context::ImportPhase::MetadataDetection);
            is_detecting.set(true);

            // Clone everything needed for spawn (to keep closure FnMut)
            let mut is_detecting = is_detecting;
            let mut import_phase = import_phase;
            let mut import_error_message = import_error_message;
            let mut search_query = search_query;
            let mut folder_files = folder_files;
            let import_context_for_async = import_context_for_metadata.clone();
            let client_for_torrent = import_context_for_metadata.torrent_client_default();
            let path = path.clone();

            spawn(async move {
                // Add torrent file using shared client
                let torrent_handle = match client_for_torrent.add_torrent_file(&path).await {
                    Ok(handle) => handle,
                    Err(e) => {
                        import_error_message
                            .set(Some(format!("Failed to add torrent file: {}", e)));
                        is_detecting.set(false);
                        import_phase.set(crate::ui::import_context::ImportPhase::FolderSelection);
                        return;
                    }
                };

                // Wait for metadata (should be immediate for torrent files, but needed for consistency)
                if let Err(e) = torrent_handle.wait_for_metadata().await {
                    import_error_message
                        .set(Some(format!("Failed to get torrent metadata: {}", e)));
                    is_detecting.set(false);
                    import_phase.set(crate::ui::import_context::ImportPhase::FolderSelection);
                    return;
                }

                // Get torrent name
                let torrent_name = match torrent_handle.name().await {
                    Ok(name) => name,
                    Err(e) => {
                        import_error_message
                            .set(Some(format!("Failed to get torrent name: {}", e)));
                        is_detecting.set(false);
                        import_phase.set(crate::ui::import_context::ImportPhase::FolderSelection);
                        return;
                    }
                };

                // Get file list from torrent
                let torrent_files = match torrent_handle.get_file_list().await {
                    Ok(files) => files,
                    Err(e) => {
                        import_error_message
                            .set(Some(format!("Failed to get torrent file list: {}", e)));
                        is_detecting.set(false);
                        import_phase.set(crate::ui::import_context::ImportPhase::FolderSelection);
                        return;
                    }
                };

                // Convert torrent files to FileInfo format
                let mut files: Vec<FileInfo> = torrent_files
                    .into_iter()
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
                folder_files.set(files);

                info!(
                    "Torrent loaded: {} ({} files)",
                    torrent_name,
                    folder_files.read().len()
                );

                // Try to detect metadata from CUE/log files in background
                let path_for_metadata = path.clone();
                let torrent_name_for_metadata = torrent_name.clone();
                let mut is_detecting_for_async = is_detecting;
                let client_for_metadata = import_context_for_async.torrent_client_default();

                // Set detecting state to show UI feedback
                is_detecting_for_async.set(true);

                spawn(async move {
                    use crate::musicbrainz::lookup_by_discid;
                    use crate::torrent::detect_metadata_from_torrent_file;

                    // Try to detect metadata from CUE/log files using shared client
                    let result =
                        detect_metadata_from_torrent_file(&path_for_metadata, &client_for_metadata)
                            .await;

                    // Check if detection was cancelled before processing results
                    if !*is_detecting_for_async.read() {
                        info!("Metadata detection was cancelled, ignoring results");
                        return;
                    }

                    match result {
                        Ok(Some(metadata)) => {
                            info!("Detected metadata from torrent: {:?}", metadata);
                            detected_metadata_for_async.set(Some(metadata.clone()));
                            is_detecting_for_async.set(false);

                            // Helper to initialize search query from metadata
                            let mut init_search_query =
                                |metadata: &crate::import::FolderMetadata| {
                                    let mut query_parts = Vec::new();
                                    if let Some(ref artist) = metadata.artist {
                                        query_parts.push(artist.clone());
                                    }
                                    if let Some(ref album) = metadata.album {
                                        query_parts.push(album.clone());
                                    }
                                    search_query_for_async.set(query_parts.join(" "));
                                };

                            // Try exact lookup if MB DiscID available
                            if let Some(ref mb_discid) = metadata.mb_discid {
                                is_looking_up_for_async.set(true);
                                info!("🎵 Found MB DiscID: {}, performing exact lookup", mb_discid);

                                match lookup_by_discid(mb_discid).await {
                                    Ok((releases, _external_urls)) => {
                                        if releases.is_empty() {
                                            info!("No exact matches found, proceeding to manual search");
                                            init_search_query(&metadata);
                                            import_phase_for_async.set(
                                                crate::ui::import_context::ImportPhase::ManualSearch,
                                            );
                                        } else if releases.len() == 1 {
                                            // Single exact match - auto-proceed to confirmation
                                            info!("✅ Single exact match found, auto-proceeding");
                                            let mb_release = releases[0].clone();
                                            let candidate = crate::import::MatchCandidate {
                                                source: crate::import::MatchSource::MusicBrainz(
                                                    mb_release,
                                                ),
                                                confidence: 100.0,
                                                match_reasons: vec![
                                                    "Exact DiscID match".to_string()
                                                ],
                                            };
                                            confirmed_candidate_for_async.set(Some(candidate));
                                            import_phase_for_async.set(
                                                crate::ui::import_context::ImportPhase::Confirmation,
                                            );
                                        } else {
                                            // Multiple exact matches - show for selection
                                            info!(
                                                "Found {} exact matches, showing for selection",
                                                releases.len()
                                            );
                                            let candidates: Vec<crate::import::MatchCandidate> = releases
                                                .into_iter()
                                                .map(|mb_release| crate::import::MatchCandidate {
                                                    source: crate::import::MatchSource::MusicBrainz(mb_release),
                                                    confidence: 100.0,
                                                    match_reasons: vec!["Exact DiscID match".to_string()],
                                                })
                                                .collect();
                                            exact_match_candidates_for_async.set(candidates);
                                            import_phase_for_async.set(
                                                crate::ui::import_context::ImportPhase::ExactLookup,
                                            );
                                        }
                                        is_looking_up_for_async.set(false);
                                    }
                                    Err(e) => {
                                        info!("MB DiscID lookup failed: {}, proceeding to manual search", e);
                                        is_looking_up_for_async.set(false);
                                        init_search_query(&metadata);
                                        import_phase_for_async.set(
                                            crate::ui::import_context::ImportPhase::ManualSearch,
                                        );
                                    }
                                }
                            } else {
                                // No MB DiscID, proceed to manual search with detected metadata
                                info!("No MB DiscID found, proceeding to manual search");
                                init_search_query(&metadata);
                                import_phase_for_async
                                    .set(crate::ui::import_context::ImportPhase::ManualSearch);
                            }
                        }
                        Ok(None) => {
                            // No CUE/log files or detection failed, proceed with torrent name
                            info!(
                                "No metadata detected from torrent, using torrent name for search"
                            );
                            is_detecting_for_async.set(false);
                            search_query_for_async.set(torrent_name_for_metadata.clone());
                            import_phase_for_async
                                .set(crate::ui::import_context::ImportPhase::ManualSearch);
                        }
                        Err(e) => {
                            warn!("Failed to detect metadata from torrent: {}", e);
                            // Fall back to torrent name
                            is_detecting_for_async.set(false);
                            search_query_for_async.set(torrent_name_for_metadata.clone());
                            import_phase_for_async
                                .set(crate::ui::import_context::ImportPhase::ManualSearch);
                        }
                    }
                });

                // Initialize search query with torrent name (will be updated if metadata is detected)
                search_query.set(torrent_name.clone());
                import_phase.set(crate::ui::import_context::ImportPhase::ManualSearch);
            });
        }
    };

    let on_magnet_link = move |(magnet, seed_after_download): (String, bool)| {
        // TODO: Handle magnet link
        let _ = (magnet, seed_after_download); // Placeholder until implementation
        info!("Magnet link selection not yet implemented");
    };

    let on_torrent_error = {
        let mut import_error_message = import_error_message;
        move |error: String| {
            import_error_message.set(Some(error));
        }
    };

    let on_folder_select = {
        let import_context_for_detect = import_context.clone();

        let mut folder_path = folder_path;
        let mut detected_metadata = detected_metadata;
        let mut exact_match_candidates = exact_match_candidates;
        let mut selected_match_index = selected_match_index;
        let mut confirmed_candidate = confirmed_candidate;
        let mut import_error_message = import_error_message;
        let mut duplicate_album_id = duplicate_album_id;
        let mut import_phase = import_phase;
        let mut is_detecting = is_detecting;

        move |path: String| {
            folder_path.set(path.clone());
            detected_metadata.set(None);
            exact_match_candidates.set(Vec::new());
            selected_match_index.set(None);
            confirmed_candidate.set(None);
            import_error_message.set(None);
            duplicate_album_id.set(None);
            import_phase.set(crate::ui::import_context::ImportPhase::MetadataDetection);
            is_detecting.set(true);

            // Read files from folder
            let folder_path_clone = path.clone();
            let import_context_for_files = import_context_for_detect.clone();
            let mut folder_files_for_read = import_context_for_files.folder_files;
            spawn(async move {
                let mut files = Vec::new();
                if let Ok(entries) = std::fs::read_dir(&folder_path_clone) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            let name = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                            let format = path
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("")
                                .to_uppercase();
                            files.push(FileInfo { name, size, format });
                        }
                    }
                    files.sort_by(|a, b| a.name.cmp(&b.name));
                }
                folder_files_for_read.set(files);
            });

            let import_context_for_detect = import_context_for_detect.clone();
            let mut detected_metadata = import_context_for_detect.detected_metadata;
            let mut is_detecting = import_context_for_detect.is_detecting;
            let mut is_looking_up = import_context_for_detect.is_looking_up;
            let mut import_phase = import_context_for_detect.import_phase;
            let mut confirmed_candidate = import_context_for_detect.confirmed_candidate;
            let mut exact_match_candidates = import_context_for_detect.exact_match_candidates;
            let mut import_error_message = import_context_for_detect.import_error_message;
            let mut search_query = import_context_for_detect.search_query;

            spawn(async move {
                // Helper to initialize search query from metadata
                let mut init_search_query = |metadata: &crate::import::FolderMetadata| {
                    let mut query_parts = Vec::new();
                    if let Some(ref artist) = metadata.artist {
                        query_parts.push(artist.clone());
                    }
                    if let Some(ref album) = metadata.album {
                        query_parts.push(album.clone());
                    }
                    search_query.set(query_parts.join(" "));
                };

                // Phase 2: Detect metadata
                match import_context_for_detect
                    .detect_folder_metadata(path.clone())
                    .await
                {
                    Ok(metadata) => {
                        detected_metadata.set(Some(metadata.clone()));
                        is_detecting.set(false);

                        // Phase 3: Exact lookup if MB DiscID available
                        if let Some(ref mb_discid) = metadata.mb_discid {
                            is_looking_up.set(true);
                            info!("🎵 Found MB DiscID: {}, performing exact lookup", mb_discid);

                            match lookup_by_discid(mb_discid).await {
                                Ok((releases, _external_urls)) => {
                                    if releases.is_empty() {
                                        info!(
                                            "No exact matches found, proceeding to manual search"
                                        );
                                        init_search_query(&metadata);
                                        import_phase.set(
                                            crate::ui::import_context::ImportPhase::ManualSearch,
                                        );
                                    } else if releases.len() == 1 {
                                        // Single exact match - auto-proceed to confirmation
                                        info!("✅ Single exact match found, auto-proceeding");
                                        let mb_release = releases[0].clone();
                                        let candidate = MatchCandidate {
                                            source: MatchSource::MusicBrainz(mb_release),
                                            confidence: 100.0,
                                            match_reasons: vec!["Exact DiscID match".to_string()],
                                        };
                                        confirmed_candidate.set(Some(candidate));
                                        import_phase.set(
                                            crate::ui::import_context::ImportPhase::Confirmation,
                                        );
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
                                                match_reasons: vec![
                                                    "Exact DiscID match".to_string()
                                                ],
                                            })
                                            .collect();
                                        exact_match_candidates.set(candidates);
                                        import_phase.set(
                                            crate::ui::import_context::ImportPhase::ExactLookup,
                                        );
                                    }
                                    is_looking_up.set(false);
                                }
                                Err(e) => {
                                    info!(
                                        "MB DiscID lookup failed: {}, proceeding to manual search",
                                        e
                                    );
                                    is_looking_up.set(false);
                                    init_search_query(&metadata);
                                    import_phase
                                        .set(crate::ui::import_context::ImportPhase::ManualSearch);
                                }
                            }
                        } else {
                            // No MB DiscID, proceed to manual search
                            info!("No MB DiscID found, proceeding to manual search");
                            init_search_query(&metadata);
                            import_phase.set(crate::ui::import_context::ImportPhase::ManualSearch);
                        }
                    }
                    Err(e) => {
                        import_error_message.set(Some(e));
                        is_detecting.set(false);
                        import_phase.set(crate::ui::import_context::ImportPhase::FolderSelection);
                    }
                }
            });
        }
    };

    let on_exact_match_select = {
        let mut selected_match_index = selected_match_index;
        let mut confirmed_candidate = confirmed_candidate;
        let mut import_phase = import_phase;
        move |index: usize| {
            selected_match_index.set(Some(index));
            if let Some(candidate) = exact_match_candidates.read().get(index) {
                confirmed_candidate.set(Some(candidate.clone()));
                import_phase.set(crate::ui::import_context::ImportPhase::Confirmation);
            }
        }
    };

    let on_manual_match_select = {
        let mut selected_match_index = selected_match_index;
        move |index: usize| {
            selected_match_index.set(Some(index));
        }
    };

    let on_confirm_from_manual = {
        let import_context_for_reset = import_context.clone();
        let library_manager = library_manager.clone();
        let import_service = import_service.clone();
        let torrent_source_signal = torrent_source;
        let seed_after_download_signal = seed_after_download;
        let selected_source_signal = selected_source;
        move |candidate: MatchCandidate| {
            let folder = folder_path.read().clone();
            let metadata = detected_metadata.read().clone();
            let torrent_source_opt = torrent_source_signal.read().clone();
            let seed_flag = *seed_after_download_signal.read();
            let current_source = selected_source_signal.read().clone();
            let import_service = import_service.clone();
            let mut duplicate_album_id = duplicate_album_id;
            let mut import_error_message = import_error_message;
            let import_context_for_reset = import_context_for_reset.clone();
            let library_manager = library_manager.clone();

            spawn(async move {
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
                            .find_duplicate_by_musicbrainz(
                                release_id.as_deref(),
                                release_group_id.as_deref(),
                            )
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
                if current_source == ImportSource::Cd {
                    match candidate.source.clone() {
                        MatchSource::Discogs(_discogs_result) => {
                            // CD imports currently only support MusicBrainz
                            import_error_message
                                .set(Some("CD imports require MusicBrainz metadata".to_string()));
                            return;
                        }
                        MatchSource::MusicBrainz(mb_release) => {
                            info!(
                                "Starting CD import for MusicBrainz release: {}",
                                mb_release.title
                            );

                            let request = ImportRequest::CD {
                                discogs_release: None,
                                mb_release: Some(mb_release.clone()),
                                drive_path: PathBuf::from(folder),
                                master_year,
                            };

                            match import_service.send_request(request).await {
                                Ok((album_id, _release_id)) => {
                                    info!("Import started, navigating to album: {}", album_id);
                                    // Reset import state before navigating
                                    import_context_for_reset.reset();
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
                else if let Some(torrent_source) = torrent_source_opt {
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

                            match import_context_for_reset
                                .import_release(release_id, master_id)
                                .await
                            {
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
                                        seed_after_download: seed_flag,
                                    };

                                    match import_service.send_request(request).await {
                                        Ok((album_id, _release_id)) => {
                                            info!(
                                                "Import started, navigating to album: {}",
                                                album_id
                                            );
                                            // Reset import state before navigating
                                            import_context_for_reset.reset();
                                            navigator.push(Route::AlbumDetail {
                                                album_id,
                                                release_id: String::new(),
                                            });
                                        }
                                        Err(e) => {
                                            let error_msg =
                                                format!("Failed to start import: {}", e);
                                            error!("{}", error_msg);
                                            import_error_message.set(Some(error_msg));
                                        }
                                    }
                                }
                                Err(e) => {
                                    import_error_message.set(Some(format!(
                                        "Failed to fetch Discogs release: {}",
                                        e
                                    )));
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
                                seed_after_download: seed_flag,
                            };

                            match import_service.send_request(request).await {
                                Ok((album_id, _release_id)) => {
                                    info!("Import started, navigating to album: {}", album_id);
                                    // Reset import state before navigating
                                    import_context_for_reset.reset();
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
                    // Folder import (existing logic)
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

                            match import_context_for_reset
                                .import_release(release_id, master_id)
                                .await
                            {
                                Ok(discogs_release) => {
                                    info!(
                                        "Starting import for Discogs release: {}",
                                        discogs_release.title
                                    );

                                    let request = ImportRequest::Folder {
                                        discogs_release: Some(discogs_release),
                                        mb_release: None,
                                        folder: PathBuf::from(folder),
                                        master_year,
                                    };

                                    match import_service.send_request(request).await {
                                        Ok((album_id, _release_id)) => {
                                            info!(
                                                "Import started, navigating to album: {}",
                                                album_id
                                            );
                                            // Reset import state before navigating
                                            import_context_for_reset.reset();
                                            navigator.push(Route::AlbumDetail {
                                                album_id,
                                                release_id: String::new(),
                                            });
                                        }
                                        Err(e) => {
                                            let error_msg =
                                                format!("Failed to start import: {}", e);
                                            error!("{}", error_msg);
                                            import_error_message.set(Some(error_msg));
                                        }
                                    }
                                }
                                Err(e) => {
                                    import_error_message.set(Some(format!(
                                        "Failed to fetch Discogs release: {}",
                                        e
                                    )));
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
                                folder: PathBuf::from(folder),
                                master_year,
                            };

                            match import_service.send_request(request).await {
                                Ok((album_id, _release_id)) => {
                                    info!("Import started, navigating to album: {}", album_id);
                                    // Reset import state before navigating
                                    import_context_for_reset.reset();
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
            });
        }
    };

    let on_edit = {
        let mut confirmed_candidate = confirmed_candidate;
        let mut selected_match_index = selected_match_index;
        let mut import_phase = import_phase;
        let mut search_query = search_query;
        move |_| {
            confirmed_candidate.set(None);
            selected_match_index.set(None);
            if !exact_match_candidates.read().is_empty() {
                import_phase.set(crate::ui::import_context::ImportPhase::ExactLookup);
            } else {
                // Initialize search query from detected metadata when transitioning to manual search
                if let Some(metadata) = detected_metadata.read().as_ref() {
                    let mut query_parts = Vec::new();
                    if let Some(ref artist) = metadata.artist {
                        query_parts.push(artist.clone());
                    }
                    if let Some(ref album) = metadata.album {
                        query_parts.push(album.clone());
                    }
                    if !query_parts.is_empty() {
                        search_query.set(query_parts.join(" "));
                    }
                }
                import_phase.set(crate::ui::import_context::ImportPhase::ManualSearch);
            }
        }
    };

    let on_confirm = move |_| {
        if let Some(candidate) = confirmed_candidate.read().as_ref().cloned() {
            on_confirm_from_manual(candidate);
        }
    };

    let on_change_folder = {
        let import_context = import_context.clone();
        move |_| {
            import_context.reset();
        }
    };

    // Check if there are .cue files available for metadata detection (computed before rsx!)
    let has_cue_files_for_manual = {
        let files = folder_files.read();
        let result = files
            .iter()
            .any(|f| f.format.to_lowercase() == "cue" || f.format.to_lowercase() == "log");
        drop(files);
        result
    };

    rsx! {
        div { class: "max-w-4xl mx-auto p-6",
            div { class: "mb-6",
                h1 { class: "text-2xl font-bold text-white", "Import" }
            }

            // Source selector
            div { class: "bg-white rounded-lg shadow p-6 mb-6",
                ImportSourceSelector {
                    selected_source,
                    on_source_select,
                }
            }

            // Phase 1: Source Selection (Folder, Torrent, or CD)
            if *import_phase.read() == crate::ui::import_context::ImportPhase::FolderSelection {
                div { class: "bg-white rounded-lg shadow p-6",
                    if *selected_source.read() == ImportSource::Folder {
                        FolderSelector {
                            on_select: on_folder_select,
                            on_error: {
                                let mut import_error_message = import_error_message;
                                move |e: String| {
                                    import_error_message.set(Some(e));
                                }
                            }
                        }
                    } else if *selected_source.read() == ImportSource::Torrent {
                        TorrentInput {
                            on_file_select: on_torrent_file_select,
                            on_magnet_link: on_magnet_link,
                            on_error: on_torrent_error,
                        }
                    } else {
                        CdRipper {
                            on_drive_select: {
                                let mut folder_path = folder_path;
                                let mut import_phase = import_phase;
                                let mut cd_toc_info = cd_toc_info;
                                // Extract signals from Rc before the closure (signals are Copy)
                                let mut is_looking_up_signal = is_looking_up;
                                let mut exact_match_candidates_signal = exact_match_candidates;
                                let mut detected_metadata_signal = detected_metadata;
                                let mut import_error_message_signal = import_error_message;
                                let mut search_query_signal = search_query;
                                let mut confirmed_candidate_signal = confirmed_candidate;
                                move |drive_path: PathBuf| {
                                    folder_path.set(drive_path.to_string_lossy().to_string());
                                    import_phase.set(crate::ui::import_context::ImportPhase::ExactLookup);
                                    is_looking_up_signal.set(true);
                                    exact_match_candidates_signal.set(Vec::new());
                                    detected_metadata_signal.set(None);
                                    import_error_message_signal.set(None);

                                    // Read TOC from CD and look up by DiscID
                                    let drive_path_clone = drive_path.clone();
                                    let mut cd_toc_info_async = cd_toc_info;
                                    // Clone signals for async task (signals are Copy)
                                    let mut is_looking_up_async = is_looking_up_signal;
                                    let mut import_phase_async = import_phase;
                                    let mut search_query_async = search_query_signal;
                                    let mut confirmed_candidate_async = confirmed_candidate_signal;
                                    let mut exact_match_candidates_async = exact_match_candidates_signal;
                                    let mut import_error_message_async = import_error_message_signal;

                                    spawn(async move {
                                        use crate::cd::CdDrive;

                                        let drive = CdDrive {
                                            device_path: drive_path_clone.clone(),
                                            name: drive_path_clone.to_string_lossy().to_string(),
                                        };

                                        match drive.read_toc() {
                                            Ok(toc) => {
                                                // Store CD info for display
                                                cd_toc_info_async.set(Some((toc.disc_id.clone(), toc.first_track, toc.last_track)));

                                                // Look up by DiscID
                                                match lookup_by_discid(&toc.disc_id).await {
                                                    Ok((matches, _external_urls)) => {
                                                        is_looking_up_async.set(false);
                                                        if matches.is_empty() {
                                                            // No exact match, go to manual search
                                                            import_phase_async.set(crate::ui::import_context::ImportPhase::ManualSearch);
                                                            search_query_async.set(format!("DiscID: {}", toc.disc_id));
                                                        } else if matches.len() == 1 {
                                                            // Single exact match, convert to MatchCandidate and auto-confirm
                                                            let mb_release = matches[0].clone();
                                                            let candidate = MatchCandidate {
                                                                source: MatchSource::MusicBrainz(mb_release),
                                                                confidence: 100.0, // Exact DiscID match
                                                                match_reasons: vec!["Exact DiscID match".to_string()],
                                                            };
                                                            confirmed_candidate_async.set(Some(candidate));
                                                            import_phase_async.set(crate::ui::import_context::ImportPhase::Confirmation);
                                                        } else {
                                                            // Multiple matches, convert to MatchCandidates and show selection
                                                            let candidates: Vec<MatchCandidate> = matches.into_iter().map(|mb_release| {
                                                                MatchCandidate {
                                                                    source: MatchSource::MusicBrainz(mb_release),
                                                                    confidence: 100.0, // Exact DiscID match
                                                                    match_reasons: vec!["Exact DiscID match".to_string()],
                                                                }
                                                            }).collect();
                                                            exact_match_candidates_async.set(candidates);
                                                        }
                                                    }
                                                    Err(e) => {
                                                        is_looking_up_async.set(false);
                                                        import_error_message_async.set(Some(format!("Failed to look up DiscID: {}", e)));
                                                        import_phase_async.set(crate::ui::import_context::ImportPhase::ManualSearch);
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                is_looking_up_async.set(false);
                                                import_error_message_async.set(Some(format!("Failed to read CD TOC: {}", e)));
                                            }
                                        }
                                    });
                                }
                            },
                            on_error: {
                                let mut import_error_message = import_error_message;
                                move |e: String| {
                                    import_error_message.set(Some(e));
                                }
                            }
                        }
                    }
                }
            } else {
                div { class: "space-y-6",
                    // Show selected folder or torrent
                    div { class: "bg-white rounded-lg shadow p-6",
                        div { class: "mb-6 pb-4 border-b border-gray-200",
                            div { class: "flex items-start justify-between mb-3",
                                h3 { class: "text-sm font-semibold text-gray-700 uppercase tracking-wide",
                                    if torrent_source.read().is_some() {
                                        "Selected Torrent"
                                    } else if *selected_source.read() == ImportSource::Cd {
                                        "Selected CD"
                                    } else {
                                        "Selected Folder"
                                    }
                                }
                                button {
                                    class: "px-3 py-1 text-sm text-blue-600 hover:text-blue-800 hover:bg-blue-50 rounded-md transition-colors",
                                    onclick: on_change_folder,
                                    "Clear"
                                }
                            }
                            div { class: "inline-block px-4 py-2 bg-gray-100 hover:bg-gray-200 rounded-full border border-gray-300 transition-colors",
                                p {
                                    class: "text-sm text-gray-900 font-mono select-text cursor-text break-all",
                                    "{folder_path.read()}"
                                }
                            }
                            if *selected_source.read() == ImportSource::Cd {
                                if let Some((disc_id, first_track, last_track)) = cd_toc_info.read().as_ref() {
                                    div { class: "mt-4 p-4 bg-blue-50 border border-blue-200 rounded-lg",
                                        div { class: "space-y-2",
                                            div { class: "flex items-center",
                                                span { class: "text-sm font-medium text-gray-700 w-24", "DiscID:" }
                                                span { class: "text-sm text-gray-900 font-mono", "{disc_id}" }
                                            }
                                            div { class: "flex items-center",
                                                span { class: "text-sm font-medium text-gray-700 w-24", "Tracks:" }
                                                span { class: "text-sm text-gray-900",
                                                    "{last_track - first_track + 1} tracks ({first_track}-{last_track})"
                                                }
                                            }
                                        }
                                    }
                                } else if *is_looking_up.read() {
                                    div { class: "mt-4 p-4 bg-gray-50 border border-gray-200 rounded-lg text-center",
                                        p { class: "text-sm text-gray-600", "Reading CD table of contents..." }
                                    }
                                }
                            }
                        }

                        if *is_detecting.read() {
                            div { class: "text-center py-8",
                                p { class: "text-gray-600 mb-4", "Downloading metadata files (CUE/log)..." }
                                button {
                                    class: "px-4 py-2 bg-gray-200 hover:bg-gray-300 text-gray-800 rounded transition-colors",
                                    onclick: {
                                        let mut is_detecting = is_detecting;
                                        let mut search_query = search_query;
                                        let mut import_phase = import_phase;
                                        move |_| {
                                            is_detecting.set(false);
                                            // Use current search query (already set to torrent name) or folder path
                                            if search_query.read().is_empty() {
                                                let path = folder_path.read().clone();
                                                if let Some(name) = std::path::Path::new(&path).file_name() {
                                                    search_query.set(name.to_string_lossy().to_string());
                                                }
                                            }
                                            import_phase.set(crate::ui::import_context::ImportPhase::ManualSearch);
                                        }
                                    },
                                    "Skip and search manually"
                                }
                            }
                        } else if !folder_files.read().is_empty() {
                            div { class: "mt-4",
                                h4 { class: "text-sm font-semibold text-gray-700 uppercase tracking-wide mb-3", "Files" }
                                FileList {
                                    files: folder_files.read().clone(),
                                }
                            }
                        }
                    }

                    // Phase 2: Metadata Detection (handled in on_folder_select)

                    // Phase 3: Exact Lookup
                    if *import_phase.read() == crate::ui::import_context::ImportPhase::ExactLookup {
                        if *is_looking_up.read() {
                            div { class: "bg-white rounded-lg shadow p-6 text-center",
                                p { class: "text-gray-600", "Looking up release by DiscID..." }
                            }
                        } else if !exact_match_candidates.read().is_empty() {
                            div { class: "bg-white rounded-lg shadow p-6",
                                h3 { class: "text-lg font-semibold text-gray-900 mb-4", "Multiple Exact Matches Found" }
                                p { class: "text-sm text-gray-600 mb-4", "Select the correct release:" }
                                MatchList {
                                    candidates: exact_match_candidates.read().clone(),
                                    selected_index: selected_match_index.read().as_ref().copied(),
                                    on_select: on_exact_match_select,
                                }
                            }
                        }
                    }

                    // Phase 4: Manual Search
                    if *import_phase.read() == crate::ui::import_context::ImportPhase::ManualSearch {
                        if has_cue_files_for_manual && detected_metadata.read().is_none() && !*is_detecting.read() {
                            div { class: "bg-blue-50 border border-blue-200 rounded-lg p-4 mb-4",
                                div { class: "flex items-center justify-between",
                                    div { class: "flex-1",
                                        p { class: "text-sm text-blue-900 font-medium mb-1",
                                            "Metadata files detected"
                                        }
                                        p { class: "text-xs text-blue-700",
                                            "CUE/log files found in torrent. Download and detect metadata automatically?"
                                        }
                                    }
                                    button {
                                        class: "px-4 py-2 bg-blue-600 text-white text-sm rounded hover:bg-blue-700 transition-colors",
                                        onclick: {
                                            move |_| {
                                                let path = folder_path.read().clone();
                                                let mut is_detecting_for_async = is_detecting;
                                                let mut detected_metadata_for_async = detected_metadata;
                                                let mut is_looking_up_for_async = is_looking_up;
                                                let mut exact_match_candidates_for_async = exact_match_candidates;
                                                let mut search_query_for_async = search_query;
                                                let mut import_phase_for_async = import_phase;
                                                let mut confirmed_candidate_for_async = confirmed_candidate;
                                                let client_for_manual = import_context.torrent_client_default();

                                                is_detecting_for_async.set(true);

                                                spawn(async move {
                                                    use crate::musicbrainz::lookup_by_discid;
                                                    use crate::torrent::detect_metadata_from_torrent_file;

                                                    let result = detect_metadata_from_torrent_file(std::path::Path::new(&path), &client_for_manual).await;

                                                    if !*is_detecting_for_async.read() {
                                                        info!("Metadata detection was cancelled");
                                                        return;
                                                    }

                                                    match result {
                                                        Ok(Some(metadata)) => {
                                                            info!("Detected metadata from torrent: {:?}", metadata);
                                                            detected_metadata_for_async.set(Some(metadata.clone()));
                                                            is_detecting_for_async.set(false);

                                                            let mut init_search_query =
                                                                |metadata: &crate::import::FolderMetadata| {
                                                                    let mut query_parts = Vec::new();
                                                                    if let Some(ref artist) = metadata.artist {
                                                                        query_parts.push(artist.clone());
                                                                    }
                                                                    if let Some(ref album) = metadata.album {
                                                                        query_parts.push(album.clone());
                                                                    }
                                                                    search_query_for_async.set(query_parts.join(" "));
                                                                };

                                                            if let Some(ref mb_discid) = metadata.mb_discid {
                                                                is_looking_up_for_async.set(true);
                                                                info!("🎵 Found MB DiscID: {}, performing exact lookup", mb_discid);

                                                                match lookup_by_discid(mb_discid).await {
                                                                    Ok((releases, _external_urls)) => {
                                                                        if releases.is_empty() {
                                                                            info!("No exact matches found, proceeding to manual search");
                                                                            init_search_query(&metadata);
                                                                            import_phase_for_async.set(
                                                                                crate::ui::import_context::ImportPhase::ManualSearch,
                                                                            );
                                                                        } else if releases.len() == 1 {
                                                                            info!("✅ Single exact match found, auto-proceeding");
                                                                            let mb_release = releases[0].clone();
                                                                            let candidate = crate::import::MatchCandidate {
                                                                                source: crate::import::MatchSource::MusicBrainz(mb_release),
                                                                                confidence: 100.0,
                                                                                match_reasons: vec!["Exact DiscID match".to_string()],
                                                                            };
                                                                            confirmed_candidate_for_async.set(Some(candidate));
                                                                            import_phase_for_async.set(
                                                                                crate::ui::import_context::ImportPhase::Confirmation,
                                                                            );
                                                                        } else {
                                                                            info!("Found {} exact matches, showing for selection", releases.len());
                                                                            let candidates: Vec<crate::import::MatchCandidate> = releases
                                                                                .into_iter()
                                                                                .map(|mb_release| crate::import::MatchCandidate {
                                                                                    source: crate::import::MatchSource::MusicBrainz(mb_release),
                                                                                    confidence: 100.0,
                                                                                    match_reasons: vec!["Exact DiscID match".to_string()],
                                                                                })
                                                                                .collect();
                                                                            exact_match_candidates_for_async.set(candidates);
                                                                            import_phase_for_async.set(
                                                                                crate::ui::import_context::ImportPhase::ExactLookup,
                                                                            );
                                                                        }
                                                                        is_looking_up_for_async.set(false);
                                                                    }
                                                                    Err(e) => {
                                                                        info!("MB DiscID lookup failed: {}, proceeding to manual search", e);
                                                                        is_looking_up_for_async.set(false);
                                                                        init_search_query(&metadata);
                                                                        import_phase_for_async.set(
                                                                            crate::ui::import_context::ImportPhase::ManualSearch,
                                                                        );
                                                                    }
                                                                }
                                                            } else {
                                                                info!("No MB DiscID found, proceeding to manual search");
                                                                init_search_query(&metadata);
                                                                import_phase_for_async
                                                                    .set(crate::ui::import_context::ImportPhase::ManualSearch);
                                                            }
                                                        }
                                                        Ok(None) => {
                                                            info!("No metadata detected from torrent");
                                                            is_detecting_for_async.set(false);
                                                        }
                                                        Err(e) => {
                                                            warn!("Failed to detect metadata from torrent: {}", e);
                                                            is_detecting_for_async.set(false);
                                                        }
                                                    }
                                                });
                                            }
                                        },
                                        "Detect from CUE/log files"
                                    }
                                }
                            }
                        }

                        if *is_detecting.read() {
                            div { class: "bg-white rounded-lg shadow p-6 text-center mb-4",
                                p { class: "text-gray-600 mb-4", "Downloading and analyzing metadata files (CUE/log)..." }
                                button {
                                    class: "px-4 py-2 bg-gray-200 hover:bg-gray-300 text-gray-800 rounded transition-colors",
                                    onclick: {
                                        let mut is_detecting = is_detecting;
                                        move |_| {
                                            is_detecting.set(false);
                                        }
                                    },
                                    "Cancel"
                                }
                            }
                        }

                        ManualSearchPanel {
                            detected_metadata: detected_metadata,
                            on_match_select: on_manual_match_select,
                            on_confirm: {
                                let mut confirmed_candidate = confirmed_candidate;
                                let mut import_phase = import_phase;
                                move |candidate: MatchCandidate| {
                                    confirmed_candidate.set(Some(candidate.clone()));
                                    import_phase.set(crate::ui::import_context::ImportPhase::Confirmation);
                                }
                            },
                            selected_index: selected_match_index,
                        }
                    }

                    // Phase 5: Confirmation
                    if *import_phase.read() == crate::ui::import_context::ImportPhase::Confirmation {
                        if let Some(candidate) = confirmed_candidate.read().as_ref() {
                            div { class: "space-y-4",
                                div { class: "bg-blue-50 border-2 border-blue-500 rounded-lg p-6",
                                    div { class: "flex items-start justify-between mb-4",
                                        div { class: "flex-1",
                                            h3 { class: "text-lg font-semibold text-gray-900 mb-2",
                                                "Selected Release"
                                            }
                                            div { class: "text-sm text-gray-600 space-y-1",
                                                p { class: "text-lg font-medium text-gray-900", "{candidate.title()}" }
                                                if let Some(ref year) = candidate.year() {
                                                    p { "Year: {year}" }
                                                }
                                                {
                                                    let (format_text, country_text, label_text) = match &candidate.source {
                                                        MatchSource::MusicBrainz(release) => (
                                                            release.format.as_ref().map(|f| format!("Format: {}", f)),
                                                            release.country.as_ref().map(|c| format!("Country: {}", c)),
                                                            release.label.as_ref().map(|l| format!("Label: {}", l)),
                                                        ),
                                                        MatchSource::Discogs(_) => (None, None, None),
                                                    };
                                                    rsx! {
                                                        if let Some(ref fmt) = format_text {
                                                            p { "{fmt}" }
                                                        }
                                                        if let Some(ref country) = country_text {
                                                            p { "{country}" }
                                                        }
                                                        if let Some(ref label) = label_text {
                                                            p { "{label}" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                div { class: "flex justify-end gap-3",
                                    button {
                                        class: "px-6 py-2 bg-gray-600 text-white rounded hover:bg-gray-700",
                                        onclick: on_edit,
                                        "Edit"
                                    }
                                    button {
                                        class: "px-6 py-2 bg-green-600 text-white rounded hover:bg-green-700",
                                        onclick: on_confirm,
                                        "Import"
                                    }
                                }
                            }
                        }
                    }

                    // Error messages
                    if let Some(ref error) = import_error_message.read().as_ref() {
                        div { class: "bg-red-50 border border-red-200 rounded-lg p-4",
                            p {
                                class: "text-sm text-red-700 select-text break-words font-mono",
                                "Error: {error}"
                            }
                            {
                                let dup_id_opt = duplicate_album_id.read().clone();
                                if let Some(dup_id) = dup_id_opt {
                                    let dup_id_clone = dup_id.clone();
                                    rsx! {
                                        div { class: "mt-2",
                                            a {
                                                href: "#",
                                                class: "text-sm text-blue-600 hover:underline",
                                                onclick: move |_| {
                                                    navigator.push(Route::AlbumDetail {
                                                        album_id: dup_id_clone.clone(),
                                                        release_id: String::new(),
                                                    });
                                                },
                                                "View existing album"
                                            }
                                        }
                                    }
                                } else {
                                    rsx! { div {} }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
