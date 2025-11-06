use nom::{
    branch::alt,
    bytes::complete::{tag, take_until},
    character::complete::{digit1, line_ending, space1},
    combinator::{map_res, opt},
    multi::many0,
    sequence::{preceded, terminated, tuple},
    IResult,
};
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CueFlacError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("FLAC parsing error: {0}")]
    Flac(String),
    #[error("CUE parsing error: {0}")]
    CueParsing(String),
}

/// Represents a single track in a CUE sheet
#[derive(Debug, Clone)]
pub struct CueTrack {
    pub number: u32,
    pub title: String,
    pub performer: Option<String>,
    pub start_time_ms: u64, // Converted from MM:SS:FF format (INDEX 01)
    pub pregap_time_ms: Option<u64>, // Converted from MM:SS:FF format (INDEX 00, if present)
    pub end_time_ms: Option<u64>, // Calculated from next track or file end
}

/// Represents a parsed CUE sheet
#[derive(Debug, Clone)]
pub struct CueSheet {
    pub title: String,
    pub performer: String,
    pub tracks: Vec<CueTrack>,
}

/// FLAC header information extracted from file
#[derive(Debug, Clone)]
pub struct FlacHeaders {
    pub headers: Vec<u8>, // Raw header blocks (fLaC + STREAMINFO + other metadata)
}

/// Represents a CUE/FLAC pair found during import
#[derive(Debug, Clone)]
pub struct CueFlacPair {
    pub flac_path: std::path::PathBuf,
    pub cue_path: std::path::PathBuf,
}

/// Main processor for CUE/FLAC operations
pub struct CueFlacProcessor;

impl CueFlacProcessor {
    /// Detect CUE/FLAC pairs from a list of file paths (no filesystem traversal)
    pub fn detect_cue_flac_from_paths(
        file_paths: &[std::path::PathBuf],
    ) -> Result<Vec<CueFlacPair>, CueFlacError> {
        let mut pairs = Vec::new();
        let mut flac_files = Vec::new();
        let mut cue_files = Vec::new();

        // Separate FLAC and CUE files
        for path in file_paths {
            if let Some(extension) = path.extension() {
                match extension.to_str() {
                    Some("flac") => flac_files.push(path.clone()),
                    Some("cue") => cue_files.push(path.clone()),
                    _ => {}
                }
            }
        }

        // Match CUE files with FLAC files
        for cue_path in cue_files {
            let cue_stem = cue_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

            // Look for matching FLAC file
            for flac_path in &flac_files {
                let flac_stem = flac_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

                if cue_stem == flac_stem {
                    pairs.push(CueFlacPair {
                        flac_path: flac_path.clone(),
                        cue_path: cue_path.clone(),
                    });
                    break;
                }
            }
        }

        Ok(pairs)
    }

    /// Extract FLAC headers from a FLAC file
    pub fn extract_flac_headers(flac_path: &Path) -> Result<FlacHeaders, CueFlacError> {
        // Read the FLAC file
        let file_data = fs::read(flac_path)?;

        // Find where audio frames start by parsing the file structure
        let audio_start_byte = Self::find_audio_start(&file_data)?;

        // Extract header blocks (everything before audio frames)
        let headers = file_data[..audio_start_byte as usize].to_vec();

        Ok(FlacHeaders { headers })
    }

    /// Find where audio frames start in a FLAC file
    fn find_audio_start(file_data: &[u8]) -> Result<u64, CueFlacError> {
        // FLAC files start with "fLaC" signature
        if file_data.len() < 4 || &file_data[0..4] != b"fLaC" {
            return Err(CueFlacError::Flac("Invalid FLAC signature".to_string()));
        }

        let mut pos = 4; // Skip "fLaC" signature

        // Parse metadata blocks
        loop {
            if pos + 4 > file_data.len() {
                return Err(CueFlacError::Flac("Unexpected end of file".to_string()));
            }

            // Read metadata block header
            let header = u32::from_be_bytes([
                file_data[pos],
                file_data[pos + 1],
                file_data[pos + 2],
                file_data[pos + 3],
            ]);

            let is_last = (header & 0x80000000) != 0;
            let block_size = (header & 0x00FFFFFF) as usize;

            pos += 4; // Skip header
            pos += block_size; // Skip block data

            if is_last {
                break;
            }
        }

        Ok(pos as u64)
    }

