use crate::cue_flac::CueFlacProcessor;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, PartialEq)]
pub struct FolderMetadata {
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<u32>,
    pub discid: Option<String>,    // FreeDB DiscID
    pub mb_discid: Option<String>, // MusicBrainz DiscID
    pub track_count: Option<u32>,
    pub confidence: f32, // 0-100%
}

#[derive(Debug, Error)]
pub enum MetadataDetectionError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Extract DISCID from CUE file content
fn extract_discid_from_cue(content: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("REM DISCID ") {
            // Extract DISCID value (everything after "REM DISCID ")
            let discid = line.strip_prefix("REM DISCID ")?.trim();
            if !discid.is_empty() {
                return Some(discid.to_string());
            }
        }
    }
    None
}

/// Extract year from CUE REM DATE lines
fn extract_year_from_cue(content: &str) -> Option<u32> {
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("REM DATE ") {
            let date_str = line.strip_prefix("REM DATE ")?.trim();
            // Try to parse year (could be "2000" or "2000 / 2004")
            if let Some(year_str) = date_str.split('/').next() {
                if let Ok(year) = year_str.trim().parse::<u32>() {
                    if (1900..=2100).contains(&year) {
                        return Some(year);
                    }
                }
            }
        }
    }
    None
}

/// Read FLAC metadata using symphonia
/// Note: Symphonia metadata reading is complex, so we'll skip FLAC tag reading for now
/// and rely on CUE files and MP3 tags. FLAC metadata can be added later if needed.
fn read_flac_metadata(_path: &Path) -> (Option<String>, Option<String>, Option<u32>) {
    // TODO: Implement FLAC metadata reading using symphonia
    // For now, return empty metadata - CUE files will provide the metadata
    (None, None, None)
}

/// Get FLAC file duration in seconds using Symphonia
fn get_flac_duration_seconds(flac_path: &Path) -> Result<f64, MetadataDetectionError> {
    use std::fs::File;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;

    let file = File::open(flac_path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = symphonia::core::probe::Hint::new();
    hint.with_extension("flac");

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &Default::default())
        .map_err(|e| {
            MetadataDetectionError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to probe FLAC file: {}", e),
            ))
        })?;

    let track = probed.format.default_track().ok_or_else(|| {
        MetadataDetectionError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "No default track found in FLAC file",
        ))
    })?;

    let total_samples = track.codec_params.n_frames.ok_or_else(|| {
        MetadataDetectionError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "FLAC file missing sample count",
        ))
    })?;

    let sample_rate = track.codec_params.sample_rate.ok_or_else(|| {
        MetadataDetectionError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "FLAC file missing sample rate",
        ))
    })?;

    let duration_seconds = total_samples as f64 / sample_rate as f64;
    Ok(duration_seconds)
}

/// Extract track INDEX offsets from CUE file content
/// Returns (final offsets with 150 added, raw sectors without 150)
fn extract_track_offsets_from_cue(
    cue_content: &str,
) -> Result<(Vec<i32>, Vec<i32>), MetadataDetectionError> {
    let mut offsets = Vec::new();
    let mut raw_sectors = Vec::new();

    for line in cue_content.lines() {
        let line = line.trim();
        if line.starts_with("INDEX 01 ") {
            let time_str = line.strip_prefix("INDEX 01 ").unwrap_or("").trim();
            let parts: Vec<&str> = time_str.split(':').collect();
            if parts.len() == 3 {
                if let (Ok(mm), Ok(ss), Ok(ff)) = (
                    parts[0].parse::<u32>(),
                    parts[1].parse::<u32>(),
                    parts[2].parse::<u32>(),
                ) {
                    // Calculate raw sectors (without lead-in offset)
                    let raw_sector = ((mm * 60 + ss) * 75 + ff) as i32;
                    raw_sectors.push(raw_sector);
                    // Add 150 for final offset
                    let sectors = raw_sector + 150;
                    offsets.push(sectors);
                }
            }
        }
    }

    if offsets.is_empty() {
        return Err(MetadataDetectionError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "No INDEX 01 entries found in CUE file",
        )));
    }

    Ok((offsets, raw_sectors))
}

