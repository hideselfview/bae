//! Recursive folder scanner with leaf detection for multi-release imports.
//!
//! Supports three folder structures:
//! 1. Single release (flat) - audio files in root, optional artwork subfolders
//! 2. Single release (multi-disc) - disc subfolders with audio, optional artwork
//! 3. Collections - recursive tree where leaves are single releases

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

/// Files from a release, pre-categorized by type
#[derive(Debug, Clone, Default)]
pub struct CategorizedFiles {
    /// Audio track files (.flac, .mp3, etc.)
    pub tracks: Vec<ScannedFile>,
    /// Artwork/image files (.jpg, .png, etc.)
    pub artwork: Vec<ScannedFile>,
    /// Document files (.cue, .log, .txt, .nfo)
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
/// and categorizes them into tracks, artwork, documents, and other.
pub fn collect_release_files(release_root: &Path) -> Result<CategorizedFiles, String> {
    let mut categorized = CategorizedFiles::default();
    collect_files_recursive(release_root, release_root, &mut categorized)?;

    // Sort each category by relative path for consistent ordering
    categorized
        .tracks
        .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    categorized
        .artwork
        .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    categorized
        .documents
        .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    categorized
        .other
        .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    Ok(categorized)
}

/// Recursively collect and categorize files
fn collect_files_recursive(
    current_dir: &Path,
    release_root: &Path,
    categorized: &mut CategorizedFiles,
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

            // Categorize the file
            if is_audio_file(&path) {
                categorized.tracks.push(file);
            } else if is_image_file(&path) {
                categorized.artwork.push(file);
            } else if is_document_file(&path) {
                categorized.documents.push(file);
            } else {
                categorized.other.push(file);
            }
        } else if path.is_dir() {
            // Recurse into subdirectories
            collect_files_recursive(&path, release_root, categorized)?;
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