    /// Parse a CUE sheet file
    pub fn parse_cue_sheet(cue_path: &Path) -> Result<CueSheet, CueFlacError> {
        let content = fs::read_to_string(cue_path)?;

        match Self::parse_cue_content(&content) {
            Ok((_, cue_sheet)) => Ok(cue_sheet),
            Err(e) => Err(CueFlacError::CueParsing(format!(
                "Failed to parse CUE: {}",
                e
            ))),
        }
    }

    /// Parse CUE sheet content using nom
    fn parse_cue_content(input: &str) -> IResult<&str, CueSheet> {
        // Skip any initial whitespace or comments
        let (input, _) = many0(alt((
            line_ending,
            space1,
            Self::parse_comment_line,
            Self::parse_file_line,
        )))(input)?;

        // Parse TITLE and PERFORMER in any order
        let (input, (title, performer)) = alt((
            |i| {
                let (i, performer) = Self::parse_performer(i)?;
                let (i, title) = Self::parse_title(i)?;
                Ok((i, (title, performer)))
            },
            |i| {
                let (i, title) = Self::parse_title(i)?;
                let (i, performer) = Self::parse_performer(i)?;
                Ok((i, (title, performer)))
            },
        ))(input)?;

        // Skip FILE line and any comments before it
        let (input, _) = many0(alt((
            line_ending,
            space1,
            Self::parse_file_line,
            Self::parse_comment_line,
        )))(input)?;

        let (input, tracks) = Self::parse_tracks(input)?;

        // Calculate end times for tracks
        // XLD behavior: All tracks end at the next track's INDEX 01
        // This means pregaps are included in the previous track
        let mut tracks_with_end_times = tracks;
        for i in 0..tracks_with_end_times.len() {
            if i + 1 < tracks_with_end_times.len() {
                let next_track = &tracks_with_end_times[i + 1];
                // Always end at next track's INDEX 01
                tracks_with_end_times[i].end_time_ms = Some(next_track.start_time_ms);
            }
        }

        Ok((
            input,
            CueSheet {
                title,
                performer,
                tracks: tracks_with_end_times,
            },
        ))
    }

    /// Parse and skip a REM (comment) line
    fn parse_comment_line(input: &str) -> IResult<&str, &str> {
        let (input, _) = tag("REM")(input)?;
        let (input, _) = take_until("\n")(input)?;
        let (input, _) = line_ending(input)?;
        Ok((input, ""))
    }

    /// Parse and skip a FILE line
    fn parse_file_line(input: &str) -> IResult<&str, &str> {
        let (input, _) = tag("FILE")(input)?;
        let (input, _) = take_until("\n")(input)?;
        let (input, _) = line_ending(input)?;
        Ok((input, ""))
    }

    /// Parse TITLE line
    fn parse_title(input: &str) -> IResult<&str, String> {
        let (input, _) = many0(alt((line_ending, space1, Self::parse_comment_line)))(input)?;
        let (input, _) = tag("TITLE")(input)?;
        let (input, _) = space1(input)?;
        let (input, title) = Self::parse_quoted_string(input)?;
        let (input, _) = opt(line_ending)(input)?;
        Ok((input, title))
    }

    /// Parse PERFORMER line
    fn parse_performer(input: &str) -> IResult<&str, String> {
        let (input, _) = many0(alt((line_ending, space1, Self::parse_comment_line)))(input)?;
        let (input, _) = tag("PERFORMER")(input)?;
        let (input, _) = space1(input)?;
        let (input, performer) = Self::parse_quoted_string(input)?;
        let (input, _) = opt(line_ending)(input)?;
        Ok((input, performer))
    }

    /// Parse all TRACK entries
    fn parse_tracks(input: &str) -> IResult<&str, Vec<CueTrack>> {
        many0(Self::parse_track)(input)
    }

