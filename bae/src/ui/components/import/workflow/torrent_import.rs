use super::file_list::{FileInfo, FileList};
use super::handlers::{handle_confirmation, handle_metadata_detection};
use super::inputs::TorrentInput;
use super::shared::{Confirmation, ErrorDisplay, ExactLookup, ManualSearch};
use crate::import::MatchCandidate;
use crate::library::{use_import_service, use_library_manager};
use crate::ui::import_context::{ImportContext, ImportPhase};
use dioxus::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;
use tracing::{info, warn};

#[component]
pub fn TorrentImport() -> Element {
    let import_context = use_context::<Rc<ImportContext>>();
    let library_manager = use_library_manager();
    let import_service = use_import_service();
    let navigator = use_navigator();

    let on_torrent_file_select = {
        let import_context = import_context.clone();
        move |(path, seed_flag): (PathBuf, bool)| {
            // Extract signals from cloned context (signals are Copy)
            let mut torrent_source = import_context.torrent_source;
            let mut seed_after_download = import_context.seed_after_download;
            let mut folder_path = import_context.folder_path;
            let mut detected_metadata = import_context.detected_metadata;
            let mut exact_match_candidates = import_context.exact_match_candidates;
            let mut selected_match_index = import_context.selected_match_index;
            let mut confirmed_candidate = import_context.confirmed_candidate;
            let mut import_error_message = import_context.import_error_message;
            let mut duplicate_album_id = import_context.duplicate_album_id;
            let mut import_phase = import_context.import_phase;
            let mut is_detecting = import_context.is_detecting;

            // Store torrent source and seed flag
            torrent_source.set(Some(crate::import::TorrentSource::File(path.clone())));
            seed_after_download.set(seed_flag);

            // Reset state
            folder_path.set(path.to_string_lossy().to_string());
            detected_metadata.set(None);
            exact_match_candidates.set(Vec::new());
            selected_match_index.set(None);
            confirmed_candidate.set(None);
            import_error_message.set(None);
            duplicate_album_id.set(None);
            import_phase.set(ImportPhase::MetadataDetection);
            is_detecting.set(true);

            // Clone everything needed for spawn (to keep closure FnMut)
            let mut folder_files = import_context.folder_files;
            let import_context_for_async = import_context.clone();
            let client_for_torrent = import_context.torrent_client_default();
            let path = path.clone();

            spawn(async move {
                // Add torrent file using shared client
                let torrent_handle = match client_for_torrent.add_torrent_file(&path).await {
                    Ok(handle) => handle,
                    Err(e) => {
                        import_error_message
                            .set(Some(format!("Failed to add torrent file: {}", e)));
                        is_detecting.set(false);
                        import_phase.set(ImportPhase::FolderSelection);
                        return;
                    }
                };

                // Wait for metadata (immediate for torrent files, but keeps code path consistent with magnet links)
                if let Err(e) = torrent_handle.wait_for_metadata().await {
                    import_error_message
                        .set(Some(format!("Failed to get torrent metadata: {}", e)));
                    is_detecting.set(false);
                    import_phase.set(ImportPhase::FolderSelection);
                    return;
                }

                // Get torrent name
                let torrent_name = match torrent_handle.name().await {
                    Ok(name) => name,
                    Err(e) => {
                        import_error_message
                            .set(Some(format!("Failed to get torrent name: {}", e)));
                        is_detecting.set(false);
                        import_phase.set(ImportPhase::FolderSelection);
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
                        import_phase.set(ImportPhase::FolderSelection);
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

                // Try to detect metadata from CUE/log files
                let mut is_detecting_for_async = is_detecting;

                // Extract signals for metadata detection
                let detected_metadata_for_async = import_context_for_async.detected_metadata;
                let is_looking_up_for_async = import_context_for_async.is_looking_up;
                let exact_match_candidates_for_async =
                    import_context_for_async.exact_match_candidates;
                let confirmed_candidate_for_async = import_context_for_async.confirmed_candidate;
                let import_phase_for_async = import_context_for_async.import_phase;
                let search_query_for_async = import_context_for_async.search_query;

                // Set detecting state to show UI feedback
                is_detecting_for_async.set(true);

                // Detect metadata from CUE/log files
                use crate::torrent::detect_metadata_from_torrent_file;
                let client_for_metadata = import_context_for_async.torrent_client_default();
                let metadata =
                    match detect_metadata_from_torrent_file(&path, &client_for_metadata).await {
                        Ok(metadata) => metadata,
                        Err(e) => {
                            warn!("Failed to detect metadata from torrent: {}", e);
                            None
                        }
                    };

                // Check if detection was cancelled before processing results
                if !*is_detecting_for_async.read() {
                    info!("Metadata detection was cancelled, ignoring results");
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

                // Mark detection as complete
                is_detecting_for_async.set(false);
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
            let mut import_error_message = import_context.import_error_message;
            import_error_message.set(Some(error));
        }
    };

    let on_confirm_from_manual = {
        let import_context = import_context.clone();
        let library_manager = library_manager.clone();
        let import_service = import_service.clone();
        move |candidate: MatchCandidate| {
            let folder = import_context.folder_path.read().clone();
            let metadata = import_context.detected_metadata.read().clone();
            let torrent_source_opt = import_context.torrent_source.read().clone();
            let seed_flag = *import_context.seed_after_download.read();
            let import_service = import_service.clone();
            let duplicate_album_id = import_context.duplicate_album_id;
            let import_error_message = import_context.import_error_message;
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
        let files = import_context.folder_files.read();
        let result = files
            .iter()
            .any(|f| f.format.to_lowercase() == "cue" || f.format.to_lowercase() == "log");
        drop(files);
        result
    };

    rsx! {
        div { class: "space-y-6",
            // Phase 1: Torrent Selection
            if *import_context.import_phase.read() == ImportPhase::FolderSelection {
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
                                    "{import_context.folder_path.read()}"
                                }
                            }
                        }

                        if *import_context.is_detecting.read() {
                            div { class: "text-center py-8",
                                p { class: "text-gray-600 mb-4", "Downloading metadata files (CUE/log)..." }
                                button {
                                    class: "px-4 py-2 bg-gray-200 hover:bg-gray-300 text-gray-800 rounded transition-colors",
                                    onclick: {
                                        let import_context = import_context.clone();
                                        move |_| {
                                            let mut is_detecting = import_context.is_detecting;
                                            let mut search_query = import_context.search_query;
                                            let mut import_phase = import_context.import_phase;
                                            let folder_path = import_context.folder_path;
                                            is_detecting.set(false);
                                            // Use current search query (already set to torrent name) or folder path
                                            if search_query.read().is_empty() {
                                                let path = folder_path.read().clone();
                                                if let Some(name) = std::path::Path::new(&path).file_name() {
                                                    search_query.set(name.to_string_lossy().to_string());
                                                }
                                            }
                                            import_phase.set(ImportPhase::ManualSearch);
                                        }
                                    },
                                    "Skip and search manually"
                                }
                            }
                        } else if !import_context.folder_files.read().is_empty() {
                            div { class: "mt-4",
                                h4 { class: "text-sm font-semibold text-gray-700 uppercase tracking-wide mb-3", "Files" }
                                FileList {
                                    files: import_context.folder_files.read().clone(),
                                }
                            }
                        }
                    }

                    // Phase 2: Exact Lookup
                    if *import_context.import_phase.read() == ImportPhase::ExactLookup {
                        ExactLookup {
                            is_looking_up: import_context.is_looking_up,
                            exact_match_candidates: import_context.exact_match_candidates,
                            selected_match_index: import_context.selected_match_index,
                            on_select: {
                                let import_context = import_context.clone();
                                move |index| {
                                    let mut selected_match_index = import_context.selected_match_index;
                                    let mut confirmed_candidate = import_context.confirmed_candidate;
                                    let mut import_phase = import_context.import_phase;
                                    let exact_match_candidates = import_context.exact_match_candidates;
                                    selected_match_index.set(Some(index));
                                    let candidate_opt = exact_match_candidates.read().get(index).cloned();
                                    if let Some(candidate) = candidate_opt {
                                        confirmed_candidate.set(Some(candidate));
                                        import_phase.set(ImportPhase::Confirmation);
                                    }
                                }
                            },
                        }
                    }

                    // Phase 3: Manual Search
                    if *import_context.import_phase.read() == ImportPhase::ManualSearch {
                        if has_cue_files_for_manual && import_context.detected_metadata.read().is_none() && !*import_context.is_detecting.read() {
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
                                            let import_context = import_context.clone();
                                            move |_| {
                                                let path = import_context.folder_path.read().clone();
                                                let mut is_detecting_for_async = import_context.is_detecting;
                                                let detected_metadata_for_async = import_context.detected_metadata;
                                                let is_looking_up_for_async = import_context.is_looking_up;
                                                let exact_match_candidates_for_async = import_context.exact_match_candidates;
                                                let search_query_for_async = import_context.search_query;
                                                let import_phase_for_async = import_context.import_phase;
                                                let confirmed_candidate_for_async = import_context.confirmed_candidate;
                                                let client_for_manual = import_context.torrent_client_default();

                                                is_detecting_for_async.set(true);

                                                // Extract torrent name from path for fallback
                                                let path_buf = PathBuf::from(&path);
                                                let torrent_name = path_buf
                                                    .file_stem()
                                                    .and_then(|s| s.to_str())
                                                    .unwrap_or("unknown")
                                                    .to_string();

                                                spawn(async move {
                                                    // Detect metadata from CUE/log files
                                                    use crate::torrent::detect_metadata_from_torrent_file;
                                                    let metadata = match detect_metadata_from_torrent_file(&path_buf, &client_for_manual).await {
                                                        Ok(metadata) => metadata,
                                                        Err(e) => {
                                                            warn!("Failed to detect metadata from torrent: {}", e);
                                                            None
                                                        }
                                                    };

                                                    // Check if detection was cancelled before processing results
                                                    if !*is_detecting_for_async.read() {
                                                        info!("Metadata detection was cancelled, ignoring results");
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

                                                    // Mark detection as complete
                                                    is_detecting_for_async.set(false);
                                                });
                                            }
                                        },
                                        "Detect from CUE/log files"
                                    }
                                }
                            }
                        }

                        if *import_context.is_detecting.read() {
                            div { class: "bg-white rounded-lg shadow p-6 text-center mb-4",
                                p { class: "text-gray-600 mb-4", "Downloading and analyzing metadata files (CUE/log)..." }
                                button {
                                    class: "px-4 py-2 bg-gray-200 hover:bg-gray-300 text-gray-800 rounded transition-colors",
                                    onclick: {
                                        let import_context = import_context.clone();
                                        move |_| {
                                            let mut is_detecting = import_context.is_detecting;
                                            is_detecting.set(false);
                                        }
                                    },
                                    "Cancel"
                                }
                            }
                        }

                        ManualSearch {
                            detected_metadata: import_context.detected_metadata,
                            selected_match_index: import_context.selected_match_index,
                            on_match_select: {
                                let import_context = import_context.clone();
                                move |index| {
                                    let mut selected_match_index = import_context.selected_match_index;
                                    selected_match_index.set(Some(index));
                                }
                            },
                            on_confirm: {
                                let import_context = import_context.clone();
                                move |candidate: MatchCandidate| {
                                    let mut confirmed_candidate = import_context.confirmed_candidate;
                                    let mut import_phase = import_context.import_phase;
                                    confirmed_candidate.set(Some(candidate.clone()));
                                    import_phase.set(ImportPhase::Confirmation);
                                }
                            },
                        }
                    }

                    // Phase 4: Confirmation
                    if *import_context.import_phase.read() == ImportPhase::Confirmation {
                        Confirmation {
                            confirmed_candidate: import_context.confirmed_candidate,
                            on_edit: {
                                let import_context = import_context.clone();
                                move || {
                                    let mut confirmed_candidate = import_context.confirmed_candidate;
                                    let mut selected_match_index = import_context.selected_match_index;
                                    let mut import_phase = import_context.import_phase;
                                    let mut search_query = import_context.search_query;
                                    let exact_match_candidates = import_context.exact_match_candidates;
                                    let detected_metadata = import_context.detected_metadata;
                                    confirmed_candidate.set(None);
                                    selected_match_index.set(None);
                                    if !exact_match_candidates.read().is_empty() {
                                        import_phase.set(ImportPhase::ExactLookup);
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
                                        import_phase.set(ImportPhase::ManualSearch);
                                    }
                                }
                            },
                            on_confirm: {
                                let on_confirm_from_manual_local = on_confirm_from_manual;
                                let import_context = import_context.clone();
                                move || {
                                    if let Some(candidate) = import_context.confirmed_candidate.read().as_ref().cloned() {
                                        on_confirm_from_manual_local(candidate);
                                    }
                                }
                            },
                        }
                    }

                    // Error messages
                    ErrorDisplay {
                        error_message: import_context.import_error_message,
                        duplicate_album_id: import_context.duplicate_album_id,
                    }
                }
            }
        }
    }
}