/// Extract lead-out sector from EAC/XLD log file
/// Looks for the "End sector" column in the TOC table
/// Format: "       10  | 37:42.72 |  4:14.43 |    169722    |   188814"
/// The 5th column (index 4) contains the end sector for each track
/// Returns (final offset with 150 added, raw sector without 150)
fn extract_leadout_from_log(log_content: &str) -> Option<(i32, i32)> {
    debug!("🔍 Parsing LOG file to extract lead-out sector");

    // Find the TOC section - look for "TOC" header (works for English and non-English logs)
    // Also detect TOC table format directly as fallback
    let mut in_toc_section = false;
    let mut last_end_sector = None;
    let mut track_count = 0;

    for line in log_content.lines() {
        let line = line.trim();
        let line_lower = line.to_ascii_lowercase();

        // Detect TOC section start - look for "TOC" keyword (language-independent)
        // This matches "TOC of the extracted CD" in English and "TOC ������������ CD" in Russian/etc
        if line_lower.contains("toc")
            && (line_lower.contains("cd") || line_lower.contains("extracted"))
        {
            in_toc_section = true;
            debug!("Found TOC section header: {}", line);
            continue;
        }

        // Also detect TOC table format directly (fallback for logs without clear header)
        // Look for lines with pipe separators containing track numbers
        if !in_toc_section && line.contains('|') {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 5 {
                // Check if first column looks like a track number (1, 2, 3, etc.)
                let first_col = parts[0].trim();
                if let Ok(track_num) = first_col.parse::<u32>() {
                    if (1..=99).contains(&track_num) {
                        // Check if 5th column (index 4) is a valid sector number
                        let end_sector_str = parts[4].trim();
                        if end_sector_str.parse::<i32>().is_ok() {
                            in_toc_section = true;
                            debug!("Found TOC table format directly (no header)");
                            // Fall through to parse this line
                        }
                    }
                }
            }
        }

        // Stop parsing after TOC section (look for next major section)
        if in_toc_section
            && (line_lower.contains("range status")
                || line_lower.contains("accuraterip")
                || (line.is_empty() && track_count > 0 && last_end_sector.is_some()))
        {
            debug!("End of TOC section, found {} tracks", track_count);
            break;
        }

        if !in_toc_section {
            continue;
        }

        // Skip header separator lines and empty lines
        // Check for header lines in a language-independent way
        if line.contains("---")
            || line.is_empty()
            || (line_lower.contains("track")
                && (line_lower.contains("start") || line_lower.contains("sector")))
        {
            continue;
        }

        // Parse lines with pipe separators (TOC table format)
        if line.contains('|') {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 5 {
                // The 5th column (index 4) is the end sector
                let end_sector_str = parts[4].trim();
                if let Ok(sector) = end_sector_str.parse::<i32>() {
                    if sector > 0 {
                        track_count += 1;
                        last_end_sector = Some(sector);
                        debug!("  Track {} end sector: {}", track_count, sector);
                    }
                }
            }
        }
    }

    if let Some(sector) = last_end_sector {
        // The last track's "End sector" is the end of the last track.
        // The lead-out starts one sector after that, so we add 1 before adding the lead-in offset.
        let lead_out_start = sector + 1;
        let lead_out = lead_out_start + 150; // Add lead-in offset
        info!(
            "✅ Extracted lead-out from LOG: {} sectors (last track end: {}, lead-out start: {}, tracks found: {})",
            lead_out, sector, lead_out_start, track_count
        );
        Some((lead_out, lead_out_start))
    } else {
        warn!("⚠️ Could not find any end sectors in LOG file");
        // Try to find TOC section for debug output (language-independent)
        let toc_start = log_content.lines().position(|l| {
            let l_lower = l.to_ascii_lowercase();
            l_lower.contains("toc") && (l_lower.contains("cd") || l_lower.contains("extracted"))
        });
        let preview: String = if let Some(start_idx) = toc_start {
            log_content
                .lines()
                .skip(start_idx)
                .take(15)
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            log_content.lines().take(30).collect::<Vec<_>>().join("\n")
        };
        debug!("LOG content preview (TOC section):\n{}", preview);
        None
    }
}