    /// Parse a single TRACK entry
    fn parse_track(input: &str) -> IResult<&str, CueTrack> {
        let (input, _) = many0(alt((line_ending, space1, Self::parse_comment_line)))(input)?;
        let (input, _) = tag("TRACK")(input)?;
        let (input, _) = space1(input)?;
        let (input, number) = map_res(digit1, |s: &str| s.parse::<u32>())(input)?;
        let (input, _) = space1(input)?;
        let (input, _) = tag("AUDIO")(input)?;
        let (input, _) = opt(line_ending)(input)?;

        // Parse track title
        let (input, _) = many0(space1)(input)?;
        let (input, _) = tag("TITLE")(input)?;
        let (input, _) = space1(input)?;
        let (input, title) = Self::parse_quoted_string(input)?;
        let (input, _) = opt(line_ending)(input)?;

        // Parse optional performer
        let (input, performer) = opt(preceded(
            tuple((many0(space1), tag("PERFORMER"), space1)),
            terminated(Self::parse_quoted_string, opt(line_ending)),
        ))(input)?;

        // Parse optional INDEX 00 (pre-gap marker) before INDEX 01
        let (input, pregap_time_ms) = opt(|input| {
            let (input, _) = many0(alt((line_ending, space1, Self::parse_comment_line)))(input)?;
            let (input, _) = tag("INDEX")(input)?;
            let (input, _) = space1(input)?;
            let (input, _) = tag("00")(input)?;
            let (input, _) = space1(input)?;
            let (input, pregap_ms) = Self::parse_time(input)?;
            let (input, _) = opt(line_ending)(input)?;
            Ok((input, pregap_ms))
        })(input)?;

        // Skip any whitespace or comments before INDEX 01
        let (input, _) = many0(alt((line_ending, space1, Self::parse_comment_line)))(input)?;

        // Parse INDEX 01 (track start time)
        let (input, _) = tag("INDEX")(input)?;
        let (input, _) = space1(input)?;
        let (input, _) = tag("01")(input)?;
        let (input, _) = space1(input)?;
        let (input, start_time_ms) = Self::parse_time(input)?;
        let (input, _) = opt(line_ending)(input)?;

        Ok((
            input,
            CueTrack {
                number,
                title,
                performer,
                start_time_ms,
                pregap_time_ms,
                end_time_ms: None, // Will be calculated later
            },
        ))
    }

    /// Parse quoted string
    fn parse_quoted_string(input: &str) -> IResult<&str, String> {
        let (input, _) = tag("\"")(input)?;
        let (input, content) = take_until("\"")(input)?;
        let (input, _) = tag("\"")(input)?;
        Ok((input, content.to_string()))
    }

    /// Parse time in MM:SS:FF format and convert to milliseconds
    fn parse_time(input: &str) -> IResult<&str, u64> {
        let (input, minutes) = map_res(digit1, |s: &str| s.parse::<u64>())(input)?;
        let (input, _) = tag(":")(input)?;
        let (input, seconds) = map_res(digit1, |s: &str| s.parse::<u64>())(input)?;
        let (input, _) = tag(":")(input)?;
        let (input, frames) = map_res(digit1, |s: &str| s.parse::<u64>())(input)?;

        // Convert to milliseconds (75 frames per second in CD audio)
        let total_ms = (minutes * 60 * 1000) + (seconds * 1000) + (frames * 1000 / 75);

        Ok((input, total_ms))
    }
}

