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
        div {
            // Phase 1: Torrent Selection
            if *import_context.import_phase().read() == ImportPhase::FolderSelection {
                TorrentInput {
                    on_file_select: on_torrent_file_select,
                    on_magnet_link: on_magnet_link,
                    on_error: on_torrent_error,
                    show_seed_checkbox: false,
                }
            } else {
                div { class: "space-y-6",
                    // Show selected torrent with all info
                    SelectedSource {
                        title: "Selected Torrent".to_string(),
                        path: import_context.folder_path(),
                        on_clear: on_change_folder,
                        children: Some(rsx! {
                            TorrentTrackerDisplay {
                                trackers: import_context.torrent_info().read().as_ref().map(|info| info.trackers.clone()).unwrap_or_default(),
                            }
                            TorrentInfoDisplay {
                                info: import_context.torrent_info(),
                            }
                            TorrentFilesDisplay {
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
fn TorrentTrackerDisplay(trackers: Vec<String>) -> Element {
    if trackers.is_empty() {
        return rsx! {
            div { "No trackers available" }
        };
    }

    // Fake data for tracker status
    struct TrackerStatus {
        url: String,
        announce_status: String,
        announce_progress: Option<(usize, usize)>, // (current, total) for announcing, None for connected
        peer_count: usize,
        seeders: usize,
        leechers: usize,
    }

    let tracker_statuses: Vec<TrackerStatus> = trackers
        .iter()
        .enumerate()
        .map(|(idx, url)| {
            // Generate fake data - use index to vary peer counts
            let peer_count = 15 + (idx * 7) % 35; // Varies between 15-49
            let seeders = peer_count / 4; // Roughly 25% seeders
            let leechers = peer_count - seeders;

            // Determine announce status and progress
            let (announce_status, announce_progress) = if url.contains("udp") {
                ("Connected".to_string(), None)
            } else {
                // Simulate announcing progress (0/2, 1/2, or 2/2 = Connected)
                let progress = idx % 3; // Cycle through 0, 1, 2
                if progress == 2 {
                    ("Connected".to_string(), None)
                } else {
                    ("Announcing".to_string(), Some((progress, 2)))
                }
            };

            TrackerStatus {
                url: url.clone(),
                announce_status,
                announce_progress,
                peer_count,
                seeders,
                leechers,
            }
        })
        .collect();

    let mut expanded = use_signal(|| false);

    let total_peers: usize = tracker_statuses.iter().map(|ts| ts.peer_count).sum();
    let total_seeders: usize = tracker_statuses.iter().map(|ts| ts.seeders).sum();
    let total_leechers: usize = tracker_statuses.iter().map(|ts| ts.leechers).sum();

    // Generate summary for collapsed state
    let mut connected_count = 0;
    let mut announcing_count = 0;
    let mut error_count = 0;

    for tracker in tracker_statuses.iter() {
        match tracker.announce_status.as_str() {
            "Connected" => connected_count += 1,
            "Announcing" => announcing_count += 1,
            _ => error_count += 1,
        }
    }

    let mut summary_parts = Vec::new();
    if connected_count > 0 {
        summary_parts.push(format!("{} connected", connected_count));
    }
    if announcing_count > 0 {
        summary_parts.push(format!("{} announcing", announcing_count));
    }
    if error_count > 0 {
        summary_parts.push(format!("{} error", error_count));
    }
    let summary = if summary_parts.is_empty() {
        "No status".to_string()
    } else {
        summary_parts.join(", ")
    };

    rsx! {
        div { class: "mb-4",
            button {
                class: "w-full flex items-center justify-between p-3 bg-gray-800 rounded border border-gray-700 hover:bg-gray-700 transition-colors",
                onclick: move |_| {
                    let current = *expanded.read();
                    expanded.set(!current);
                },
                div { class: "flex items-center gap-3",
                    span { class: "text-xs text-gray-400",
                        if *expanded.read() {
                            "▼"
                        } else {
                            "▶"
                        }
                    }
                    h3 { class: "text-sm font-semibold text-gray-300 uppercase tracking-wide", "Trackers" }
                    if !*expanded.read() {
                        span { class: "text-xs text-gray-400", {format!("({})", summary)} }
                    }
                }
                div { class: "flex items-center gap-4 text-sm text-gray-400",
                    div {
                        span { "Total peers: " }
                        span { class: "font-medium text-white", {total_peers.to_string()} }
                    }
                    div { class: "flex items-center gap-2",
                        span { class: "px-2 py-0.5 rounded bg-green-900/30 text-green-400 border border-green-700",
                            span { "Seeders: " }
                            span { class: "font-medium", {total_seeders.to_string()} }
                        }
                        span { class: "px-2 py-0.5 rounded bg-blue-900/30 text-blue-400 border border-blue-700",
                            span { "Leechers: " }
                            span { class: "font-medium", {total_leechers.to_string()} }
                        }
                    }
                }
            }

            if *expanded.read() {
                div { class: "mt-3 space-y-2",
                    for tracker in tracker_statuses.iter() {
                        TrackerItem {
                            url: tracker.url.clone(),
                            announce_status: tracker.announce_status.clone(),
                            announce_progress: tracker.announce_progress,
                            peer_count: tracker.peer_count,
                            seeders: tracker.seeders,
                            leechers: tracker.leechers,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TrackerItem(
    url: String,
    announce_status: String,
    announce_progress: Option<(usize, usize)>,
    peer_count: usize,
    seeders: usize,
    leechers: usize,
) -> Element {
    rsx! {
        div { class: "bg-gray-800 rounded border border-gray-700 p-3",
            div { class: "flex items-center justify-between",
                div { class: "flex-1 min-w-0",
                    p { class: "text-sm font-mono text-gray-300 truncate", {url.clone()} }
                }
                div { class: "flex items-center gap-4 ml-4",
                    span { class: "text-xs px-2 py-1 rounded",
                        class: if announce_status == "Connected" {
                            "bg-green-900/30 text-green-400 border border-green-700"
                        } else {
                            "bg-yellow-900/30 text-yellow-400 border border-yellow-700"
                        },
                        {announce_status.clone()}
                    }
                    span { class: "text-xs text-gray-400",
                        {peer_count.to_string()}
                        span { " peers" }
                    }
                }
            }
            div { class: "mt-3 flex items-center gap-4 text-xs pt-3 border-t border-gray-700",
                div {
                    span { class: "text-gray-400", "Seeders: " }
                    span { class: "text-green-400 font-medium", {seeders.to_string()} }
                }
                div {
                    span { class: "text-gray-400", "Leechers: " }
                    span { class: "text-blue-400 font-medium", {leechers.to_string()} }
                }
            }
        }
    }
}

#[component]
fn TorrentInfoDisplay(info: ReadSignal<Option<TorrentInfo>>) -> Element {
    let mut expanded = use_signal(|| false);

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

    rsx! {
        div { class: "mt-4",
            // Header with toggle button
            button {
                class: "w-full flex items-center justify-between text-left p-3 bg-gray-800 rounded border border-gray-700 hover:bg-gray-700 transition-colors",
                onclick: move |_| {
                    let current = *expanded.read();
                    expanded.set(!current);
                },
                h3 { class: "text-sm font-semibold text-gray-300 uppercase tracking-wide", "Details" }
                span { class: "text-xs text-gray-400",
                    if *expanded.read() {
                        "▼"
                    } else {
                        "▶"
                    }
                }
            }

            if *expanded.read() {
                div { class: "mt-3 space-y-4",
                    // Basic Info
                    div { class: "grid grid-cols-2 gap-4",
                div {
                    h4 { class: "text-xs font-semibold text-gray-400 uppercase tracking-wide mb-2", "Name" }
                    p {
                        class: "text-sm font-medium tracking-tight text-white bg-gray-800 px-3 py-2 rounded border border-gray-700",
                        {torrent_info.name.clone()}
                    }
                }
                div {
                    h4 { class: "text-xs font-semibold text-gray-400 uppercase tracking-wide mb-2", "Total Size" }
                    p {
                        class: "text-sm font-medium tracking-tight text-white bg-gray-800 px-3 py-2 rounded border border-gray-700",
                        {format_size(torrent_info.total_size)}
                    }
                }
                div {
                    h4 { class: "text-xs font-semibold text-gray-400 uppercase tracking-wide mb-2", "Piece Length" }
                    p {
                        class: "text-sm font-medium tracking-tight text-white bg-gray-800 px-3 py-2 rounded border border-gray-700",
                        {format_size(torrent_info.piece_length as i64)}
                    }
                }
                div {
                    h4 { class: "text-xs font-semibold text-gray-400 uppercase tracking-wide mb-2", "Number of Pieces" }
                    p {
                        class: "text-sm font-medium tracking-tight text-white bg-gray-800 px-3 py-2 rounded border border-gray-700",
                        {torrent_info.num_pieces.to_string()}
                    }
                }
                div {
                    h4 { class: "text-xs font-semibold text-gray-400 uppercase tracking-wide mb-2", "Private" }
                    p {
                        class: "text-sm font-medium tracking-tight text-white bg-gray-800 px-3 py-2 rounded border border-gray-700",
                        if torrent_info.is_private { "Yes" } else { "No" }
                    }
                }
                    }

                    // Comment
                    if !torrent_info.comment.is_empty() {
                        div {
                            h4 { class: "text-xs font-semibold text-gray-400 uppercase tracking-wide mb-2", "Comment" }
                            p {
                                class: "text-sm font-medium tracking-tight text-white bg-gray-800 px-3 py-2 rounded border border-gray-700 break-words",
                                {torrent_info.comment.clone()}
                            }
                        }
                    }

                    // Creator
                    if !torrent_info.creator.is_empty() {
                        div {
                            h4 { class: "text-xs font-semibold text-gray-400 uppercase tracking-wide mb-2", "Created By" }
                            p {
                                class: "text-sm font-medium tracking-tight text-white bg-gray-800 px-3 py-2 rounded border border-gray-700",
                                {torrent_info.creator.clone()}
                            }
                        }
                    }

                    // Creation Date
                    if torrent_info.creation_date != 0 {
                        div {
                            h4 { class: "text-xs font-semibold text-gray-400 uppercase tracking-wide mb-2", "Creation Date" }
                            p {
                                class: "text-sm font-medium tracking-tight text-white bg-gray-800 px-3 py-2 rounded border border-gray-700",
                                {format_date(torrent_info.creation_date)}
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TorrentFilesDisplay(info: ReadSignal<Option<TorrentInfo>>) -> Element {
    use crate::ui::components::import::FileInfo;
    let mut expanded = use_signal(|| false);

    let torrent_info_opt = info.read();
    let Some(torrent_info) = torrent_info_opt.as_ref() else {
        return rsx! {
            div { "No torrent info available" }
        };
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

    if files.is_empty() {
        return rsx! {
            div { "No files available" }
        };
    }

    rsx! {
        div { class: "mt-4",
            // Header with toggle button
            button {
                class: "w-full flex items-center justify-between text-left p-3 bg-gray-800 rounded border border-gray-700 hover:bg-gray-700 transition-colors",
                onclick: move |_| {
                    let current = *expanded.read();
                    expanded.set(!current);
                },
                h3 { class: "text-sm font-semibold text-gray-300 uppercase tracking-wide", "Files" }
                span { class: "text-xs text-gray-400",
                    if *expanded.read() {
                        "▼"
                    } else {
                        "▶"
                    }
                }
            }

            if *expanded.read() {
                div { class: "mt-3",
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
