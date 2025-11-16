use super::file_list::{FileInfo, FileList};
use super::handlers::{handle_confirmation, handle_metadata_detection};
use super::inputs::TorrentInput;
use super::shared::{Confirmation, ErrorDisplay, ExactLookup, ManualSearch};
use crate::import::TorrentSource::{self, File};
use crate::import::{MatchCandidate, TorrentFileMetadata, TorrentImportMetadata};
use crate::library::{use_import_service, use_library_manager};
use crate::ui::import_context::{ImportContext, ImportPhase};
use dioxus::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;
use tracing::{info, warn};

#[component]
pub fn TorrentImport() -> Element {
    let navigator = use_navigator();
    let import_context = use_context::<Rc<ImportContext>>();
    let library_manager = use_library_manager();
    let import_service = use_import_service();

    let on_torrent_file_select = {
        let import_context = import_context.clone();

        move |(path, seed_flag): (PathBuf, bool)| {
            // Reset state for new torrent selection
            import_context.select_torrent_file(
                path.to_string_lossy().to_string(),
                File(path.clone()),
                seed_flag,
            );

            let import_context = import_context.clone();
            // let path = path.clone();

            spawn(async move {
                let torrent_manager = import_context.torrent_manager();
                // Add torrent file via torrent manager
                let mut torrent_handle_opt = match torrent_manager
                    .add_torrent(TorrentSource::File(path.clone()))
                    .await
                {
                    Ok(handle) => Some(handle),
                    Err(e) => {
                        import_context.set_import_error_message(Some(format!(
                            "Failed to add torrent file: {}",
                            e
                        )));
                        import_context.set_is_detecting(false);
                        import_context.set_import_phase(ImportPhase::FolderSelection);
                        return;
                    }
                };

                // Wait for metadata (immediate for torrent files, but keeps code path consistent with magnet links)
                let torrent_handle = torrent_handle_opt.as_ref().unwrap();
                if let Err(e) = torrent_handle.wait_for_metadata().await {
                    let _ = torrent_manager
                        .remove_torrent(torrent_handle_opt.take().unwrap(), true)
                        .await;
                    import_context.set_import_error_message(Some(format!(
                        "Failed to get torrent metadata: {}",
                        e
                    )));
                    import_context.set_is_detecting(false);
                    import_context.set_import_phase(ImportPhase::FolderSelection);
                    return;
                }

                // Get torrent name
                let torrent_name = match torrent_handle.name().await {
                    Ok(name) => name,
                    Err(e) => {
                        let _ = torrent_manager
                            .remove_torrent(torrent_handle_opt.take().unwrap(), true)
                            .await;
                        import_context.set_import_error_message(Some(format!(
                            "Failed to get torrent name: {}",
                            e
                        )));
                        import_context.set_is_detecting(false);
                        import_context.set_import_phase(ImportPhase::FolderSelection);
                        return;
                    }
                };

                // Get file list from torrent
                let torrent_files = match torrent_handle.get_file_list().await {
                    Ok(files) => files,
                    Err(e) => {
                        let _ = torrent_manager
                            .remove_torrent(torrent_handle_opt.take().unwrap(), true)
                            .await;
                        import_context.set_import_error_message(Some(format!(
                            "Failed to get torrent file list: {}",
                            e
                        )));
                        import_context.set_is_detecting(false);
                        import_context.set_import_phase(ImportPhase::FolderSelection);
                        return;
                    }
                };

                // Extract comprehensive torrent metadata
                let info_hash = torrent_handle.info_hash().await;
                let total_size = match torrent_handle.total_size().await {
                    Ok(size) => size,
                    Err(e) => {
                        let _ = torrent_manager
                            .remove_torrent(torrent_handle_opt.take().unwrap(), true)
                            .await;
                        import_context.set_import_error_message(Some(format!(
                            "Failed to get torrent size: {}",
                            e
                        )));
                        import_context.set_is_detecting(false);
                        import_context.set_import_phase(ImportPhase::FolderSelection);
                        return;
                    }
                };
                let piece_length = match torrent_handle.piece_length().await {
                    Ok(length) => length,
                    Err(e) => {
                        let _ = torrent_manager
                            .remove_torrent(torrent_handle_opt.take().unwrap(), true)
                            .await;
                        import_context.set_import_error_message(Some(format!(
                            "Failed to get piece length: {}",
                            e
                        )));
                        import_context.set_is_detecting(false);
                        import_context.set_import_phase(ImportPhase::FolderSelection);
                        return;
                    }
                };
                let num_pieces = match torrent_handle.num_pieces().await {
                    Ok(pieces) => pieces,
                    Err(e) => {
                        let _ = torrent_manager
                            .remove_torrent(torrent_handle_opt.take().unwrap(), true)
                            .await;
                        import_context.set_import_error_message(Some(format!(
                            "Failed to get piece count: {}",
                            e
                        )));
                        import_context.set_is_detecting(false);
                        import_context.set_import_phase(ImportPhase::FolderSelection);
                        return;
                    }
                };

                // Convert file list to metadata format
                let file_list: Vec<TorrentFileMetadata> = torrent_files
                    .iter()
                    .map(|tf| TorrentFileMetadata {
                        path: tf.path.clone(),
                        size: tf.size,
                    })
                    .collect();

                // Create and store torrent metadata
                let torrent_metadata = TorrentImportMetadata {
                    info_hash,
                    magnet_link: None,
                    torrent_name: torrent_name.clone(),
                    total_size_bytes: total_size,
                    piece_length,
                    num_pieces,
                    seed_after_download: seed_flag,
                    file_list,
                };

                import_context.set_torrent_metadata(Some(torrent_metadata));

                // Convert torrent files to FileInfo format for UI display
                let mut files: Vec<FileInfo> = torrent_files
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

                import_context.set_folder_files(files);

                info!(
                    "Torrent loaded: {} ({} files)",
                    torrent_name,
                    import_context.folder_files().read().len()
                );

                // Extract signals for metadata detection
                let detected_metadata_for_async = import_context.detected_metadata();
                let is_looking_up_for_async = import_context.is_looking_up();
                let exact_match_candidates_for_async = import_context.exact_match_candidates();
                let confirmed_candidate_for_async = import_context.confirmed_candidate();
                let import_phase_for_async = import_context.import_phase();
                let search_query_for_async = import_context.search_query();

                // Set detecting state to show UI feedback
                import_context.set_is_detecting(true);

                // Detect metadata from CUE/log files
                use crate::torrent::detect_metadata_from_torrent_file;
                let metadata = match detect_metadata_from_torrent_file(&torrent_handle).await {
                    Ok(metadata) => metadata,
                    Err(e) => {
                        warn!("Failed to detect metadata from torrent: {}", e);
                        None
                    }
                };

                // Check if detection was cancelled before processing results
                if !*import_context.is_detecting().read() {
                    info!("Metadata detection was cancelled, ignoring results");
                    let _ = torrent_manager
                        .remove_torrent(torrent_handle_opt.take().unwrap(), true)
                        .await;
                    return;
                }

                // Process detection result (linearized, no nested spawn)
                handle_metadata_detection(
                    metadata,
                    torrent_name.clone(),
                    detected_metadata_for_async,
                    is_looking_up_for_async,
                    exact_match_candidates_for_async,
                    search_query_for_async,
                    import_phase_for_async,
                    confirmed_candidate_for_async,
                )
                .await;

                // Remove torrent from session after metadata detection is complete
                // Delete files since they're temporary and only used for metadata detection
                // Note: We keep the torrent alive - ImportService will use it for download
                // The torrent will be removed by ImportService after import completes

                // Mark detection as complete
                import_context.set_is_detecting(false);
            });
        }
    };

    let on_magnet_link = move |(magnet, seed_after_download): (String, bool)| {
        // TODO: Handle magnet link
        let _ = (magnet, seed_after_download); // Placeholder until implementation
        info!("Magnet link selection not yet implemented");
    };

    let on_torrent_error = {
        let import_context = import_context.clone();
        move |error: String| {
            import_context.set_import_error_message(Some(error));
        }
    };

    let on_confirm_from_manual = {
        let import_context = import_context.clone();
        let library_manager = library_manager.clone();
        let import_service = import_service.clone();
        move |candidate: MatchCandidate| {
            let folder = import_context.folder_path().read().clone();
            let metadata = import_context.detected_metadata().read().clone();
            let torrent_source_opt = import_context.torrent_source().read().clone();
            let seed_flag = *import_context.seed_after_download().read();
            let import_service = import_service.clone();
            let duplicate_album_id = import_context.duplicate_album_id();
            let import_error_message = import_context.import_error_message();
            let import_context_for_reset = import_context.clone();
            let library_manager = library_manager.clone();

            spawn(async move {
                handle_confirmation(
                    candidate,
                    folder,
                    metadata,
                    crate::ui::components::import::ImportSource::Torrent,
                    torrent_source_opt,
                    seed_flag,
                    import_context_for_reset,
                    library_manager,
                    import_service,
                    navigator,
                    duplicate_album_id,
                    import_error_message,
                )
                .await;
            });
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
        let folder_files = import_context.folder_files();
        let files = folder_files.read();
        let result = files
            .iter()
            .any(|f| f.format.to_lowercase() == "cue" || f.format.to_lowercase() == "log");
        drop(files);
        result
    };

    let on_detect_from_cue_log = {
        let import_context = import_context.clone();
        move |_| {
            let path = import_context.folder_path().read().clone();
            let detected_metadata_for_async = import_context.detected_metadata();
            let is_looking_up_for_async = import_context.is_looking_up();
            let exact_match_candidates_for_async = import_context.exact_match_candidates();
            let search_query_for_async = import_context.search_query();
            let import_phase_for_async = import_context.import_phase();
            let confirmed_candidate_for_async = import_context.confirmed_candidate();
            let torrent_manager_for_manual = import_context.torrent_manager();
            let import_context_for_async = import_context.clone();

            import_context.set_is_detecting(true);

            // Extract torrent name from path for fallback
            let path_buf = PathBuf::from(&path);
            let torrent_name = path_buf
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            spawn(async move {
                // Add torrent and get handle for metadata detection
                let mut torrent_handle_opt_manual = match torrent_manager_for_manual
                    .add_torrent(TorrentSource::File(path_buf.clone()))
                    .await
                {
                    Ok(handle) => Some(handle),
                    Err(e) => {
                        warn!("Failed to add torrent file: {}", e);
                        import_context_for_async.set_is_detecting(false);
                        return;
                    }
                };

                // Wait for metadata
                let torrent_handle_manual = torrent_handle_opt_manual.as_ref().unwrap();
                if let Err(e) = torrent_handle_manual.wait_for_metadata().await {
                    let _ = torrent_manager_for_manual
                        .remove_torrent(torrent_handle_opt_manual.take().unwrap(), true)
                        .await;
                    warn!("Failed to get torrent metadata: {}", e);
                    import_context_for_async.set_is_detecting(false);
                    return;
                }

                // Detect metadata from CUE/log files
                use crate::torrent::detect_metadata_from_torrent_file;
                let metadata = match detect_metadata_from_torrent_file(torrent_handle_manual).await
                {
                    Ok(metadata) => metadata,
                    Err(e) => {
                        warn!("Failed to detect metadata from torrent: {}", e);
                        None
                    }
                };

                // Check if detection was cancelled before processing results
                if !*import_context_for_async.is_detecting().read() {
                    info!("Metadata detection was cancelled, ignoring results");
                    let _ = torrent_manager_for_manual
                        .remove_torrent(torrent_handle_opt_manual.take().unwrap(), true)
                        .await;
                    return;
                }

                // Process detection result
                handle_metadata_detection(
                    metadata,
                    torrent_name,
                    detected_metadata_for_async,
                    is_looking_up_for_async,
                    exact_match_candidates_for_async,
                    search_query_for_async,
                    import_phase_for_async,
                    confirmed_candidate_for_async,
                )
                .await;

                // Remove torrent from session after metadata detection is complete
                // Delete files since they're temporary and only used for metadata detection
                // Note: We keep the torrent alive - ImportService will use it for download
                // The torrent will be removed by ImportService after import completes

                // Mark detection as complete
                import_context_for_async.set_is_detecting(false);
            });
        }
    };

    rsx! {
        div { class: "space-y-6",
            // Phase 1: Torrent Selection
            if *import_context.import_phase().read() == ImportPhase::FolderSelection {
                div { class: "bg-white rounded-lg shadow p-6",
                    TorrentInput {
                        on_file_select: on_torrent_file_select,
                        on_magnet_link: on_magnet_link,
                        on_error: on_torrent_error,
                    }
                }
            } else {
                div { class: "space-y-6",
                    // Show selected torrent
                    div { class: "bg-white rounded-lg shadow p-6",
                        div { class: "mb-6 pb-4 border-b border-gray-200",
                            div { class: "flex items-start justify-between mb-3",
                                h3 { class: "text-sm font-semibold text-gray-700 uppercase tracking-wide",
                                    "Selected Torrent"
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
                                    "{import_context.folder_path().read()}"
                                }
                            }
                        }

                        if *import_context.is_detecting().read() {
                            div { class: "text-center py-8",
                                p { class: "text-gray-600 mb-4", "Downloading metadata files (CUE/log)..." }
                                button {
                                    class: "px-4 py-2 bg-gray-200 hover:bg-gray-300 text-gray-800 rounded transition-colors",
                                    onclick: {
                                        let import_context = import_context.clone();
                                        move |_| {
                                            import_context.set_is_detecting(false);
                                            // Use current search query (already set to torrent name) or folder path
                                            if import_context.search_query().read().is_empty() {
                                                let path = import_context.folder_path().read().clone();
                                                if let Some(name) = std::path::Path::new(&path).file_name() {
                                                    import_context.set_search_query(name.to_string_lossy().to_string());
                                                }
                                            }
                                            import_context.set_import_phase(ImportPhase::ManualSearch);
                                        }
                                    },
                                    "Skip and search manually"
                                }
                            }
                        } else if !import_context.folder_files().read().is_empty() {
                            div { class: "mt-4",
                                h4 { class: "text-sm font-semibold text-gray-700 uppercase tracking-wide mb-3", "Files" }
                                FileList {
                                    files: import_context.folder_files().read().clone(),
                                }
                            }
                        }
                    }

                    // Phase 2: Exact Lookup
                    if *import_context.import_phase().read() == ImportPhase::ExactLookup {
                        ExactLookup {
                            is_looking_up: import_context.is_looking_up(),
                            exact_match_candidates: import_context.exact_match_candidates(),
                            selected_match_index: import_context.selected_match_index(),
                            on_select: {
                                let import_context = import_context.clone();
                                move |index| {
                                    import_context.set_selected_match_index(Some(index));
                                    let candidate_opt = import_context.exact_match_candidates().read().get(index).cloned();
                                    if let Some(candidate) = candidate_opt {
                                        import_context.set_confirmed_candidate(Some(candidate));
                                        import_context.set_import_phase(ImportPhase::Confirmation);
                                    }
                                }
                            },
                        }
                    }

                    // Phase 3: Manual Search
                    if *import_context.import_phase().read() == ImportPhase::ManualSearch {
                        if has_cue_files_for_manual && import_context.detected_metadata().read().is_none() && !*import_context.is_detecting().read() {
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
                                        onclick: on_detect_from_cue_log,
                                        "Detect from CUE/log files"
                                    }
                                }
                            }
                        }

                        if *import_context.is_detecting().read() {
                            div { class: "bg-white rounded-lg shadow p-6 text-center mb-4",
                                p { class: "text-gray-600 mb-4", "Downloading and analyzing metadata files (CUE/log)..." }
                                button {
                                    class: "px-4 py-2 bg-gray-200 hover:bg-gray-300 text-gray-800 rounded transition-colors",
                                    onclick: {
                                        let import_context = import_context.clone();
                                        move |_| {
                                            import_context.set_is_detecting(false);
                                        }
                                    },
                                    "Cancel"
                                }
                            }
                        }

                        ManualSearch {
                            detected_metadata: import_context.detected_metadata(),
                            selected_match_index: import_context.selected_match_index(),
                            on_match_select: {
                                let import_context = import_context.clone();
                                move |index| {
                                    import_context.set_selected_match_index(Some(index));
                                }
                            },
                            on_confirm: {
                                let import_context = import_context.clone();
                                move |candidate: MatchCandidate| {
                                    import_context.set_confirmed_candidate(Some(candidate.clone()));
                                    import_context.set_import_phase(ImportPhase::Confirmation);
                                }
                            },
                        }
                    }

                    // Phase 4: Confirmation
                    if *import_context.import_phase().read() == ImportPhase::Confirmation {
                        Confirmation {
                            confirmed_candidate: import_context.confirmed_candidate(),
                            on_edit: {
                                let import_context = import_context.clone();
                                move || {
                                    import_context.set_confirmed_candidate(None);
                                    import_context.set_selected_match_index(None);
                                    if !import_context.exact_match_candidates().read().is_empty() {
                                        import_context.set_import_phase(ImportPhase::ExactLookup);
                                    } else {
                                        // Initialize search query from detected metadata when transitioning to manual search
                                        if let Some(metadata) = import_context.detected_metadata().read().as_ref() {
                                            let mut query_parts = Vec::new();
                                            if let Some(ref artist) = metadata.artist {
                                                query_parts.push(artist.clone());
                                            }
                                            if let Some(ref album) = metadata.album {
                                                query_parts.push(album.clone());
                                            }
                                            if !query_parts.is_empty() {
                                                import_context.set_search_query(query_parts.join(" "));
                                            }
                                        }
                                        import_context.set_import_phase(ImportPhase::ManualSearch);
                                    }
                                }
                            },
                            on_confirm: {
                                let on_confirm_from_manual_local = on_confirm_from_manual;
                                let import_context = import_context.clone();
                                move || {
                                    if let Some(candidate) = import_context.confirmed_candidate().read().as_ref().cloned() {
                                        on_confirm_from_manual_local(candidate);
                                    }
                                }
                            },
                        }
                    }

                    // Error messages
                    ErrorDisplay {
                        error_message: import_context.import_error_message(),
                        duplicate_album_id: import_context.duplicate_album_id(),
                    }
                }
            }
        }
    }
}