/// Updates STREAMINFO and injects a seektable into FLAC headers using the metaflac library
///
/// This replaces our manual byte manipulation with a proper library.
pub fn update_headers_with_seektable(
    headers: &[u8],
    new_total_samples: u64,
    seektable: &std::collections::HashMap<u64, u64>,
) -> Result<Vec<u8>, String> {
    use std::io::Cursor;
    use tracing::info;

    // Read the existing headers using metaflac
    let mut cursor = Cursor::new(headers);
    let mut tag = metaflac::Tag::read_from(&mut cursor)
        .map_err(|e| format!("Failed to parse FLAC headers: {}", e))?;

    // Update STREAMINFO with new total_samples
    if let Some(mut streaminfo) = tag.get_streaminfo().cloned() {
        streaminfo.total_samples = new_total_samples;
        tag.set_streaminfo(streaminfo);
        info!(
            "✅ Updated STREAMINFO total_samples to {}",
            new_total_samples
        );
    } else {
        return Err("No STREAMINFO block found".to_string());
    }

    // Remove any existing seektable
    tag.remove_blocks(metaflac::BlockType::SeekTable);

    // Create new seektable from our HashMap
    let mut seekpoints: Vec<(u64, u64)> = seektable.iter().map(|(s, b)| (*s, *b)).collect();
    seekpoints.sort_by_key(|(sample, _)| *sample);

    // Log first, middle, and last seekpoint for debugging
    if !seekpoints.is_empty() {
        let first = seekpoints[0];
        let mid = seekpoints[seekpoints.len() / 2];
        let last = seekpoints[seekpoints.len() - 1];
        info!(
            "📊 Injecting seektable: first sample={} offset={}, mid sample={} offset={}, last sample={} offset={}",
            first.0, first.1, mid.0, mid.1, last.0, last.1
        );
    }

    // Create SeekPoints using from_bytes since the fields are private
    let metaflac_seekpoints: Vec<metaflac::block::SeekPoint> = seekpoints
        .into_iter()
        .map(|(sample, byte_offset)| {
            // Encode as 18 bytes: 8 bytes sample + 8 bytes offset + 2 bytes num_samples
            let mut bytes = Vec::with_capacity(18);
            bytes.extend_from_slice(&sample.to_be_bytes());
            bytes.extend_from_slice(&byte_offset.to_be_bytes());
            bytes.extend_from_slice(&0u16.to_be_bytes()); // num_samples = 0 (unknown)
            metaflac::block::SeekPoint::from_bytes(&bytes)
        })
        .collect();

    let seektable_block = metaflac::Block::SeekTable(metaflac::block::SeekTable {
        seekpoints: metaflac_seekpoints,
    });
    tag.push_block(seektable_block);

    info!(
        "✅ Injected seektable with {} seekpoints using metaflac",
        seektable.len()
    );

    // Write the modified headers to a Vec<u8>
    let mut output = Vec::new();
    tag.write_to(&mut output)
        .map_err(|e| format!("Failed to write FLAC headers: {}", e))?;

    Ok(output)
}

/// Updates the total_samples field in a FLAC STREAMINFO block
///
/// STREAMINFO structure (34 bytes of data after 4-byte header):
/// - Bytes 0-1: min_blocksize (16 bits)
/// - Bytes 2-3: max_blocksize (16 bits)  
/// - Bytes 4-6: min_framesize (24 bits)
/// - Bytes 7-9: max_framesize (24 bits)
/// - Bytes 10-17: sample_rate (20 bits), channels (3 bits), bits_per_sample (5 bits), total_samples (36 bits)
/// - Bytes 18-33: MD5 signature (128 bits)
///
/// The total_samples field is 36 bits starting at bit 64 of the STREAMINFO data.
pub fn update_streaminfo_total_samples(
    headers: &[u8],
    new_total_samples: u64,
) -> Result<Vec<u8>, String> {
    if headers.len() < 42 {
        return Err("Headers too short to contain STREAMINFO".to_string());
    }

    if &headers[0..4] != b"fLaC" {
        return Err("Invalid FLAC headers".to_string());
    }

    let mut result = headers.to_vec();

    // STREAMINFO is always the first metadata block after "fLaC"
    // Block header at bytes 4-7, block data starts at byte 8
    let block_type = headers[4] & 0x7F;
    if block_type != 0 {
        return Err(format!(
            "First block is not STREAMINFO (type {})",
            block_type
        ));
    }

    // total_samples is a 36-bit field within the 64-bit packed field at bytes 10-17
    // It starts at bit 28 of that packed field (after 20 bits sample_rate + 3 bits channels + 5 bits bps)
    // In file bytes: starts at bit 4 of byte 13 (file offset 21 = 4 fLaC + 4 header + 13)
    //
    // Layout:
    // - Byte 13 (file offset 21): upper 4 bits are bits_per_sample, lower 4 bits are total_samples bits 35-32
    // - Bytes 14-17 (file offsets 22-25): total_samples bits 31-0

    let streaminfo_data_offset = 8; // After fLaC + block header
    let byte_13_offset = streaminfo_data_offset + 13; // Byte 13 of STREAMINFO data

    // Read the existing byte 13 to preserve bits_per_sample (upper 4 bits)
    let byte_13_upper = result[byte_13_offset] & 0xF0;

    // Write the 36-bit total_samples value
    // Byte 13: bits_per_sample (upper 4 bits) + total_samples bits 35-32 (lower 4 bits)
    // Byte 14: total_samples bits 31-24
    // Byte 15: total_samples bits 23-16
    // Byte 16: total_samples bits 15-8
    // Byte 17: total_samples bits 7-0
    result[byte_13_offset] = byte_13_upper | (((new_total_samples >> 32) & 0x0F) as u8);
    result[byte_13_offset + 1] = ((new_total_samples >> 24) & 0xFF) as u8;
    result[byte_13_offset + 2] = ((new_total_samples >> 16) & 0xFF) as u8;
    result[byte_13_offset + 3] = ((new_total_samples >> 8) & 0xFF) as u8;
    result[byte_13_offset + 4] = (new_total_samples & 0xFF) as u8;

    use tracing::info;
    info!(
        "✅ Updated STREAMINFO total_samples to {} (was in album FLAC)",
        new_total_samples
    );

    Ok(result)
}

