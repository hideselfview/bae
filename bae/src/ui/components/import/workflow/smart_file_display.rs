use crate::ui::components::import::{AudioContentInfo, CategorizedFileInfo, FileInfo};
use chardetng::EncodingDetector;
use dioxus::prelude::*;
use tracing::warn;

use super::text_file_modal::TextFileModal;

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

/// Render audio content based on type (CUE/FLAC pairs or track files)
fn render_audio_content(audio: &AudioContentInfo) -> Element {
    match audio {
        AudioContentInfo::CueFlacPairs(pairs) => {
            rsx! {
                for pair in pairs.iter() {
                    div {
                        class: "p-4 bg-gray-800/50 border border-purple-500/30 rounded-lg",
                        div { class: "flex items-start gap-3",
                            div {
                                class: "flex-shrink-0 w-10 h-10 bg-purple-600 rounded flex items-center justify-center",
                                span { class: "text-white text-lg", "💿" }
                            }
                            div { class: "flex-1",
                                div { class: "flex items-center gap-2 mb-1",
                                    span { class: "text-sm font-semibold text-purple-300", "CUE/FLAC" }
                                    span {
                                        class: "px-2 py-0.5 bg-purple-600/50 text-purple-200 text-xs rounded",
                                        {format!("{} tracks", pair.track_count)}
                                    }
                                }
                                div { class: "text-xs text-gray-400",
                                    {format!("{} total", format_file_size(pair.total_size))}
                                }
                                div { class: "text-xs text-gray-500 mt-1 truncate",
                                    {pair.flac_name.clone()}
                                }
                            }
                        }
                    }
                }
            }
        }
        AudioContentInfo::TrackFiles(tracks) if !tracks.is_empty() => {
            let total_size: u64 = tracks.iter().map(|f| f.size).sum();
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
                                    {format!("{} tracks", tracks.len())}
                                }
                            }
                            div { class: "text-xs text-gray-400",
                                {format!("{} total", format_file_size(total_size))}
                            }
                        }
                    }
                }
            }
        }
        AudioContentInfo::TrackFiles(_) => {
            // Empty tracks - render nothing
            rsx! {}
        }
    }
}

/// Read a text file with automatic encoding detection
async fn read_text_file_with_encoding(path: &str) -> Result<String, String> {
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
pub fn SmartFileDisplay(files: CategorizedFileInfo, folder_path: String) -> Element {
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

    rsx! {
        if files.is_empty() {
            div { class: "text-gray-400 text-center py-8",
                "No files found"
            }
        } else {
            div { class: "space-y-3",
                // Audio section - render based on content type
                {render_audio_content(&files.audio)}

                // Artwork section
                for image in files.artwork.iter() {
                    {render_image_file(image, &folder_path)}
                }

                // Documents section
                for doc in files.documents.iter() {
                    {render_text_file(doc, &folder_path, on_text_file_click)}
                }

                // Other files section (initially hidden)
                if !files.other.is_empty() {
                    div { class: "pt-2",
                        button {
                            class: "w-full px-3 py-2 text-sm text-gray-400 hover:text-gray-300 bg-gray-900/50 hover:bg-gray-800/50 border border-gray-800 hover:border-gray-700 rounded transition-colors",
                            onclick: move |_| show_other_files.set(!show_other_files()),
                            div { class: "flex items-center justify-between",
                                span {
                                    if show_other_files() {
                                        "Hide other files ({files.other.len()})"
                                    } else {
                                        "Show other files ({files.other.len()})"
                                    }
                                }
                                span { class: "text-xs",
                                    if show_other_files() { "▲" } else { "▼" }
                                }
                            }
                        }

                        if show_other_files() {
                            div { class: "mt-3 space-y-2",
                                for file in files.other.iter() {
                                    {render_other_file(file)}
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

fn render_image_file(file: &FileInfo, folder_path: &str) -> Element {
    let image_path = format!("{}/{}", folder_path, file.name);
    rsx! {
        div {
            class: "p-3 bg-gray-800 border border-gray-700 rounded-lg",
            div { class: "flex items-start gap-3",
                img {
                    src: "file://{image_path}",
                    class: "w-20 h-20 object-cover rounded flex-shrink-0",
                    alt: "{file.name}",
                }
                div { class: "flex-1 min-w-0",
                    div { class: "text-sm text-white font-medium truncate", {file.name.clone()} }
                    div { class: "text-xs text-gray-400 mt-1",
                        {format!("{} • {}", format_file_size(file.size), file.format)}
                    }
                }
            }
        }
    }
}

fn render_text_file(
    file: &FileInfo,
    folder_path: &str,
    on_click: impl Fn(String, String) + Copy + 'static,
) -> Element {
    let text_path = format!("{}/{}", folder_path, file.name);
    let filename = file.name.clone();
    let file_size = file.size;
    rsx! {
        div {
            class: "p-3 bg-gray-800 border border-gray-700 rounded-lg hover:bg-gray-750 hover:border-gray-600 transition-colors cursor-pointer",
            onclick: move |_| on_click(filename.clone(), text_path.clone()),
            div { class: "flex items-center gap-3",
                div {
                    class: "flex-shrink-0 w-8 h-8 bg-gray-700 rounded flex items-center justify-center",
                    span { class: "text-gray-400 text-sm", "📄" }
                }
                div { class: "flex-1 min-w-0",
                    div { class: "text-sm text-white font-medium truncate", {file.name.clone()} }
                    div { class: "text-xs text-gray-400 mt-0.5",
                        {format!("{} • Click to view", format_file_size(file_size))}
                    }
                }
            }
        }
    }
}

fn render_other_file(file: &FileInfo) -> Element {
    rsx! {
        div {
            class: "p-2 bg-gray-800 border border-gray-700 rounded",
            div { class: "flex items-center justify-between",
                div { class: "flex-1 min-w-0",
                    div { class: "text-sm text-gray-300 truncate", {file.name.clone()} }
                }
                div { class: "text-xs text-gray-500 ml-2",
                    {format!("{} • {}", format_file_size(file.size), file.format)}
                }
            }
        }
    }
}
