//! Recursive folder scanner with leaf detection for multi-release imports.
//!
//! Supports three folder structures:
//! 1. Single release (flat) - audio files in root, optional artwork subfolders
//! 2. Single release (multi-disc) - disc subfolders with audio, optional artwork
//! 3. Collections - recursive tree where leaves are single releases

use crate::cue_flac::CueFlacProcessor;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

const MAX_RECURSION_DEPTH: usize = 10;
const AUDIO_EXTENSIONS: &[&str] = &["flac", "mp3", "wav", "m4a", "aac", "ogg"];
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp"];
const DOCUMENT_EXTENSIONS: &[&str] = &["cue", "log", "txt", "nfo", "m3u", "m3u8"];

/// A file discovered during folder scanning
#[derive(Debug, Clone)]
pub struct ScannedFile {
    /// Full path to the file
    pub path: PathBuf,
    /// Relative path from release root (for display)
    pub relative_path: String,
    /// File size in bytes
    pub size: u64,
}

/// A CUE/FLAC pair representing a single disc with track count
#[derive(Debug, Clone)]
pub struct ScannedCueFlacPair {
    /// The CUE sheet file
    pub cue_file: ScannedFile,
    /// The audio file (FLAC, WAV, etc.)
    pub audio_file: ScannedFile,
    /// Number of tracks defined in the CUE sheet
    pub track_count: usize,
}

/// The audio content type of a release - mutually exclusive
#[derive(Debug, Clone)]
pub enum AudioContent {
    /// One or more CUE/FLAC pairs (multi-disc releases can have multiple)
    CueFlacPairs(Vec<ScannedCueFlacPair>),
    /// Individual track files (file-per-track releases)
    TrackFiles(Vec<ScannedFile>),
}

impl Default for AudioContent {
    fn default() -> Self {
        AudioContent::TrackFiles(Vec::new())
    }
}

/// Files from a release, pre-categorized by type
#[derive(Debug, Clone, Default)]
pub struct CategorizedFiles {
    /// Audio content - either CUE/FLAC pairs or individual track files
    pub audio: AudioContent,
    /// Artwork/image files (.jpg, .png, etc.)
    pub artwork: Vec<ScannedFile>,
    /// Document files (.log, .txt, .nfo) - CUE files in pairs are NOT included here
    pub documents: Vec<ScannedFile>,
    /// Everything else
    pub other: Vec<ScannedFile>,
}

/// A detected release (leaf directory) in a collection
#[derive(Debug, Clone)]
pub struct DetectedRelease {
    /// Root path of this release
    pub path: PathBuf,
    /// Display name (derived from folder name)
    pub name: String,
    /// Pre-categorized files for this release
    pub files: CategorizedFiles,
}

/// Check if a file is an audio file based on extension
pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Check if a file is an image/artwork file
fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Check if a file is a document file (.cue, .log, .txt, .nfo)
fn is_document_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| DOCUMENT_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Check if a file is a CUE file
fn is_cue_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase() == "cue")
        .unwrap_or(false)
}

/// Check if a file is noise (.DS_Store, Thumbs.db, etc.)
fn is_noise_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|name| name == ".DS_Store" || name == "Thumbs.db" || name == "desktop.ini")
        .unwrap_or(false)
}

