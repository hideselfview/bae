use crate::import::folder_scanner::{AudioContent, ScannedCueFlacPair, ScannedFile};
use dioxus::prelude::*;

#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub format: String,
}

/// A CUE/FLAC pair for UI display
#[derive(Debug, Clone, PartialEq)]
pub struct CueFlacPairInfo {
    pub cue_name: String,
    pub flac_name: String,
    pub total_size: u64,
    pub track_count: usize,
}

/// Audio content type for UI display
#[derive(Debug, Clone, PartialEq)]
pub enum AudioContentInfo {
    /// One or more CUE/FLAC pairs
    CueFlacPairs(Vec<CueFlacPairInfo>),
    /// Individual track files
    TrackFiles(Vec<FileInfo>),
}

impl Default for AudioContentInfo {
    fn default() -> Self {
        AudioContentInfo::TrackFiles(Vec::new())
    }
}

/// Pre-categorized files for UI display
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CategorizedFileInfo {
    /// Audio content - CUE/FLAC pairs or track files
    pub audio: AudioContentInfo,
    /// Artwork/image files
    pub artwork: Vec<FileInfo>,
    /// Document files (.log, .txt, .nfo) - CUE files in pairs are NOT here
    pub documents: Vec<FileInfo>,
    /// Everything else
    pub other: Vec<FileInfo>,
}

impl CategorizedFileInfo {
    /// Convert from backend CategorizedFiles
    pub fn from_scanned(categorized: &crate::import::CategorizedFiles) -> Self {
        let convert = |files: &[ScannedFile]| -> Vec<FileInfo> {
            files
                .iter()
                .map(|f| {
                    let format = std::path::Path::new(&f.relative_path)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_uppercase();
                    FileInfo {
                        name: f.relative_path.clone(),
                        size: f.size,
                        format,
                    }
                })
                .collect()
        };

        let convert_pair = |pair: &ScannedCueFlacPair| -> CueFlacPairInfo {
            CueFlacPairInfo {
                cue_name: pair.cue_file.relative_path.clone(),
                flac_name: pair.audio_file.relative_path.clone(),
                total_size: pair.cue_file.size + pair.audio_file.size,
                track_count: pair.track_count,
            }
        };

        let audio = match &categorized.audio {
            AudioContent::CueFlacPairs(pairs) => {
                AudioContentInfo::CueFlacPairs(pairs.iter().map(convert_pair).collect())
            }
            AudioContent::TrackFiles(tracks) => AudioContentInfo::TrackFiles(convert(tracks)),
        };

        Self {
            audio,
            artwork: convert(&categorized.artwork),
            documents: convert(&categorized.documents),
            other: convert(&categorized.other),
        }
    }

    /// Total number of files across all categories
    pub fn total_count(&self) -> usize {
        let audio_count = match &self.audio {
            AudioContentInfo::CueFlacPairs(pairs) => pairs.len() * 2, // CUE + FLAC
            AudioContentInfo::TrackFiles(tracks) => tracks.len(),
        };
        audio_count + self.artwork.len() + self.documents.len() + self.other.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.total_count() == 0
    }
}

#[component]
pub fn FileList(files: Vec<FileInfo>) -> Element {
    rsx! {
        if files.is_empty() {
            div { class: "text-gray-400 text-center py-8",
                "No files found"
            }
        } else {
            div { class: "space-y-2",
                for file in files.iter() {
                    div {
                        class: "flex items-center justify-between py-2 px-3 bg-gray-800 rounded hover:bg-gray-700 transition-colors border border-gray-700",
                        div {
                            class: "flex-1",
                            div {
                                class: "text-white text-sm font-medium",
                                {file.name.clone()}
                            }
                            div {
                                class: "text-gray-400 text-xs mt-1",
                                {format!("{} • {}", format_file_size(file.size as i64), file.format)}
                            }
                        }
                    }
                }
            }
        }
    }
}

fn format_file_size(bytes: i64) -> String {
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