/// Encodes a seektable HashMap into a FLAC SEEKTABLE metadata block
///
/// FLAC SEEKTABLE format:
/// - Block header: 1 byte (type 3 + is_last flag) + 3 bytes (length in big-endian)
/// - Each seekpoint: 8 bytes sample_number + 8 bytes byte_offset + 2 bytes samples_in_frame
///
/// For our purposes, we set samples_in_frame to 0xFFFF (unknown) since we only track
/// the start of frames, not their sample count.
pub fn encode_seektable_block(seektable: &std::collections::HashMap<u64, u64>) -> Vec<u8> {
    use tracing::debug;

    // Sort seekpoints by sample number (required by FLAC spec)
    let mut seekpoints: Vec<(&u64, &u64)> = seektable.iter().collect();
    seekpoints.sort_by_key(|(sample, _)| *sample);

    debug!("Encoding seektable with {} seekpoints", seekpoints.len());

    // Log first few seekpoints for debugging
    if seekpoints.len() > 0 {
        use tracing::info;
        info!(
            "📍 First 5 seekpoints: {:?}",
            seekpoints
                .iter()
                .take(5)
                .map(|(s, b)| (*s, *b))
                .collect::<Vec<_>>()
        );
        if seekpoints.len() > 5 {
            info!(
                "📍 Last 5 seekpoints: {:?}",
                seekpoints
                    .iter()
                    .rev()
                    .take(5)
                    .rev()
                    .map(|(s, b)| (*s, *b))
                    .collect::<Vec<_>>()
            );
        }
    }

    // Calculate block size: 18 bytes per seekpoint
    let num_seekpoints = seekpoints.len();
    let block_data_size = num_seekpoints * 18;

    // Block header: 1 byte type + 3 bytes length
    let mut block = Vec::with_capacity(4 + block_data_size);

    // Type byte: 0x03 (SEEKTABLE), is_last = 0 (we'll update this when injecting)
    block.push(0x03);

    // Length in big-endian (24 bits)
    block.push(((block_data_size >> 16) & 0xFF) as u8);
    block.push(((block_data_size >> 8) & 0xFF) as u8);
    block.push((block_data_size & 0xFF) as u8);

    // Encode each seekpoint
    for (sample_number, byte_offset) in seekpoints {
        // Sample number (8 bytes, big-endian)
        block.extend_from_slice(&sample_number.to_be_bytes());

        // Byte offset (8 bytes, big-endian)
        block.extend_from_slice(&byte_offset.to_be_bytes());

        // Samples in frame (2 bytes) - use 0xFFFF for unknown
        block.push(0xFF);
        block.push(0xFF);
    }

    debug!("Encoded seektable block: {} bytes total", block.len());
    block
}