/// Check if a directory contains audio files directly
fn has_audio_files(dir: &Path) -> Result<bool, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("Failed to read dir {:?}: {}", dir, e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && is_audio_file(&path) {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Check if a directory has CUE files directly
fn has_cue_files(dir: &Path) -> Result<bool, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("Failed to read dir {:?}: {}", dir, e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && is_cue_file(&path) {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Check if any subdirectory contains audio files
fn has_subdirs_with_audio(dir: &Path) -> Result<bool, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("Failed to read dir {:?}: {}", dir, e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && has_audio_files(&path)? {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Check if any subdirectory has its own subdirectories with audio files
fn has_nested_audio_dirs(dir: &Path) -> Result<bool, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("Failed to read dir {:?}: {}", dir, e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && has_subdirs_with_audio(&path)? {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Determine if a directory is a leaf (single release).
///
/// A directory is a leaf if:
/// - Has audio files directly in it, OR
/// - Has CUE files, OR
/// - Has subdirectories containing audio files, but those subdirectories don't have
///   their own subdirectories with audio files (multi-disc case)
fn is_leaf_directory(dir: &Path) -> Result<bool, String> {
    // Check if has audio files directly
    if has_audio_files(dir)? {
        debug!("Directory {:?} is a leaf (has audio files)", dir);
        return Ok(true);
    }

    // Check if has CUE files
    if has_cue_files(dir)? {
        debug!("Directory {:?} is a leaf (has CUE files)", dir);
        return Ok(true);
    }

    // Check if has subdirs with audio, but no nested audio dirs (multi-disc case)
    if has_subdirs_with_audio(dir)? && !has_nested_audio_dirs(dir)? {
        debug!(
            "Directory {:?} is a leaf (has subdirs with audio, no nesting)",
            dir
        );
        return Ok(true);
    }

    debug!("Directory {:?} is not a leaf", dir);
    Ok(false)
}

/// Recursively scan for release leaves in a folder tree
fn scan_recursive(
    dir: &Path,
    depth: usize,
    releases: &mut Vec<DetectedRelease>,
) -> Result<(), String> {
    if depth > MAX_RECURSION_DEPTH {
        warn!(
            "Max recursion depth {} reached at {:?}, stopping",
            MAX_RECURSION_DEPTH, dir
        );
        return Ok(());
    }

    // Check if this directory is a leaf
    if is_leaf_directory(dir)? {
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        info!("Found release leaf: {:?}", dir);

        // Collect and categorize files for this release
        let files = collect_release_files(dir)?;

        releases.push(DetectedRelease {
            path: dir.to_path_buf(),
            name,
            files,
        });

        // Don't recurse further - this is a release boundary
        return Ok(());
    }

    // Not a leaf - recurse into subdirectories
    let entries = fs::read_dir(dir).map_err(|e| format!("Failed to read dir {:?}: {}", dir, e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_recursive(&path, depth + 1, releases)?;
        }
    }

    Ok(())
}

/// Scan a folder for releases (leaf directories)
///
/// Returns a list of detected releases. Each release is a leaf in the folder tree.
pub fn scan_for_releases(root: PathBuf) -> Result<Vec<DetectedRelease>, String> {
    info!("Scanning for releases in: {:?}", root);

    let mut releases = Vec::new();
    scan_recursive(&root, 0, &mut releases)?;

    info!("Found {} release(s)", releases.len());

    Ok(releases)
}

/// Collect all files from a release directory and categorize them
///
/// This collects files recursively within a single release, preserving relative paths,
/// and categorizes them into audio (CUE/FLAC pairs or track files), artwork, documents, and other.
pub fn collect_release_files(release_root: &Path) -> Result<CategorizedFiles, String> {
    // First, collect all files into temporary vectors
    let mut all_audio: Vec<ScannedFile> = Vec::new();
    let mut all_cue: Vec<ScannedFile> = Vec::new();
    let mut artwork: Vec<ScannedFile> = Vec::new();
    let mut documents: Vec<ScannedFile> = Vec::new();
    let mut other: Vec<ScannedFile> = Vec::new();

    collect_files_into_vectors(
        release_root,
        release_root,
        &mut all_audio,
        &mut all_cue,
        &mut artwork,
        &mut documents,
        &mut other,
    )?;

    // Try to detect CUE/FLAC pairs
    let audio_paths: Vec<PathBuf> = all_audio.iter().map(|f| f.path.clone()).collect();
    let cue_paths: Vec<PathBuf> = all_cue.iter().map(|f| f.path.clone()).collect();
    let all_paths: Vec<PathBuf> = audio_paths
        .iter()
        .chain(cue_paths.iter())
        .cloned()
        .collect();

    let detected_pairs = CueFlacProcessor::detect_cue_flac_from_paths(&all_paths)
        .map_err(|e| format!("CUE/FLAC detection failed: {}", e))?;

    let audio = if !detected_pairs.is_empty() {
        // We have CUE/FLAC pairs - build ScannedCueFlacPair with track counts
        let mut pairs = Vec::new();
        let mut used_audio_paths = std::collections::HashSet::new();
        let mut used_cue_paths = std::collections::HashSet::new();

        for pair in detected_pairs {
            // Find the matching ScannedFile entries
            let cue_file = all_cue
                .iter()
                .find(|f| f.path == pair.cue_path)
                .cloned()
                .ok_or_else(|| format!("CUE file not found: {:?}", pair.cue_path))?;

            let audio_file = all_audio
                .iter()
                .find(|f| f.path == pair.flac_path)
                .cloned()
                .ok_or_else(|| format!("Audio file not found: {:?}", pair.flac_path))?;

            // Parse CUE to get track count
            let track_count = match CueFlacProcessor::parse_cue_sheet(&pair.cue_path) {
                Ok(cue_sheet) => cue_sheet.tracks.len(),
                Err(e) => {
                    warn!("Failed to parse CUE sheet {:?}: {}", pair.cue_path, e);
                    0
                }
            };

            used_audio_paths.insert(pair.flac_path);
            used_cue_paths.insert(pair.cue_path);

            pairs.push(ScannedCueFlacPair {
                cue_file,
                audio_file,
                track_count,
            });
        }

        // Any remaining audio files that weren't paired go to "other"
        // (shouldn't happen in a proper CUE/FLAC release, but handle it)
        for audio in all_audio {
            if !used_audio_paths.contains(&audio.path) {
                other.push(audio);
            }
        }

        // Any remaining CUE files that weren't paired go to documents
        for cue in all_cue {
            if !used_cue_paths.contains(&cue.path) {
                documents.push(cue);
            }
        }

        // Sort pairs by relative path
        pairs.sort_by(|a, b| a.cue_file.relative_path.cmp(&b.cue_file.relative_path));

        AudioContent::CueFlacPairs(pairs)
    } else {
        // No CUE/FLAC pairs - all audio files are individual tracks
        // CUE files go to documents (they're documentation-only)
        documents.extend(all_cue);

        // Sort tracks by relative path
        let mut tracks = all_audio;
        tracks.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

        AudioContent::TrackFiles(tracks)
    };

    // Sort other categories
    artwork.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    documents.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    other.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    Ok(CategorizedFiles {
        audio,
        artwork,
        documents,
        other,
    })
}

/// Recursively collect files into separate vectors by type
fn collect_files_into_vectors(
    current_dir: &Path,
    release_root: &Path,
    audio: &mut Vec<ScannedFile>,
    cue: &mut Vec<ScannedFile>,
    artwork: &mut Vec<ScannedFile>,
    documents: &mut Vec<ScannedFile>,
    other: &mut Vec<ScannedFile>,
) -> Result<(), String> {
    let entries = fs::read_dir(current_dir)
        .map_err(|e| format!("Failed to read dir {:?}: {}", current_dir, e))?;

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_file() {
            // Skip noise files
            if is_noise_file(&path) {
                continue;
            }

            let size = entry
                .metadata()
                .map_err(|e| format!("Failed to read metadata for {:?}: {}", path, e))?
                .len();

            // Calculate relative path from release root
            let relative_path = path
                .strip_prefix(release_root)
                .map_err(|e| format!("Failed to strip prefix: {}", e))?
                .to_string_lossy()
                .to_string();

            let file = ScannedFile {
                path: path.clone(),
                relative_path,
                size,
            };

            // Categorize the file - separate CUE files from other documents
            if is_audio_file(&path) {
                audio.push(file);
            } else if is_cue_file(&path) {
                cue.push(file);
            } else if is_image_file(&path) {
                artwork.push(file);
            } else if is_document_file(&path) {
                documents.push(file);
            } else {
                other.push(file);
            }
        } else if path.is_dir() {
            // Recurse into subdirectories
            collect_files_into_vectors(&path, release_root, audio, cue, artwork, documents, other)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_audio_file() {
        assert!(is_audio_file(Path::new("track.flac")));
        assert!(is_audio_file(Path::new("track.mp3")));
        assert!(is_audio_file(Path::new("track.FLAC")));
        assert!(!is_audio_file(Path::new("cover.jpg")));
        assert!(!is_audio_file(Path::new("notes.txt")));
    }

    #[test]
    fn test_is_cue_file() {
        assert!(is_cue_file(Path::new("album.cue")));
        assert!(is_cue_file(Path::new("album.CUE")));
        assert!(!is_cue_file(Path::new("album.flac")));
    }
}
