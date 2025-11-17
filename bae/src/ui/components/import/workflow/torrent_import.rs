use super::file_list::FileList;
use super::inputs::TorrentInput;
use super::shared::{Confirmation, ErrorDisplay, ExactLookup, ManualSearch, SelectedSource};
use crate::import::MatchCandidate;
use crate::torrent::ffi::TorrentInfo;
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
        EventHandler::new(move |()| {
            import_context.reset();
        })
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
                    // Show selected torrent with all info
                    SelectedSource {
                        title: "Selected Torrent".to_string(),
                        path: import_context.folder_path(),
                        on_clear: on_change_folder,
                        children: Some(rsx! {
                            TorrentInfoDisplay {
                                info: import_context.torrent_info(),
                            }
                        }),
                    }

                    // OLD CODE - Commented out
                    /*
                    // Show torrent status if we have an info_hash
                    if let Some(info_hash) = import_context.torrent_info_hash().read().as_ref() {
                        TorrentStatus {
                            info_hash: info_hash.clone(),
                            on_skip: if *import_context.is_detecting().read() {
                                Some({
                                    let import_context = import_context.clone();
                                    EventHandler::new(move |()| {
                                        import_context.skip_metadata_detection();
                                    })
                                })
                            } else {
                                None
                            },
                        }
                    }

                    // Show file list if available
                    if !import_context.folder_files().read().is_empty() {
                        div { class: "bg-white rounded-lg shadow p-6",
                            h4 { class: "text-sm font-semibold text-gray-700 uppercase tracking-wide mb-3", "Files" }
                            FileList {
                                files: import_context.folder_files().read().clone(),
                            }
                        }
                    }
                    */

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
                            MetadataDetectionPrompt {
                                on_detect: {
                                    let import_context = import_context.clone();
                                    EventHandler::new(move |()| {
                                        let import_context = import_context.clone();
                                        spawn(async move {
                                            if let Err(e) = import_context
                                                .retry_torrent_metadata_detection()
                                                .await
                                            {
                                                warn!("Failed to retry metadata detection: {}", e);
                                            }
                                        });
                                    })
                                },
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

#[component]
fn TorrentInfoDisplay(info: ReadSignal<Option<TorrentInfo>>) -> Element {
    use crate::ui::components::import::FileInfo;

    let torrent_info_opt = info.read();
    let Some(torrent_info) = torrent_info_opt.as_ref() else {
        return rsx! {
            div { "No torrent info available" }
        };
    };

    // Format file size
    let format_size = |bytes: i64| -> String {
        if bytes < 1024 {
            format!("{} B", bytes)
        } else if bytes < 1024 * 1024 {
            format!("{:.2} KB", bytes as f64 / 1024.0)
        } else if bytes < 1024 * 1024 * 1024 {
            format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    };

    // Format creation date
    let format_date = |timestamp: i64| -> String {
        if timestamp == 0 {
            "Not available".to_string()
        } else {
            use chrono::TimeZone;
            if let Some(dt) = chrono::Utc.timestamp_opt(timestamp, 0).single() {
                dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
            } else {
                "Invalid date".to_string()
            }
        }
    };

    // Convert files to FileInfo format
    let mut files: Vec<FileInfo> = torrent_info
        .files
        .iter()
        .map(|tf| {
            let path_buf = std::path::PathBuf::from(&tf.path);
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

    rsx! {
        div { class: "mt-4 space-y-4",
            // Basic Info
            div { class: "grid grid-cols-2 gap-4",
                div {
                    h4 { class: "text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1", "Name" }
                    p { class: "text-sm text-gray-900", {torrent_info.name.clone()} }
                }
                div {
                    h4 { class: "text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1", "Total Size" }
                    p { class: "text-sm text-gray-900", {format_size(torrent_info.total_size)} }
                }
                div {
                    h4 { class: "text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1", "Piece Length" }
                    p { class: "text-sm text-gray-900", {format_size(torrent_info.piece_length as i64)} }
                }
                div {
                    h4 { class: "text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1", "Number of Pieces" }
                    p { class: "text-sm text-gray-900", {torrent_info.num_pieces.to_string()} }
                }
                div {
                    h4 { class: "text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1", "Private" }
                    p { class: "text-sm text-gray-900", if torrent_info.is_private { "Yes" } else { "No" } }
                }
            }

            // Comment
            if !torrent_info.comment.is_empty() {
                div {
                    h4 { class: "text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1", "Comment" }
                    p { class: "text-sm text-gray-900", {torrent_info.comment.clone()} }
                }
            }

            // Creator
            if !torrent_info.creator.is_empty() {
                div {
                    h4 { class: "text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1", "Created By" }
                    p { class: "text-sm text-gray-900", {torrent_info.creator.clone()} }
                }
            }

            // Creation Date
            if torrent_info.creation_date != 0 {
                div {
                    h4 { class: "text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1", "Creation Date" }
                    p { class: "text-sm text-gray-900", {format_date(torrent_info.creation_date)} }
                }
            }

            // Trackers
            if !torrent_info.trackers.is_empty() {
                div {
                    h4 { class: "text-xs font-semibold text-gray-500 uppercase tracking-wide mb-2", "Trackers" }
                    ul { class: "space-y-1",
                        for tracker in torrent_info.trackers.iter() {
                            li { class: "text-sm text-gray-700 font-mono bg-gray-50 px-2 py-1 rounded",
                                {tracker.clone()}
                            }
                        }
                    }
                }
            }

            // Files
            if !files.is_empty() {
                div {
                    h4 { class: "text-xs font-semibold text-gray-500 uppercase tracking-wide mb-3", "Files" }
                    FileList {
                        files: files,
                    }
                }
            }
        }
    }
}

#[component]
fn MetadataDetectionPrompt(on_detect: EventHandler<()>) -> Element {
    rsx! {
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
                    onclick: move |_| on_detect.call(()),
                    "Detect from CUE/log files"
                }
            }
        }
    }
}