/// Extract track offsets from EAC/XLD log file
/// Looks for the "Start sector" column in the TOC table
/// Format: "       10  | 37:42.72 |  4:14.43 |    169722    |   188814"
/// The 4th column (index 3) contains the start sector for each track
/// Returns (final offsets with 150 added, raw sectors without 150)
fn extract_track_offsets_from_log(
    log_content: &str,
) -> Result<(Vec<i32>, Vec<i32>), MetadataDetectionError> {
    debug!("🔍 Parsing LOG file to extract track offsets");

    // Find the TOC section - look for "TOC" header (works for English and non-English logs)
    // Also detect TOC table format directly as fallback
    let mut in_toc_section = false;
    let mut track_offsets = Vec::new();
    let mut raw_sectors = Vec::new();

    for line in log_content.lines() {
        let line = line.trim();
        let line_lower = line.to_ascii_lowercase();

        // Detect TOC section start - look for "TOC" keyword (language-independent)
        // This matches "TOC of the extracted CD" in English and "TOC ������������ CD" in Russian/etc
        if line_lower.contains("toc")
            && (line_lower.contains("cd") || line_lower.contains("extracted"))
        {
            in_toc_section = true;
            debug!("Found TOC section header: {}", line);
            continue;
        }

        // Also detect TOC table format directly (fallback for logs without clear header)
        // Look for lines with pipe separators containing track numbers
        if !in_toc_section && line.contains('|') {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 5 {
                // Check if first column looks like a track number (1, 2, 3, etc.)
                let first_col = parts[0].trim();
                if let Ok(track_num) = first_col.parse::<u32>() {
                    if (1..=99).contains(&track_num) {
                        // Check if 4th column (index 3) is a valid sector number
                        let start_sector_str = parts[3].trim();
                        if start_sector_str.parse::<i32>().is_ok() {
                            in_toc_section = true;
                            debug!("Found TOC table format directly (no header)");
                            // Fall through to parse this line
                        }
                    }
                }
            }
        }

        // Stop parsing after TOC section (look for next major section)
        if in_toc_section
            && (line_lower.contains("range status")
                || line_lower.contains("accuraterip")
                || (line.is_empty() && !track_offsets.is_empty()))
        {
            debug!("End of TOC section, found {} tracks", track_offsets.len());
            break;
        }

        if !in_toc_section {
            continue;
        }

        // Skip header separator lines and empty lines
        // Check for header lines in a language-independent way
        if line.contains("---")
            || line.is_empty()
            || (line_lower.contains("track")
                && (line_lower.contains("start") || line_lower.contains("sector")))
        {
            continue;
        }

        // Parse lines with pipe separators (TOC table format)
        // Format: "       1  |  0:00.00 |  5:12.10 |         0    |    23409   "
        // Columns: Track | Start | Length | Start sector | End sector
        // Index:     0        1       2          3             4
        if line.contains('|') {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 5 {
                // The 4th column (index 3) is the start sector
                let start_sector_str = parts[3].trim();
                if let Ok(sector) = start_sector_str.parse::<i32>() {
                    if sector >= 0 {
                        raw_sectors.push(sector);
                        // Add 150 to match discid format (lead-in offset)
                        let offset = sector + 150;
                        track_offsets.push(offset);
                        debug!(
                            "  Track {} start sector: {} (offset: {})",
                            track_offsets.len(),
                            sector,
                            offset
                        );
                    }
                }
            }
        }
    }

    if track_offsets.is_empty() {
        return Err(MetadataDetectionError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "No track offsets found in LOG file",
        )));
    }

    info!(
        "✅ Extracted {} track offset(s) from LOG",
        track_offsets.len()
    );
    Ok((track_offsets, raw_sectors))
}