/// Injects a seektable block into FLAC headers
///
/// This function:
/// 1. Parses the original headers to find metadata blocks
/// 2. Removes any existing SEEKTABLE block (type 3)
/// 3. Inserts the new seektable block after STREAMINFO (type 0)
/// 4. Updates the "is_last" flags appropriately
///
/// Returns the modified headers with the injected seektable.
pub fn inject_seektable_into_headers(original_headers: &[u8], seektable_block: Vec<u8>) -> Vec<u8> {
    use tracing::{debug, warn};

    if original_headers.len() < 8 {
        warn!("Headers too short, returning original");
        return original_headers.to_vec();
    }

    // Verify fLaC marker
    if &original_headers[0..4] != b"fLaC" {
        warn!("Invalid FLAC headers (missing fLaC marker), returning original");
        return original_headers.to_vec();
    }

    let mut result = Vec::with_capacity(original_headers.len() + seektable_block.len());
    result.extend_from_slice(b"fLaC");

    let mut pos = 4;
    let mut streaminfo_block: Option<Vec<u8>> = None;
    let mut other_blocks: Vec<Vec<u8>> = Vec::new();
    let mut found_seektable = false;

    // Parse all metadata blocks
    while pos < original_headers.len() {
        if pos + 4 > original_headers.len() {
            break;
        }

        let header_byte = original_headers[pos];
        let is_last = (header_byte & 0x80) != 0;
        let block_type = header_byte & 0x7F;

        // Read block length (24 bits, big-endian)
        let length = ((original_headers[pos + 1] as usize) << 16)
            | ((original_headers[pos + 2] as usize) << 8)
            | (original_headers[pos + 3] as usize);

        if pos + 4 + length > original_headers.len() {
            warn!("Block extends beyond headers, truncating");
            break;
        }

        // Extract the complete block (header + data)
        let mut block = Vec::with_capacity(4 + length);
        // Clear is_last flag for now, we'll set it properly later
        block.push(block_type);
        block.extend_from_slice(&original_headers[pos + 1..pos + 4 + length]);

        match block_type {
            0 => {
                // STREAMINFO - always first
                debug!("Found STREAMINFO block ({} bytes)", block.len());
                streaminfo_block = Some(block);
            }
            3 => {
                // SEEKTABLE - skip it, we'll inject our own
                debug!("Found existing SEEKTABLE block, will replace");
                found_seektable = true;
            }
            _ => {
                // Other blocks (VORBIS_COMMENT, PICTURE, etc.)
                debug!(
                    "Found metadata block type {} ({} bytes)",
                    block_type,
                    block.len()
                );
                other_blocks.push(block);
            }
        }

        pos += 4 + length;

        if is_last {
            break;
        }
    }

    // Reconstruct headers: STREAMINFO, SEEKTABLE, other blocks
    if let Some(streaminfo) = streaminfo_block {
        result.extend_from_slice(&streaminfo);
        debug!("Added STREAMINFO to new headers");
    } else {
        warn!("No STREAMINFO found, returning original headers");
        return original_headers.to_vec();
    }

    // Add our seektable
    result.extend_from_slice(&seektable_block);
    debug!("Injected seektable block ({} bytes)", seektable_block.len());

    // Verify the STREAMINFO total_samples in the result
    if result.len() >= 26 {
        let byte_21 = result[21];
        let total_samples_in_result = ((byte_21 & 0x0F) as u64) << 32
            | (result[22] as u64) << 24
            | (result[23] as u64) << 16
            | (result[24] as u64) << 8
            | (result[25] as u64);
        use tracing::info;
        info!(
            "🔍 After inject: STREAMINFO total_samples = {} (~{:.1}s)",
            total_samples_in_result,
            total_samples_in_result as f64 / 44100.0
        );
    }

    // Add other blocks
    for block in &other_blocks {
        result.extend_from_slice(block);
    }

    // Update is_last flags
    // The last block should have the is_last flag set
    if !other_blocks.is_empty() {
        // Last block is in other_blocks
        let last_block_pos = result.len() - other_blocks.last().unwrap().len();
        result[last_block_pos] |= 0x80;
    } else {
        // Seektable is the last block
        let seektable_pos = result.len() - seektable_block.len();
        result[seektable_pos] |= 0x80;
    }

    debug!(
        "Final headers: {} bytes (original: {}, {} seektable)",
        result.len(),
        original_headers.len(),
        if found_seektable { "replaced" } else { "added" }
    );

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_time() {
        let result = CueFlacProcessor::parse_time("03:45:12");
        assert!(result.is_ok());
        let (_, time_ms) = result.unwrap();

        // 3 minutes + 45 seconds + 12 frames
        // = 3*60*1000 + 45*1000 + 12*1000/75
        // = 180000 + 45000 + 160
        // = 225160 ms
        assert_eq!(time_ms, 225160);
    }

    #[test]
    fn test_parse_time_zero() {
        let result = CueFlacProcessor::parse_time("00:00:00");
        assert!(result.is_ok());
        let (_, time_ms) = result.unwrap();
        assert_eq!(time_ms, 0);
    }

    #[test]
    fn test_parse_time_large_values() {
        // Test with large minute value (60+ minutes)
        let result = CueFlacProcessor::parse_time("60:35:00");
        assert!(result.is_ok());
        let (_, time_ms) = result.unwrap();
        assert_eq!(time_ms, 60 * 60 * 1000 + 35 * 1000);
    }

    #[test]
    fn test_parse_quoted_string() {
        let result = CueFlacProcessor::parse_quoted_string("\"Test Album\"");
        assert!(result.is_ok());
        let (_, string) = result.unwrap();
        assert_eq!(string, "Test Album");
    }

    #[test]
    fn test_parse_quoted_string_with_special_chars() {
        let result = CueFlacProcessor::parse_quoted_string(
            "\"Track with Sections: i. First Part / ii. Second Part / iii. Third Part\"",
        );
        assert!(result.is_ok());
        let (_, string) = result.unwrap();
        assert_eq!(
            string,
            "Track with Sections: i. First Part / ii. Second Part / iii. Third Part"
        );
    }

    #[test]
    fn test_parse_comment_line() {
        let input = "REM GENRE \"Genre Name\"\n";
        let result = CueFlacProcessor::parse_comment_line(input);
        assert!(result.is_ok());
        let (remaining, _) = result.unwrap();
        assert_eq!(remaining, "");
    }

    #[test]
    fn test_parse_file_line() {
        let input = "FILE \"Artist Name - Album Title.flac\" WAVE\n";
        let result = CueFlacProcessor::parse_file_line(input);
        assert!(result.is_ok());
        let (remaining, _) = result.unwrap();
        assert_eq!(remaining, "");
    }

    #[test]
    fn test_parse_simple_cue_sheet() {
        let cue_content = r#"PERFORMER "Test Artist"
TITLE "Test Album"
FILE "test.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Track 1"
    PERFORMER "Test Artist"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Track 2"
    PERFORMER "Test Artist"
    INDEX 01 03:45:00
"#;

        let result = CueFlacProcessor::parse_cue_content(cue_content);
        assert!(result.is_ok());
        let (_, cue_sheet) = result.unwrap();

        assert_eq!(cue_sheet.title, "Test Album");
        assert_eq!(cue_sheet.performer, "Test Artist");
        assert_eq!(cue_sheet.tracks.len(), 2);
        assert_eq!(cue_sheet.tracks[0].title, "Track 1");
        assert_eq!(cue_sheet.tracks[0].start_time_ms, 0);
        assert_eq!(cue_sheet.tracks[1].title, "Track 2");
        assert_eq!(cue_sheet.tracks[1].start_time_ms, 3 * 60 * 1000 + 45 * 1000);
    }

    #[test]
    fn test_parse_cue_sheet_with_comments() {
        let cue_content = r#"REM GENRE "Genre Name"
REM DATE 2000 / 2004
REM COMMENT "Vinyl Rip by User Name"
PERFORMER "Artist Name"
TITLE "Album Title"
FILE "Artist Name - Album Title.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Track One"
    PERFORMER "Artist Name"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Track Two"
    PERFORMER "Artist Name"
    INDEX 01 03:04:00
"#;

        let result = CueFlacProcessor::parse_cue_content(cue_content);
        assert!(result.is_ok());
        let (_, cue_sheet) = result.unwrap();

        assert_eq!(cue_sheet.title, "Album Title");
        assert_eq!(cue_sheet.performer, "Artist Name");
        assert_eq!(cue_sheet.tracks.len(), 2);
        assert_eq!(cue_sheet.tracks[0].title, "Track One");
        assert_eq!(cue_sheet.tracks[1].title, "Track Two");
    }

    #[test]
    fn test_parse_cue_sheet_with_windows_line_endings() {
        // Windows line endings (\r\n)
        let cue_content = "REM GENRE \"Genre Name\"\r\nPERFORMER \"Test Artist\"\r\nTITLE \"Test Album\"\r\nFILE \"test.flac\" WAVE\r\n  TRACK 01 AUDIO\r\n    TITLE \"Track 1\"\r\n    PERFORMER \"Test Artist\"\r\n    INDEX 01 00:00:00\r\n";

        let result = CueFlacProcessor::parse_cue_content(cue_content);
        assert!(result.is_ok());
        let (_, cue_sheet) = result.unwrap();

        assert_eq!(cue_sheet.title, "Test Album");
        assert_eq!(cue_sheet.performer, "Test Artist");
        assert_eq!(cue_sheet.tracks.len(), 1);
    }

    #[test]
    fn test_parse_cue_sheet_calculates_end_times() {
        let cue_content = r#"PERFORMER "Test Artist"
TITLE "Test Album"
FILE "test.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Track 1"
    PERFORMER "Test Artist"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Track 2"
    PERFORMER "Test Artist"
    INDEX 01 03:00:00
  TRACK 03 AUDIO
    TITLE "Track 3"
    PERFORMER "Test Artist"
    INDEX 01 06:00:00
"#;

        let result = CueFlacProcessor::parse_cue_content(cue_content);
        assert!(result.is_ok());
        let (_, cue_sheet) = result.unwrap();

        // Track 1 should end where Track 2 starts
        assert_eq!(cue_sheet.tracks[0].end_time_ms, Some(3 * 60 * 1000));
        // Track 2 should end where Track 3 starts
        assert_eq!(cue_sheet.tracks[1].end_time_ms, Some(6 * 60 * 1000));
        // Track 3 should have no end time (last track)
        assert_eq!(cue_sheet.tracks[2].end_time_ms, None);
    }

    #[test]
    fn test_parse_cue_sheet_without_per_track_performer() {
        // Some CUE sheets only have album-level performer
        let cue_content = r#"PERFORMER "Test Artist"
TITLE "Test Album"
FILE "test.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Track 1"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Track 2"
    INDEX 01 03:00:00
"#;

        let result = CueFlacProcessor::parse_cue_content(cue_content);
        assert!(result.is_ok());
        let (_, cue_sheet) = result.unwrap();

        assert_eq!(cue_sheet.tracks.len(), 2);
        // Tracks without explicit performer should have None
        assert_eq!(cue_sheet.tracks[0].performer, None);
        assert_eq!(cue_sheet.tracks[1].performer, None);
    }

    #[test]
    fn test_parse_cue_with_index_00_minimal_repro() {
        // Minimal reproduction case: Track with INDEX 00 before INDEX 01
        let cue_content = r#"PERFORMER "Test Artist"
TITLE "Test Album"
FILE "test.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Track 1"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Track 2"
    INDEX 00 03:00:00
    INDEX 01 03:01:00
"#;

        let result = CueFlacProcessor::parse_cue_content(cue_content);
        assert!(result.is_ok());

        let (_, cue_sheet) = result.unwrap();
        assert_eq!(cue_sheet.tracks.len(), 2, "Should parse 2 tracks");
    }

    #[test]
    fn test_parse_cue_with_rem_between_title_and_file() {
        // Test case with REM between TITLE and FILE (common CUE format)
        let cue_content = r#"REM DATE 1970
REM DISCID A1B2C3D4
REM COMMENT "ExactAudioCopy v1.3"
PERFORMER "Test Artist"
TITLE "Test Album"
REM COMPOSER ""
FILE "Test Artist - Test Album.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Track 1"
    PERFORMER "Test Artist"
    REM COMPOSER ""
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Track 2"
    PERFORMER "Test Artist"
    REM COMPOSER ""
    INDEX 01 06:17:53
  TRACK 03 AUDIO
    TITLE "Track 3 With Multiple Sections"
    PERFORMER "Test Artist"
    REM COMPOSER ""
    INDEX 00 10:39:50
    INDEX 01 10:41:28
"#;

        let result = CueFlacProcessor::parse_cue_content(cue_content);
        assert!(
            result.is_ok(),
            "Should parse CUE with REM between TITLE and FILE"
        );

        let (_, cue_sheet) = result.unwrap();
        assert_eq!(cue_sheet.title, "Test Album");
        assert_eq!(cue_sheet.performer, "Test Artist");
        assert_eq!(cue_sheet.tracks.len(), 3, "Should parse 3 tracks");
        assert_eq!(cue_sheet.tracks[0].title, "Track 1");
        assert_eq!(cue_sheet.tracks[1].title, "Track 2");
        assert_eq!(cue_sheet.tracks[2].title, "Track 3 With Multiple Sections");
        assert_eq!(cue_sheet.tracks[0].start_time_ms, 0);
        assert_eq!(
            cue_sheet.tracks[1].start_time_ms,
            6 * 60 * 1000 + 17 * 1000 + 53 * 1000 / 75
        );
    }
}
