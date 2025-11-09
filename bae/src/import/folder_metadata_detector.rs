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

/// Convert CUE time format (MM:SS:FF) to CD sectors
/// CD audio uses 75 sectors per second, and frames are already in 1/75 second units
/// Formula: (MM * 60 + SS) * 75 + FF + 150 (lead-in offset)
fn cue_time_to_sectors(mm: u32, ss: u32, ff: u32) -> i32 {
    ((mm * 60 + ss) * 75 + ff) as i32 + 150
}

/// Extract track INDEX offsets from CUE file content
/// Returns vector of sector offsets for each track's INDEX 01
fn extract_track_offsets_from_cue(cue_content: &str) -> Result<Vec<i32>, MetadataDetectionError> {
    let mut offsets = Vec::new();

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
                    let sectors = cue_time_to_sectors(mm, ss, ff);
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

    Ok(offsets)
}

/// Calculate MusicBrainz DiscID from CUE file and FLAC file
pub fn calculate_mb_discid_from_cue(
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
    let track_offsets = extract_track_offsets_from_cue(&cue_content)?;
    info!("📊 Found {} track(s) in CUE file", track_offsets.len());

    // Get FLAC duration
    let duration_seconds = get_flac_duration_seconds(flac_path)?;
    info!("⏱️ FLAC duration: {:.2} seconds", duration_seconds);

    // Calculate lead-out offset: total duration in sectors + lead-in
    let lead_out_sectors = (duration_seconds * 75.0).round() as i32 + 150;
    info!("📏 Lead-out offset: {} sectors", lead_out_sectors);

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

    // Debug: Print all offsets
    debug!("📋 Offsets (lead-out first, then tracks):");
    debug!("   Lead-out: {} sectors", offsets[0]);
    for (i, offset) in offsets.iter().enumerate().skip(1) {
        debug!("   Track {}: {} sectors", i, offset);
    }

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
    let mut audio_files = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            match ext.to_lowercase().as_str() {
                "cue" => cue_files.push(path),
                "flac" | "mp3" | "wav" | "m4a" | "aac" | "ogg" => {
                    audio_files.push(path);
                }
                _ => {}
            }
        }
    }

    info!(
        "📄 Found {} CUE file(s), {} audio file(s)",
        cue_files.len(),
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

            // Calculate MusicBrainz DiscID if we have a matching FLAC file
            if mb_discid.is_none() {
                // Find matching FLAC file (same stem as CUE file)
                let cue_stem = cue_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if let Some(flac_path) = audio_files.iter().find(|p| {
                    p.extension().and_then(|e| e.to_str()) == Some("flac")
                        && p.file_stem().and_then(|s| s.to_str()) == Some(cue_stem)
                }) {
                    match calculate_mb_discid_from_cue(cue_path, flac_path) {
                        Ok(id) => {
                            mb_discid = Some(id);
                            info!(
                                "🎵 Calculated MusicBrainz DiscID: {}",
                                mb_discid.as_ref().unwrap()
                            );
                        }
                        Err(e) => {
                            warn!("✗ Failed to calculate MB DiscID: {}", e);
                        }
                    }
                } else {
                    debug!("No matching FLAC file found for CUE: {:?}", cue_path);
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
