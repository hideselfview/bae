use crate::cue_flac::CueFlacProcessor;
use crate::ui::components::import::FileInfo;
use chardetng::EncodingDetector;
use dioxus::prelude::*;
use std::path::PathBuf;
use tracing::warn;

use super::text_file_modal::TextFileModal;

#[derive(Debug, Clone, PartialEq)]
struct CueFlacGroup {
    cue_file: FileInfo,
    flac_file: FileInfo,
    total_size: u64,
}

#[derive(Debug, Clone, PartialEq)]
struct TrackGroup {
    files: Vec<FileInfo>,
    total_size: u64,
}

#[derive(Debug, Clone, PartialEq)]
struct ImageFile {
    file: FileInfo,
}

#[derive(Debug, Clone, PartialEq)]
struct TextFile {
    file: FileInfo,
}

#[derive(Debug, Clone, PartialEq)]
struct OtherFile {
    file: FileInfo,
    is_noise: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum FileGroup {
    CueFlac(CueFlacGroup),
    Tracks(TrackGroup),
    Image(ImageFile),
    Text(TextFile),
    Other(OtherFile),
}

fn is_image_file(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".webp")
        || lower.ends_with(".gif")
}

fn is_text_file(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".txt")
        || lower.ends_with(".log")
        || lower.ends_with(".cue")
        || lower.ends_with(".nfo")
}

fn is_noise_file(name: &str) -> bool {
    name == ".DS_Store" || name == "Thumbs.db" || name == "desktop.ini"
}

fn is_sequential_track(name: &str) -> bool {
    // Match patterns like "01.flac", "02 - Title.flac", "Track 01.flac", etc.
    let lower = name.to_lowercase();
    if !lower.ends_with(".flac") && !lower.ends_with(".mp3") && !lower.ends_with(".m4a") {
        return false;
    }

    // Check if starts with digits (possibly zero-padded)
    let first_chars: String = name.chars().take(2).collect();
    first_chars.chars().all(|c| c.is_ascii_digit())
}

fn group_files(files: &[FileInfo], folder_path: &str) -> Vec<FileGroup> {
    let mut primary_groups = Vec::new();
    let mut other_groups = Vec::new();
    let mut processed_indices = std::collections::HashSet::new();

    // Detect CUE/FLAC pairs
    let file_paths: Vec<PathBuf> = files
        .iter()
        .map(|f| PathBuf::from(folder_path).join(&f.name))
        .collect();

    let cue_flac_pairs = match CueFlacProcessor::detect_cue_flac_from_paths(&file_paths) {
        Ok(pairs) => pairs,
        Err(e) => {
            warn!("Failed to detect CUE/FLAC pairs: {}", e);
            Vec::new()
        }
    };

    // Process CUE/FLAC pairs
    for pair in cue_flac_pairs {
        let cue_name = pair
            .cue_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let flac_name = pair
            .flac_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if let (Some(cue_idx), Some(flac_idx)) = (
            files.iter().position(|f| f.name == cue_name),
            files.iter().position(|f| f.name == flac_name),
        ) {
            let cue_file = files[cue_idx].clone();
            let flac_file = files[flac_idx].clone();
            let total_size = cue_file.size + flac_file.size;

            primary_groups.push(FileGroup::CueFlac(CueFlacGroup {
                cue_file,
                flac_file,
                total_size,
            }));

            processed_indices.insert(cue_idx);
            processed_indices.insert(flac_idx);
        }
    }

    // Group sequential tracks
    let mut track_files = Vec::new();
    for (idx, file) in files.iter().enumerate() {
        if processed_indices.contains(&idx) {
            continue;
        }

        if is_sequential_track(&file.name) {
            track_files.push(file.clone());
            processed_indices.insert(idx);
        }
    }

    if track_files.len() >= 3 {
        // Only group if there are at least 3 tracks
        let total_size = track_files.iter().map(|f| f.size).sum();
        primary_groups.push(FileGroup::Tracks(TrackGroup {
            files: track_files,
            total_size,
        }));
    } else {
        // Add them back as individual files
        for file in track_files {
            other_groups.push(FileGroup::Other(OtherFile {
                file,
                is_noise: false,
            }));
        }
    }

    // Process remaining files
    for (idx, file) in files.iter().enumerate() {
        if processed_indices.contains(&idx) {
            continue;
        }

        if is_image_file(&file.name) {
            primary_groups.push(FileGroup::Image(ImageFile { file: file.clone() }));
        } else if is_text_file(&file.name) {
            primary_groups.push(FileGroup::Text(TextFile { file: file.clone() }));
        } else {
            other_groups.push(FileGroup::Other(OtherFile {
                file: file.clone(),
                is_noise: is_noise_file(&file.name),
            }));
        }
    }

    // Combine primary groups first, then other groups at the end
    primary_groups.extend(other_groups);
    primary_groups
}

fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Read a text file with automatic encoding detection
async fn read_text_file_with_encoding(path: &str) -> Result<String, String> {
    // Read the raw bytes
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Try UTF-8 first (fast path)
    if let Ok(content) = String::from_utf8(bytes.clone()) {
        return Ok(content);
    }

    // Use encoding detection
    let mut detector = EncodingDetector::new();
    detector.feed(&bytes, true);
    let encoding = detector.guess(None, true);

    // Decode using detected encoding
    let (decoded, _, had_errors) = encoding.decode(&bytes);

    if had_errors {
        warn!(
            "Decoding errors occurred while reading {} with encoding {}",
            path,
            encoding.name()
        );
    }

    Ok(decoded.into_owned())
}

#[component]
pub fn SmartFileDisplay(files: Vec<FileInfo>, folder_path: String) -> Element {
    let mut modal_state = use_signal(|| None::<(String, String)>);
    let mut show_other_files = use_signal(|| false);

    let on_text_file_click = move |filename: String, filepath: String| {
        spawn(async move {
            match read_text_file_with_encoding(&filepath).await {
                Ok(content) => {
                    modal_state.set(Some((filename, content)));
                }
                Err(e) => {
                    warn!("Failed to read file {}: {}", filepath, e);
                }
            }
        });
    };

    let groups = group_files(&files, &folder_path);
    let (primary_groups, other_groups): (Vec<_>, Vec<_>) = groups
        .iter()
        .partition(|g| !matches!(g, FileGroup::Other(_)));

    rsx! {
        if files.is_empty() {
            div { class: "text-gray-400 text-center py-8",
                "No files found"
            }
        } else {
            div { class: "space-y-3",
                // Primary groups (CUE/FLAC, tracks, images, text files)
                for group in primary_groups.iter() {
                    {render_file_group(group, folder_path.clone(), on_text_file_click)}
                }

                // Other files section (initially hidden)
                if !other_groups.is_empty() {
                    div { class: "pt-2",
                        button {
                            class: "w-full px-3 py-2 text-sm text-gray-400 hover:text-gray-300 bg-gray-900/50 hover:bg-gray-800/50 border border-gray-800 hover:border-gray-700 rounded transition-colors",
                            onclick: move |_| show_other_files.set(!show_other_files()),
                            div { class: "flex items-center justify-between",
                                span {
                                    if show_other_files() {
                                        "Hide other files ({other_groups.len()})"
                                    } else {
                                        "Show other files ({other_groups.len()})"
                                    }
                                }
                                span { class: "text-xs",
                                    if show_other_files() { "▲" } else { "▼" }
                                }
                            }
                        }

                        if show_other_files() {
                            div { class: "mt-3 space-y-2",
                                for group in other_groups.iter() {
                                    {render_file_group(group, folder_path.clone(), on_text_file_click)}
                                }
                            }
                        }
                    }
                }
            }
        }

        // Modal for text files
        if let Some((filename, content)) = modal_state.read().as_ref() {
            TextFileModal {
                filename: filename.clone(),
                content: content.clone(),
                on_close: move |_| modal_state.set(None),
            }
        }
    }
}