/// Calculate MusicBrainz DiscID from LOG file alone
/// This is the most efficient method as it doesn't require CUE or audio files
pub fn calculate_mb_discid_from_log(log_path: &Path) -> Result<String, MetadataDetectionError> {
    info!("🎵 Calculating MusicBrainz DiscID from LOG: {:?}", log_path);

    // Read LOG file - handle UTF-16 and non-UTF-8 content gracefully
    info!("📄 Reading LOG file: {:?}", log_path);
    let log_bytes = fs::read(log_path)?;
    info!("📏 LOG file size: {} bytes", log_bytes.len());

    // Try to decode - LOG files can be UTF-16 (Windows EAC) or UTF-8
    let log_content = if log_bytes.len() >= 2 && log_bytes[0] == 0xFF && log_bytes[1] == 0xFE {
        // UTF-16 LE BOM
        info!("📄 Detected UTF-16 LE encoding");
        let utf16_chars: Vec<u16> = log_bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        String::from_utf16_lossy(&utf16_chars)
    } else if log_bytes.len() >= 2 && log_bytes[0] == 0xFE && log_bytes[1] == 0xFF {
        // UTF-16 BE BOM
        info!("📄 Detected UTF-16 BE encoding");
        let utf16_chars: Vec<u16> = log_bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect();
        String::from_utf16_lossy(&utf16_chars)
    } else {
        // Try UTF-8, using lossy conversion if needed
        info!("📄 Assuming UTF-8 encoding");
        String::from_utf8_lossy(&log_bytes).to_string()
    };
    info!("📄 LOG file decoded, length: {} chars", log_content.len());

    // Extract track offsets from log
    let (track_offsets, raw_track_sectors) = extract_track_offsets_from_log(&log_content)?;
    info!("📊 Found {} track(s) in LOG file", track_offsets.len());
    info!(
        "📊 LOG METHOD - Raw track start sectors (before adding 150): {:?}",
        raw_track_sectors
    );

    // Extract lead-out from log
    let (lead_out_sectors, raw_leadout_sector) = extract_leadout_from_log(&log_content).ok_or_else(|| {
        warn!("⚠️ Could not extract lead-out sector from log file. Log content preview (first 500 chars):\n{}", 
              log_content.chars().take(500).collect::<String>());
        MetadataDetectionError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Could not extract lead-out sector from log file",
        ))
    })?;
    info!(
        "📏 LOG METHOD - Raw lead-out sector (before adding 150): {}",
        raw_leadout_sector
    );
    info!(
        "📏 LOG METHOD - Lead-out offset: {} sectors (raw: {} + 150)",
        lead_out_sectors, raw_leadout_sector
    );

    // Build offsets array in the format expected by discid:
    // [lead_out, track1_offset, track2_offset, ...]
    let mut offsets = Vec::with_capacity(track_offsets.len() + 1);
    offsets.push(lead_out_sectors);
    offsets.extend_from_slice(&track_offsets);

    let first_track = 1;
    let last_track = track_offsets.len() as i32;

    info!(
        "🎯 First track: {}, Last track: {}, Total offsets: {}",
        first_track,
        last_track,
        offsets.len()
    );

    // Print all offsets for comparison
    info!("📋 LOG METHOD - Offsets array (lead-out first, then tracks):");
    info!("   Lead-out: {} sectors", offsets[0]);
    for (i, offset) in offsets.iter().enumerate().skip(1) {
        info!("   Track {}: {} sectors", i, offset);
    }
    info!("📋 LOG METHOD - Raw offsets array: {:?}", offsets);

    // Create DiscID using discid crate
    let disc = discid::DiscId::put(first_track, &offsets).map_err(|e| {
        MetadataDetectionError::Io(std::io::Error::other(format!(
            "Failed to calculate DiscID: {}",
            e
        )))
    })?;

    let mb_discid_str = disc.id();
    info!("✅ MusicBrainz DiscID: {}", mb_discid_str);
    println!("🎵 MusicBrainz DiscID result: {}", mb_discid_str);

    Ok(mb_discid_str.to_string())
}

/// Calculate MusicBrainz DiscID from CUE file and FLAC file
/// This requires both files: CUE for track offsets, FLAC for lead-out calculation
pub fn calculate_mb_discid_from_cue_flac(
    cue_path: &Path,
    flac_path: &Path,
) -> Result<String, MetadataDetectionError> {
    info!(
        "🎵 Calculating MusicBrainz DiscID from CUE: {:?}, FLAC: {:?}",
        cue_path, flac_path
    );

    // Read CUE file
    let cue_content = fs::read_to_string(cue_path)?;

    // Extract track offsets
    let (track_offsets, raw_track_sectors) = extract_track_offsets_from_cue(&cue_content)?;
    info!("📊 Found {} track(s) in CUE file", track_offsets.len());
    info!(
        "📊 CUE/FLAC METHOD - Raw track start sectors (before adding 150): {:?}",
        raw_track_sectors
    );

    // Get FLAC duration
    let duration_seconds = get_flac_duration_seconds(flac_path)?;
    info!("⏱️ FLAC duration: {:.2} seconds", duration_seconds);

    // Calculate lead-out offset: total duration in sectors + lead-in
    let raw_leadout_sector = (duration_seconds * 75.0).round() as i32;
    let lead_out_sectors = raw_leadout_sector + 150;
    info!(
        "📏 CUE/FLAC METHOD - Raw lead-out sector (from FLAC duration): {} sectors",
        raw_leadout_sector
    );
    info!(
        "📏 CUE/FLAC METHOD - Lead-out offset: {} sectors (raw: {} + 150)",
        lead_out_sectors, raw_leadout_sector
    );

    // Build offsets array in the format expected by discid:
    // [lead_out, track1_offset, track2_offset, ...]
    // The discid crate expects: offsets[0] = lead-out, offsets[1..] = track offsets
    let mut offsets = Vec::with_capacity(track_offsets.len() + 1);
    offsets.push(lead_out_sectors);
    offsets.extend_from_slice(&track_offsets);

    let first_track = 1;
    let last_track = track_offsets.len() as i32;

    info!(
        "🎯 First track: {}, Last track: {}, Total offsets: {}",
        first_track,
        last_track,
        offsets.len()
    );

    // Print all offsets for comparison
    info!("📋 CUE/FLAC METHOD - Offsets array (lead-out first, then tracks):");
    info!("   Lead-out: {} sectors", offsets[0]);
    for (i, offset) in offsets.iter().enumerate().skip(1) {
        info!("   Track {}: {} sectors", i, offset);
    }
    info!("📋 CUE/FLAC METHOD - Raw offsets array: {:?}", offsets);

    // Create DiscID using discid crate
    // The discid crate API: DiscId::put(first, offsets) where:
    // - first = first track number (usually 1)
    // - offsets[0] = lead-out (total sectors)
    // - offsets[1..] = track offsets
    let disc = discid::DiscId::put(first_track, &offsets).map_err(|e| {
        MetadataDetectionError::Io(std::io::Error::other(format!(
            "Failed to calculate DiscID: {}",
            e
        )))
    })?;

    let mb_discid_str = disc.id();
    info!("✅ MusicBrainz DiscID calculated: {}", mb_discid_str);

    Ok(mb_discid_str.to_string())
}

