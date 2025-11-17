use crate::torrent::progress::{TorrentProgress, TrackerStatus};
use crate::ui::components::import::FileInfo;
use dioxus::prelude::*;
use std::collections::HashMap;

#[derive(Clone)]
pub struct TorrentStatusState {
    pub info_hash: String,
    pub name: String,
    pub total_size: u64,
    pub num_files: usize,
    pub num_peers: i32,
    pub num_seeds: i32,
    pub trackers: Vec<TrackerStatus>,
    pub files: Vec<FileInfo>,
    pub metadata_files: Vec<String>,
    pub metadata_progress: HashMap<String, f32>, // file -> progress (0.0 to 1.0)
    pub is_detecting: bool,
}

#[component]
pub fn TorrentStatus(info_hash: String, on_skip: Option<EventHandler<()>>) -> Element {
    let import_context = use_context::<std::rc::Rc<crate::ui::import_context::ImportContext>>();
    let torrent_manager = import_context.torrent_manager();
    let mut state = use_signal(|| TorrentStatusState {
        info_hash: info_hash.clone(),
        name: String::new(),
        total_size: 0,
        num_files: 0,
        num_peers: 0,
        num_seeds: 0,
        trackers: vec![],
        files: vec![],
        metadata_files: vec![],
        metadata_progress: HashMap::new(),
        is_detecting: false,
    });

    // Subscribe to progress events
    use_effect({
        let torrent_manager = torrent_manager.clone();
        let info_hash = info_hash.clone();
        let mut state = state;
        let import_context = import_context.clone();
        move || {
            let torrent_manager = torrent_manager.clone();
            let info_hash = info_hash.clone();
            let import_context = import_context.clone();
            spawn(async move {
                let mut progress_rx = torrent_manager.subscribe_torrent(info_hash.clone());
                while let Some(progress) = progress_rx.recv().await {
                    match progress {
                        TorrentProgress::WaitingForMetadata { .. } => {
                            // Keep waiting state
                        }
                        TorrentProgress::TorrentInfoReady {
                            name,
                            total_size,
                            num_files,
                            ..
                        } => {
                            state.write().name = name;
                            state.write().total_size = total_size;
                            state.write().num_files = num_files;
                            // Update files from import context
                            let folder_files = import_context.folder_files();
                            state.write().files = folder_files.read().clone();
                        }
                        TorrentProgress::StatusUpdate {
                            num_peers,
                            num_seeds,
                            trackers,
                            ..
                        } => {
                            state.write().num_peers = num_peers;
                            state.write().num_seeds = num_seeds;
                            state.write().trackers = trackers;
                        }
                        TorrentProgress::MetadataFilesDetected { files, .. } => {
                            state.write().metadata_files = files.clone();
                            state.write().is_detecting = true;
                        }
                        TorrentProgress::MetadataProgress { file, progress, .. } => {
                            state.write().metadata_progress.insert(file, progress);
                        }
                        TorrentProgress::MetadataComplete { .. } => {
                            state.write().is_detecting = false;
                        }
                        TorrentProgress::Error { message, .. } => {
                            tracing::warn!("Torrent error: {}", message);
                        }
                    }
                }
            });
        }
    });

    // Also populate files from import context on mount
    use_effect({
        let import_context = import_context.clone();
        let mut state = state;
        move || {
            let folder_files = import_context.folder_files();
            state.write().files = folder_files.read().clone();
        }
    });

    let state_read = state.read();

    rsx! {
        div { class: "bg-white rounded-lg shadow p-6 space-y-4",
            // Torrent info
            if !state_read.name.is_empty() {
                div { class: "border-b border-gray-200 pb-4",
                    h4 { class: "text-lg font-semibold text-gray-900 mb-2", {state_read.name.clone()} }
                    div { class: "flex gap-4 text-sm text-gray-600",
                        span { "Size: {format_size(state_read.total_size)}" }
                        span { "Files: {state_read.num_files}" }
                    }
                }
            }

            // Trackers section
            if !state_read.trackers.is_empty() {
                div { class: "border-b border-gray-200 pb-4",
                    h5 { class: "text-sm font-semibold text-gray-700 uppercase tracking-wide mb-2", "Trackers" }
                    div { class: "space-y-2",
                        for tracker in &state_read.trackers {
                            div { class: "flex items-center justify-between text-sm",
                                span { class: "text-gray-700 font-mono", {tracker.url.clone()} }
                                span {
                                    class: match tracker.status.as_str() {
                                        "announcing" => "px-2 py-1 bg-yellow-100 text-yellow-800 rounded",
                                        "connected" => "px-2 py-1 bg-green-100 text-green-800 rounded",
                                        "error" => "px-2 py-1 bg-red-100 text-red-800 rounded",
                                        _ => "px-2 py-1 bg-gray-100 text-gray-800 rounded",
                                    },
                                    {tracker.status.clone()}
                                }
                            }
                            if let Some(ref msg) = tracker.message {
                                if !msg.is_empty() {
                                    p { class: "text-xs text-gray-500 ml-4", {msg.clone()} }
                                }
                            }
                        }
                    }
                }
            }

            // Peers section
            div { class: "border-b border-gray-200 pb-4",
                h5 { class: "text-sm font-semibold text-gray-700 uppercase tracking-wide mb-2", "Peers" }
                div { class: "flex gap-4 text-sm text-gray-600",
                    span { "Peers: {state_read.num_peers}" }
                    span { "Seeds: {state_read.num_seeds}" }
                }
            }

            // Files section
            if !state_read.files.is_empty() {
                div { class: "border-b border-gray-200 pb-4",
                    h5 { class: "text-sm font-semibold text-gray-700 uppercase tracking-wide mb-2", "Files" }
                    div { class: "space-y-1 max-h-48 overflow-y-auto",
                        for file in &state_read.files {
                            div {
                                class: if state_read.metadata_files.iter().any(|mf| file.name.contains(mf)) {
                                    "text-sm py-1 px-2 bg-blue-50 border border-blue-200 rounded"
                                } else {
                                    "text-sm py-1 px-2"
                                },
                                {file.name.clone()}
                                span { class: "text-gray-500 ml-2", "({format_size(file.size)})" }
                            }
                        }
                    }
                }
            }

            // Metadata detection section
            if state_read.is_detecting && !state_read.metadata_files.is_empty() {
                div { class: "border-b border-gray-200 pb-4",
                    h5 { class: "text-sm font-semibold text-gray-700 uppercase tracking-wide mb-2", "Downloading Metadata Files" }
                    div { class: "space-y-3",
                        for file in &state_read.metadata_files {
                            div { class: "space-y-1",
                                div { class: "flex justify-between text-sm",
                                    span { class: "text-gray-700", {file.clone()} }
                                    span { class: "text-gray-600", "{(state_read.metadata_progress.get(file).copied().unwrap_or(0.0) * 100.0) as u32}%" }
                                }
                                div { class: "w-full bg-gray-200 rounded-full h-2",
                                    div {
                                        class: "bg-blue-600 h-2 rounded-full transition-all",
                                        style: "width: {state_read.metadata_progress.get(file).copied().unwrap_or(0.0) * 100.0}%",
                                    }
                                }
                            }
                        }
                    }
                    if let Some(on_skip) = on_skip {
                        div { class: "mt-4",
                            button {
                                class: "px-4 py-2 bg-gray-200 hover:bg-gray-300 text-gray-800 rounded transition-colors",
                                onclick: move |_| on_skip.call(()),
                                "Skip and search manually"
                            }
                        }
                    }
                }
            }
        }
    }
}

fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.2} {}", size, UNITS[unit_index])
}