fn render_file_group(
    group: &FileGroup,
    folder_path: String,
    on_text_file_click: impl Fn(String, String) + Copy + 'static,
) -> Element {
    match group {
        FileGroup::CueFlac(cue_flac) => {
            let cue_path = format!("{}/{}", folder_path, cue_flac.cue_file.name);
            let cue_filename = cue_flac.cue_file.name.clone();
            rsx! {
                div {
                    class: "p-4 bg-gradient-to-r from-purple-900/30 to-blue-900/30 border border-purple-500/50 rounded-lg",
                    div { class: "flex items-start gap-3",
                        div {
                            class: "flex-shrink-0 w-10 h-10 bg-purple-600 rounded flex items-center justify-center",
                            span { class: "text-white text-lg", "🎵" }
                        }
                        div { class: "flex-1 min-w-0",
                            div { class: "flex items-center gap-2 mb-1",
                                span { class: "text-sm font-semibold text-purple-300", "CUE/FLAC" }
                            }
                            div { class: "text-sm text-gray-300 space-y-0.5",
                                div { class: "font-medium truncate", {cue_flac.flac_file.name.clone()} }
                                div {
                                    class: "text-xs text-gray-400 truncate hover:text-purple-300 cursor-pointer transition-colors",
                                    onclick: move |_| on_text_file_click(cue_filename.clone(), cue_path.clone()),
                                    {cue_flac.cue_file.name.clone()}
                                }
                            }
                            div { class: "text-xs text-gray-400 mt-2",
                                {format_file_size(cue_flac.total_size)}
                            }
                        }
                    }
                }
            }
        }
        FileGroup::Tracks(tracks) => {
            rsx! {
                div {
                    class: "p-4 bg-gray-800/50 border border-blue-500/30 rounded-lg",
                    div { class: "flex items-start gap-3",
                        div {
                            class: "flex-shrink-0 w-10 h-10 bg-blue-600 rounded flex items-center justify-center",
                            span { class: "text-white text-lg", "🎼" }
                        }
                        div { class: "flex-1",
                            div { class: "flex items-center gap-2 mb-1",
                                span { class: "text-sm font-semibold text-blue-300", "Track Files" }
                                span {
                                    class: "px-2 py-0.5 bg-blue-600/50 text-blue-200 text-xs rounded",
                                    {format!("{} tracks", tracks.files.len())}
                                }
                            }
                            div { class: "text-xs text-gray-400",
                                {format!("{} total", format_file_size(tracks.total_size))}
                            }
                        }
                    }
                }
            }
        }
        FileGroup::Image(image) => {
            let image_path = format!("{}/{}", folder_path, image.file.name);
            rsx! {
                div {
                    class: "p-3 bg-gray-800 border border-gray-700 rounded-lg",
                    div { class: "flex items-start gap-3",
                        img {
                            src: "file://{image_path}",
                            class: "w-20 h-20 object-cover rounded flex-shrink-0",
                            alt: "{image.file.name}",
                        }
                        div { class: "flex-1 min-w-0",
                            div { class: "text-sm text-white font-medium truncate", {image.file.name.clone()} }
                            div { class: "text-xs text-gray-400 mt-1",
                                {format!("{} • {}", format_file_size(image.file.size), image.file.format)}
                            }
                        }
                    }
                }
            }
        }
        FileGroup::Text(text) => {
            let text_path = format!("{}/{}", folder_path, text.file.name);
            let filename = text.file.name.clone();
            rsx! {
                div {
                    class: "p-3 bg-gray-800 border border-gray-700 rounded-lg hover:bg-gray-750 hover:border-gray-600 transition-colors cursor-pointer",
                    onclick: move |_| on_text_file_click(filename.clone(), text_path.clone()),
                    div { class: "flex items-center gap-3",
                        div {
                            class: "flex-shrink-0 w-8 h-8 bg-gray-700 rounded flex items-center justify-center",
                            span { class: "text-gray-400 text-sm", "📄" }
                        }
                        div { class: "flex-1 min-w-0",
                            div { class: "text-sm text-white font-medium truncate", {text.file.name.clone()} }
                            div { class: "text-xs text-gray-400 mt-0.5",
                                {format!("{} • Click to view", format_file_size(text.file.size))}
                            }
                        }
                    }
                }
            }
        }
        FileGroup::Other(other) => {
            let class_name = if other.is_noise {
                "p-2 bg-gray-900/50 border border-gray-800 rounded opacity-50"
            } else {
                "p-2 bg-gray-800 border border-gray-700 rounded"
            };
            rsx! {
                div {
                    class: "{class_name}",
                    div { class: "flex items-center justify-between",
                        div { class: "flex-1 min-w-0",
                            div { class: "text-sm text-gray-300 truncate", {other.file.name.clone()} }
                        }
                        div { class: "text-xs text-gray-500 ml-2",
                            {format!("{} • {}", format_file_size(other.file.size), other.file.format)}
                        }
                    }
                }
            }
        }
    }
}