/// Read MP3 metadata using id3
fn read_mp3_metadata(path: &Path) -> (Option<String>, Option<String>, Option<u32>) {
    match id3::Tag::read_from_path(path) {
        Ok(tag) => {
            let mut artist = None;
            let mut album = None;
            let mut year = None;

            // Iterate through frames to find metadata
            for frame in tag.frames() {
                match frame.id() {
                    "TPE1" | "TPE2" => {
                        // Lead performer/soloist or Band/orchestra/accompaniment
                        if artist.is_none() {
                            if let Some(text) = frame.content().text() {
                                artist = Some(text.to_string());
                            }
                        }
                    }
                    "TALB" => {
                        // Album/Movie/Show title
                        if album.is_none() {
                            if let Some(text) = frame.content().text() {
                                album = Some(text.to_string());
                            }
                        }
                    }
                    "TDRC" => {
                        // Recording time (YYYY-MM-DD format)
                        if year.is_none() {
                            if let Some(text) = frame.content().text() {
                                if let Some(year_str) = text.split('-').next() {
                                    if let Ok(y) = year_str.parse::<u32>() {
                                        if (1900..=2100).contains(&y) {
                                            year = Some(y);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "TYER" => {
                        // Year (ID3v2.3)
                        if year.is_none() {
                            if let Some(text) = frame.content().text() {
                                if let Ok(y) = text.parse::<u32>() {
                                    if (1900..=2100).contains(&y) {
                                        year = Some(y);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            (artist, album, year)
        }
        Err(id3::Error {
            kind: id3::ErrorKind::NoTag,
            ..
        }) => {
            // No tags found, not an error
            (None, None, None)
        }
        Err(e) => {
            warn!("Failed to read MP3 metadata from {:?}: {}", path, e);
            // Return empty metadata instead of error to allow fallback to other sources
            (None, None, None)
        }
    }
}

/// Try to extract artist/album from folder name (e.g., "Artist - Album")
fn parse_folder_name(folder_path: &Path) -> (Option<String>, Option<String>) {
    if let Some(folder_name) = folder_path.file_name().and_then(|n| n.to_str()) {
        if let Some((artist, album)) = folder_name.split_once(" - ") {
            let artist = artist.trim().to_string();
            let album = album.trim().to_string();
            if !artist.is_empty() && !album.is_empty() {
                return (Some(artist), Some(album));
            }
        }
    }
    (None, None)
}

/// Detect metadata from a folder containing audio files
pub fn detect_metadata(folder_path: PathBuf) -> Result<FolderMetadata, MetadataDetectionError> {
    use tracing::info;

    info!(
        "📁 Starting metadata detection for folder: {:?}",
        folder_path
    );

    let mut artist_sources = Vec::new();
    let mut album_sources = Vec::new();
    let mut year_sources = Vec::new();
    let mut discid: Option<String> = None;
    let mut mb_discid: Option<String> = None;
    let mut track_count: Option<u32> = None;

    // Check for CUE files first (highest priority for DISCID)
    let entries = fs::read_dir(&folder_path)?;
    let mut cue_files = Vec::new();
    let mut log_files = Vec::new();
    let mut audio_files = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            match ext.to_lowercase().as_str() {
                "cue" => cue_files.push(path),
                "log" => log_files.push(path),
                "flac" | "mp3" | "wav" | "m4a" | "aac" | "ogg" => {
                    audio_files.push(path);
                }
                _ => {}
            }
        }
    }

    info!(
        "📄 Found {} CUE file(s), {} log file(s), {} audio file(s)",
        cue_files.len(),
        log_files.len(),
        audio_files.len()
    );

    // Process CUE files
    for cue_path in &cue_files {
        debug!("Reading CUE file: {:?}", cue_path);
        if let Ok(content) = fs::read_to_string(cue_path) {
            // Extract FreeDB DISCID
            if discid.is_none() {
                discid = extract_discid_from_cue(&content);
                if let Some(ref id) = discid {
                    info!("💿 Found FreeDB DISCID in CUE: {}", id);
                }
            }

            // Calculate MusicBrainz DiscID - try log file first, then FLAC
            if mb_discid.is_none() {
                let cue_stem = cue_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

                // Try matching log file first (more efficient, no audio download needed)
                if let Some(log_path) = log_files
                    .iter()
                    .find(|p| p.file_stem().and_then(|s| s.to_str()) == Some(cue_stem))
                {
                    info!(
                        "🔍 Attempting MB DiscID calculation from LOG file: {:?}",
                        log_path
                    );
                    match calculate_mb_discid_from_log(log_path) {
                        Ok(id) => {
                            info!("✅ Calculated MusicBrainz DiscID from log: {}", id);
                            mb_discid = Some(id);
                        }
                        Err(e) => {
                            warn!("✗ Failed to calculate MB DiscID from log: {}", e);
                            info!("🔄 Will try FLAC file as fallback if available");
                        }
                    }
                } else {
                    debug!("No matching LOG file found for CUE stem: {}", cue_stem);
                }

                // Fall back to FLAC if log didn't work
                if mb_discid.is_none() {
                    info!(
                        "🔍 Looking for matching FLAC file for CUE stem: {}",
                        cue_stem
                    );
                    if let Some(flac_path) = audio_files.iter().find(|p| {
                        p.extension().and_then(|e| e.to_str()) == Some("flac")
                            && p.file_stem().and_then(|s| s.to_str()) == Some(cue_stem)
                    }) {
                        info!("📀 Found matching FLAC file: {:?}", flac_path);
                        match calculate_mb_discid_from_cue_flac(cue_path, flac_path) {
                            Ok(id) => {
                                info!("✅ Calculated MusicBrainz DiscID from FLAC: {}", id);
                                mb_discid = Some(id);
                            }
                            Err(e) => {
                                warn!("✗ Failed to calculate MB DiscID from FLAC: {}", e);
                            }
                        }
                    } else {
                        info!(
                            "⚠️ No matching FLAC file found for CUE: {:?} (stem: {})",
                            cue_path, cue_stem
                        );
                        info!(
                            "📋 Available audio files: {:?}",
                            audio_files
                                .iter()
                                .map(|p| p.file_name())
                                .collect::<Vec<_>>()
                        );
                    }
                }
            }

            // Extract year from REM DATE
            if year_sources.is_empty() {
                if let Some(y) = extract_year_from_cue(&content) {
                    year_sources.push((y, 0.9)); // High confidence from CUE
                }
            }

            // Parse CUE sheet for title/performer
            match CueFlacProcessor::parse_cue_sheet(cue_path) {
                Ok(cue_sheet) => {
                    info!(
                        "✓ Parsed CUE: artist='{}', album='{}', tracks={}",
                        cue_sheet.performer,
                        cue_sheet.title,
                        cue_sheet.tracks.len()
                    );
                    if !cue_sheet.performer.is_empty() {
                        artist_sources.push((cue_sheet.performer.clone(), 0.9));
                    }
                    if !cue_sheet.title.is_empty() {
                        album_sources.push((cue_sheet.title.clone(), 0.9));
                    }
                    track_count = Some(cue_sheet.tracks.len() as u32);
                }
                Err(e) => {
                    warn!("✗ Failed to parse CUE file {:?}: {}", cue_path, e);
                }
            }
        }
    }

    // Process audio files for metadata
    let mut audio_files_read = 0;
    for audio_path in &audio_files {
        let (artist, album, year) = match audio_path.extension().and_then(|e| e.to_str()) {
            Some("flac") => {
                debug!("Reading FLAC metadata: {:?}", audio_path.file_name());
                read_flac_metadata(audio_path)
            }
            Some("mp3") => {
                debug!("Reading MP3 metadata: {:?}", audio_path.file_name());
                read_mp3_metadata(audio_path)
            }
            _ => continue,
        };

        if artist.is_some() || album.is_some() || year.is_some() {
            audio_files_read += 1;
            debug!(
                "  → artist={:?}, album={:?}, year={:?}",
                artist, album, year
            );
        }

        if let Some(a) = artist {
            artist_sources.push((a, 0.8)); // High confidence from tags
        }
        if let Some(alb) = album {
            album_sources.push((alb, 0.8));
        }
        if let Some(y) = year {
            year_sources.push((y, 0.7)); // Medium confidence from tags
        }
    }

    if audio_files_read > 0 {
        info!("✓ Read metadata from {} audio file(s)", audio_files_read);
    }

    // Count tracks if not already set
    if track_count.is_none() {
        track_count = Some(audio_files.len() as u32);
    }

    // Fallback to folder name parsing
    let (folder_artist, folder_album) = parse_folder_name(&folder_path);
    if let Some(ref a) = folder_artist {
        debug!("Parsed folder name: artist='{}'", a);
        artist_sources.push((a.clone(), 0.3)); // Low confidence from folder name
    }
    if let Some(ref alb) = folder_album {
        debug!("Parsed folder name: album='{}'", alb);
        album_sources.push((alb.clone(), 0.3));
    }

    // Aggregate metadata with weighted scoring
    info!(
        "📊 Aggregating metadata from {} artist sources, {} album sources, {} year sources",
        artist_sources.len(),
        album_sources.len(),
        year_sources.len()
    );

    let artist = aggregate_string_sources(artist_sources);
    let album = aggregate_string_sources(album_sources);
    let year = aggregate_year_sources(year_sources);

    // Calculate overall confidence
    let mut confidence = 0.0;
    if artist.is_some() {
        confidence += 30.0;
    }
    if album.is_some() {
        confidence += 30.0;
    }
    if year.is_some() {
        confidence += 10.0;
    }
    if discid.is_some() {
        confidence += 20.0; // FreeDB DISCID is very reliable
    }
    if mb_discid.is_some() {
        confidence += 20.0; // MusicBrainz DiscID is very reliable
    }
    if track_count.is_some() {
        confidence += 10.0;
    }

    let metadata = FolderMetadata {
        artist: artist.clone(),
        album: album.clone(),
        year,
        discid: discid.clone(),
        mb_discid: mb_discid.clone(),
        track_count,
        confidence,
    };

    info!("✅ Detection complete: confidence={:.0}%", confidence);
    info!("   → Artist: {:?}", artist);
    info!("   → Album: {:?}", album);
    info!("   → Year: {:?}", year);
    info!("   → FreeDB DISCID: {:?}", discid);
    info!("   → MusicBrainz DiscID: {:?}", mb_discid);
    info!("   → Tracks: {:?}", track_count);

    Ok(metadata)
}

/// Aggregate string sources by picking the highest confidence one
fn aggregate_string_sources(sources: Vec<(String, f32)>) -> Option<String> {
    sources
        .into_iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(s, _)| s)
}

/// Aggregate year sources by picking the most common or highest confidence
fn aggregate_year_sources(sources: Vec<(u32, f32)>) -> Option<u32> {
    if sources.is_empty() {
        return None;
    }

    // Group by year and sum confidence
    use std::collections::HashMap;
    let mut year_scores: HashMap<u32, f32> = HashMap::new();
    for (year, conf) in sources {
        *year_scores.entry(year).or_insert(0.0) += conf;
    }

    // Pick year with highest total confidence
    year_scores
        .into_iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(y, _)| y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_leadout_from_log_acdc() {
        // Use the fixture LOG file
        let log_path = PathBuf::from("tests/fixtures/acdc_back_in_black.log");

        // Try alternative path if running from different directory
        let log_path = if log_path.exists() {
            log_path
        } else {
            PathBuf::from("bae/tests/fixtures/acdc_back_in_black.log")
        };

        if !log_path.exists() {
            eprintln!("LOG file not found at: {:?}", log_path);
            eprintln!("Current directory: {:?}", std::env::current_dir().unwrap());
            return;
        }

        println!("🎵 Testing LOG file parsing");
        println!("   LOG: {:?}", log_path);

        // Initialize tracing for debug output
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();

        // Read the LOG file as bytes (like the real code does)
        let log_bytes = std::fs::read(&log_path).expect("Failed to read LOG file");
        println!("📄 LOG file size: {} bytes", log_bytes.len());

        // Decode - matching the real implementation (handles UTF-16 and UTF-8)
        let log_content = if log_bytes.len() >= 2 && log_bytes[0] == 0xFF && log_bytes[1] == 0xFE {
            // UTF-16 LE BOM
            println!("📄 Detected UTF-16 LE encoding");
            let utf16_chars: Vec<u16> = log_bytes[2..]
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();
            String::from_utf16_lossy(&utf16_chars)
        } else if log_bytes.len() >= 2 && log_bytes[0] == 0xFE && log_bytes[1] == 0xFF {
            // UTF-16 BE BOM
            println!("📄 Detected UTF-16 BE encoding");
            let utf16_chars: Vec<u16> = log_bytes[2..]
                .chunks_exact(2)
                .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
                .collect();
            String::from_utf16_lossy(&utf16_chars)
        } else {
            // Try UTF-8, using lossy conversion if needed
            println!("📄 Assuming UTF-8 encoding");
            String::from_utf8_lossy(&log_bytes).to_string()
        };
        println!(
            "📄 LOG file decoded, length: {} chars, {} lines",
            log_content.len(),
            log_content.lines().count()
        );

        // Show TOC section for debugging
        println!("📄 TOC section:");
        let mut in_toc = false;
        for (i, line) in log_content.lines().enumerate() {
            if line.contains("TOC of the extracted") {
                in_toc = true;
            }
            if in_toc {
                println!("   {}: {}", i + 1, line);
                if line.contains("Range status") || line.contains("AccurateRip") {
                    break;
                }
            }
        }

        // Test extracting lead-out
        let lead_out = extract_leadout_from_log(&log_content);
        match lead_out {
            Some((final_offset, raw_sector)) => {
                println!(
                    "✅ Successfully extracted lead-out: {} sectors (raw: {})",
                    final_offset, raw_sector
                );
                // Expected: last track end sector is 188814, so lead-out start is 188815, and final offset is 188815 + 150 = 188965
                assert_eq!(
                    final_offset, 188965,
                    "Expected lead-out to be 188965 (188814 + 1 + 150)"
                );
                assert_eq!(
                    raw_sector, 188815,
                    "Expected raw lead-out sector to be 188815 (188814 + 1)"
                );
            }
            None => {
                eprintln!("❌ Failed to extract lead-out from LOG file");
                eprintln!(
                    "LOG content preview (TOC section):\n{}",
                    log_content
                        .lines()
                        .skip_while(|l| !l.contains("TOC of the extracted"))
                        .take(15)
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                panic!("Failed to extract lead-out");
            }
        }
    }

    #[test]
    fn test_calculate_mb_discid_from_log_acdc() {
        // Use the fixture LOG file
        let log_path = PathBuf::from("tests/fixtures/acdc_back_in_black.log");

        // Try alternative path if running from different directory
        let log_path = if log_path.exists() {
            log_path
        } else {
            PathBuf::from("bae/tests/fixtures/acdc_back_in_black.log")
        };

        if !log_path.exists() {
            eprintln!("LOG file not found at: {:?}", log_path);
            eprintln!("Current directory: {:?}", std::env::current_dir().unwrap());
            return;
        }

        println!("🎵 Testing MB DiscID calculation from LOG file alone");
        println!("   LOG: {:?}", log_path);

        // Initialize tracing for debug output
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();

        match calculate_mb_discid_from_log(&log_path) {
            Ok(discid) => {
                println!(
                    "✅ Successfully calculated MusicBrainz DiscID from LOG: {}",
                    discid
                );
                assert_eq!(discid.len(), 28, "DiscID should be 28 characters");
                assert!(
                    discid
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                    "DiscID should contain only alphanumeric characters, dashes, and underscores"
                );
            }
            Err(e) => {
                eprintln!("❌ Failed to calculate DiscID from LOG: {}", e);
                panic!("Failed to calculate DiscID from LOG: {}", e);
            }
        }
    }

    #[test]
    fn test_calculate_mb_discid_from_log_acdc_cue_log() {
        // Use the fixture LOG file (CUE not needed anymore)
        let log_path = PathBuf::from("tests/fixtures/acdc_back_in_black.log");

        // Try alternative path if running from different directory
        let log_path = if log_path.exists() {
            log_path
        } else {
            PathBuf::from("bae/tests/fixtures/acdc_back_in_black.log")
        };

        if !log_path.exists() {
            eprintln!("LOG file not found, skipping test");
            eprintln!("  LOG: {:?} (exists: {})", log_path, log_path.exists());
            return;
        }

        println!("🎵 Testing MB DiscID calculation from LOG file alone");
        println!("   LOG: {:?}", log_path);

        // Initialize tracing for debug output
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();

        match calculate_mb_discid_from_log(&log_path) {
            Ok(discid) => {
                println!("✅ Successfully calculated MusicBrainz DiscID: {}", discid);
                assert_eq!(discid.len(), 28, "DiscID should be 28 characters");
                assert!(
                    discid
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                    "DiscID should contain only alphanumeric characters, dashes, and underscores"
                );
            }
            Err(e) => {
                eprintln!("❌ Failed to calculate DiscID: {}", e);
                panic!("Failed to calculate DiscID: {}", e);
            }
        }
    }
}
