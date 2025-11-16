use super::file_list::FileList;
use super::inputs::TorrentInput;
use super::shared::{Confirmation, ErrorDisplay, ExactLookup, ManualSearch};
use crate::import::MatchCandidate;
use crate::ui::components::import::ImportSource;
use crate::ui::import_context::{ImportContext, ImportPhase};
use dioxus::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;
use tracing::{info, warn};

#[component]
pub fn TorrentImport() -> Element {
    let navigator = use_navigator();
    let import_context = use_context::<Rc<ImportContext>>();

    let on_torrent_file_select = {
        let import_context = import_context.clone();
        move |(path, seed_flag): (PathBuf, bool)| {
            let import_context = import_context.clone();
            spawn(async move {
                if let Err(e) = import_context
                    .load_torrent_for_import(path, seed_flag)
                    .await
                {
                    warn!("Failed to load torrent: {}", e);
                }
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
        move |candidate: MatchCandidate| {
            let import_context = import_context.clone();
            let navigator = navigator;
            spawn(async move {
                if let Err(e) = import_context
                    .confirm_and_start_import(candidate, ImportSource::Torrent, navigator)
                    .await
                {
                    warn!("Failed to confirm and start import: {}", e);
                }
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
            let seed_flag = *import_context.seed_after_download().read();
            let import_context = import_context.clone();
            spawn(async move {
                let path_buf = PathBuf::from(&path);
                if let Err(e) = import_context
                    .load_torrent_for_import(path_buf, seed_flag)
                    .await
                {
                    warn!("Failed to load torrent for metadata detection: {}", e);
                }
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
                                    import_context.select_exact_match(index);
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
                                    import_context.confirm_candidate(candidate);
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
                                    import_context.reject_confirmation();
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
