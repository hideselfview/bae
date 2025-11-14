use super::file_list::{FileInfo, FileList};
use super::handlers::{handle_confirmation, handle_metadata_detection};
use super::inputs::FolderSelector;
use super::shared::{Confirmation, ErrorDisplay, ExactLookup, ManualSearch};
use crate::import::MatchCandidate;
use crate::library::{use_import_service, use_library_manager};
use crate::ui::import_context::{ImportContext, ImportPhase};
use dioxus::prelude::*;
use std::rc::Rc;

#[component]
pub fn FolderImport() -> Element {
    let import_context = use_context::<Rc<ImportContext>>();
    let library_manager = use_library_manager();
    let import_service = use_import_service();
    let navigator = use_navigator();

    // Get signals via getters (signals are Copy)
    let folder_path = import_context.folder_path();
    let detected_metadata = import_context.detected_metadata();
    let import_phase = import_context.import_phase();
    let exact_match_candidates = import_context.exact_match_candidates();
    let selected_match_index = import_context.selected_match_index();
    let confirmed_candidate = import_context.confirmed_candidate();
    let is_detecting = import_context.is_detecting();
    let is_looking_up = import_context.is_looking_up();
    let import_error_message = import_context.import_error_message();
    let duplicate_album_id = import_context.duplicate_album_id();
    let folder_files = import_context.folder_files();

    let on_folder_select = {
        let import_context_for_detect = import_context.clone();

        move |path: String| {
            import_context_for_detect.set_folder_path(path.clone());
            import_context_for_detect.set_detected_metadata(None);
            import_context_for_detect.set_exact_match_candidates(Vec::new());
            import_context_for_detect.set_selected_match_index(None);
            import_context_for_detect.set_confirmed_candidate(None);
            import_context_for_detect.set_import_error_message(None);
            import_context_for_detect.set_duplicate_album_id(None);
            import_context_for_detect.set_import_phase(ImportPhase::MetadataDetection);
            import_context_for_detect.set_is_detecting(true);

            // Read files from folder
            let folder_path_clone = path.clone();
            let import_context_for_files = import_context_for_detect.clone();
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
                import_context_for_files.set_folder_files(files);
            });

            let import_context_for_detect = import_context_for_detect.clone();
            let detected_metadata = import_context_for_detect.detected_metadata();
            let is_looking_up = import_context_for_detect.is_looking_up();
            let confirmed_candidate = import_context_for_detect.confirmed_candidate();
            let exact_match_candidates = import_context_for_detect.exact_match_candidates();
            let search_query = import_context_for_detect.search_query();
            let import_phase = import_context_for_detect.import_phase();

            spawn(async move {
                // Detect metadata
                let metadata_result = import_context_for_detect
                    .detect_folder_metadata(path.clone())
                    .await;

                match metadata_result {
                    Ok(metadata) => {
                        import_context_for_detect.set_is_detecting(false);
                        handle_metadata_detection(
                            Some(metadata),
                            path.clone(),
                            detected_metadata,
                            is_looking_up,
                            exact_match_candidates,
                            search_query,
                            import_phase,
                            confirmed_candidate,
                        )
                        .await;
                    }
                    Err(e) => {
                        import_context_for_detect.set_import_error_message(Some(e));
                        import_context_for_detect.set_is_detecting(false);
                        import_context_for_detect.set_import_phase(ImportPhase::FolderSelection);
                    }
                }
            });
        }
    };

    let on_confirm_from_manual = {
        let import_context_for_reset = import_context.clone();
        let library_manager = library_manager.clone();
        let import_service = import_service.clone();
        move |candidate: MatchCandidate| {
            let folder = folder_path.read().clone();
            let metadata = detected_metadata.read().clone();
            let import_service = import_service.clone();
            let duplicate_album_id = duplicate_album_id;
            let import_error_message = import_error_message;
            let import_context_for_reset = import_context_for_reset.clone();
            let library_manager = library_manager.clone();

            spawn(async move {
                handle_confirmation(
                    candidate,
                    folder,
                    metadata,
                    crate::ui::components::import::ImportSource::Folder,
                    None,
                    false,
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

    rsx! {
        div { class: "space-y-6",
            // Phase 1: Folder Selection
            if *import_phase.read() == ImportPhase::FolderSelection {
                div { class: "bg-white rounded-lg shadow p-6",
                    FolderSelector {
                        on_select: on_folder_select,
                        on_error: {
                            let import_context = import_context.clone();
                            move |e: String| {
                                import_context.set_import_error_message(Some(e));
                            }
                        }
                    }
                }
            } else {
                div { class: "space-y-6",
                    // Show selected folder
                    div { class: "bg-white rounded-lg shadow p-6",
                        div { class: "mb-6 pb-4 border-b border-gray-200",
                            div { class: "flex items-start justify-between mb-3",
                                h3 { class: "text-sm font-semibold text-gray-700 uppercase tracking-wide",
                                    "Selected Folder"
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
                        }

                        if *is_detecting.read() {
                            div { class: "text-center py-8",
                                p { class: "text-gray-600 mb-4", "Detecting metadata..." }
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

                    // Phase 2: Exact Lookup
                    if *import_phase.read() == ImportPhase::ExactLookup {
                        ExactLookup {
                            is_looking_up: is_looking_up,
                            exact_match_candidates: exact_match_candidates,
                            selected_match_index: selected_match_index,
                            on_select: {
                                let import_context = import_context.clone();
                                move |index| {
                                    import_context.set_selected_match_index(Some(index));
                                    if let Some(candidate) = import_context.exact_match_candidates().read().get(index) {
                                        import_context.set_confirmed_candidate(Some(candidate.clone()));
                                        import_context.set_import_phase(ImportPhase::Confirmation);
                                    }
                                }
                            },
                        }
                    }

                    // Phase 3: Manual Search
                    if *import_phase.read() == ImportPhase::ManualSearch {
                        ManualSearch {
                            detected_metadata: detected_metadata,
                            selected_match_index: selected_match_index,
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
                    if *import_phase.read() == ImportPhase::Confirmation {
                        Confirmation {
                            confirmed_candidate: confirmed_candidate,
                            on_edit: {
                                let import_context = import_context.clone();
                                move |_| {
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
                                move || {
                                    if let Some(candidate) = confirmed_candidate.read().as_ref().cloned() {
                                        on_confirm_from_manual_local(candidate);
                                    }
                                }
                            },
                        }
                    }

                    // Error messages
                    ErrorDisplay {
                        error_message: import_error_message,
                        duplicate_album_id: duplicate_album_id,
                    }
                }
            }
        }
    }
}
