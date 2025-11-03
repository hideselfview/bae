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
#[allow(dead_code)]
pub struct CueTrack {
    pub number: u32,
    pub title: String,
    pub performer: Option<String>,
    pub start_time_ms: u64,       // Converted from MM:SS:FF format
    pub end_time_ms: Option<u64>, // Calculated from next track or file end
}

/// Represents a parsed CUE sheet
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CueSheet {
    pub title: String,
    pub performer: String,
    pub tracks: Vec<CueTrack>,
}

/// FLAC header information extracted from file
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FlacHeaders {
    pub headers: Vec<u8>,      // Raw header blocks
    pub audio_start_byte: u64, // Where audio frames begin
    pub sample_rate: u32,
    pub total_samples: u64,
    pub channels: u16,
    pub bits_per_sample: u16,
}

impl FlacHeaders {
    /// Parse STREAMINFO from raw headers to get sample rate
    ///
    /// Helper method to extract just the sample rate from stored headers.
    pub fn parse_sample_rate_from_headers(headers: &[u8]) -> Result<u32, CueFlacError> {
        CueFlacProcessor::parse_streaminfo(headers).map(|(rate, _, _, _)| rate)
    }
}

/// Represents a CUE/FLAC pair found during import
#[derive(Debug, Clone)]
pub struct CueFlacPair {
    pub flac_path: std::path::PathBuf,
    pub cue_path: std::path::PathBuf,
}

/// Main processor for CUE/FLAC operations
pub struct CueFlacProcessor;

/// Blocking strategy for FLAC frames
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockingStrategy {
    Fixed,    // Frame number in header
    Variable, // Sample number in header
}

/// Parsed FLAC frame header information
#[derive(Debug, Clone)]
struct FrameHeader {
    blocking_strategy: BlockingStrategy,
    block_size: u16,
    sample_rate: u32,
    channels: u8,
    bits_per_sample: u8,
    frame_or_sample_number: u64, // Frame number (fixed) or sample number (variable)
    header_size: usize,          // Total header size in bytes
}

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

        // Parse STREAMINFO from the headers to get audio properties
        let (sample_rate, total_samples, channels, bits_per_sample) =
            Self::parse_streaminfo(&headers)?;

        Ok(FlacHeaders {
            headers,
            audio_start_byte,
            sample_rate,
            total_samples,
            channels,
            bits_per_sample,
        })
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

    /// Parse STREAMINFO block from FLAC headers
    pub(crate) fn parse_streaminfo(headers: &[u8]) -> Result<(u32, u64, u16, u16), CueFlacError> {
        // Skip "fLaC" signature
        if headers.len() < 4 || &headers[0..4] != b"fLaC" {
            return Err(CueFlacError::Flac("Invalid FLAC signature".to_string()));
        }

        let mut pos = 4;

        // Find STREAMINFO block (type 0)
        loop {
            if pos + 4 > headers.len() {
                return Err(CueFlacError::Flac("STREAMINFO block not found".to_string()));
            }

            let header = u32::from_be_bytes([
                headers[pos],
                headers[pos + 1],
                headers[pos + 2],
                headers[pos + 3],
            ]);

            let is_last = (header & 0x80000000) != 0;
            let block_type = (header >> 24) & 0x7F;
            let block_size = (header & 0x00FFFFFF) as usize;

            pos += 4; // Skip header

            if block_type == 0 {
                // STREAMINFO
                if pos + 34 > headers.len() {
                    return Err(CueFlacError::Flac(
                        "Invalid STREAMINFO block size".to_string(),
                    ));
                }

                // Parse STREAMINFO fields (34 bytes total)
                let sample_rate = (u32::from_be_bytes([
                    0,
                    headers[pos + 10],
                    headers[pos + 11],
                    headers[pos + 12],
                ]) >> 4)
                    & 0xFFFFF;
                let channels = ((headers[pos + 12] >> 1) & 0x07) as u16 + 1;
                let bits_per_sample =
                    (((headers[pos + 12] & 0x01) << 4) | ((headers[pos + 13] >> 4) & 0x0F)) as u16
                        + 1;

                // Total samples (36 bits)
                let total_samples = ((headers[pos + 13] as u64 & 0x0F) << 32)
                    | (u32::from_be_bytes([
                        headers[pos + 14],
                        headers[pos + 15],
                        headers[pos + 16],
                        headers[pos + 17],
                    ]) as u64);

                return Ok((sample_rate, total_samples, channels, bits_per_sample));
            }

            pos += block_size; // Skip block data

            if is_last {
                break;
            }
        }

        Err(CueFlacError::Flac("STREAMINFO block not found".to_string()))
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
        let mut tracks_with_end_times = tracks;
        for i in 0..tracks_with_end_times.len() {
            if i + 1 < tracks_with_end_times.len() {
                tracks_with_end_times[i].end_time_ms =
                    Some(tracks_with_end_times[i + 1].start_time_ms);
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

    /// Parse and skip an INDEX 00 line (pre-gap marker)
    /// These are optional and appear before INDEX 01 to indicate pre-gap silence
    fn parse_index_00_line(input: &str) -> IResult<&str, &str> {
        let (input, _) = many0(alt((line_ending, space1, Self::parse_comment_line)))(input)?;
        let (input, _) = tag("INDEX")(input)?;
        let (input, _) = space1(input)?;
        let (input, _) = tag("00")(input)?;
        let (input, _) = space1(input)?;
        let (input, _) = Self::parse_time(input)?; // Skip the time value
        let (input, _) = opt(line_ending)(input)?;
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

        // Skip any optional INDEX 00 entries (pre-gap markers) before INDEX 01
        let (input, _) = many0(Self::parse_index_00_line)(input)?;

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

    /// Decode a UTF-8 coded number from FLAC frame header
    ///
    /// FLAC uses modified UTF-8 encoding for variable-length integers.
    /// Returns the decoded number and the number of bytes consumed.
    pub(crate) fn decode_utf8_number(data: &[u8]) -> Result<(u64, usize), CueFlacError> {
        if data.is_empty() {
            return Err(CueFlacError::Flac(
                "Empty data for UTF-8 decode".to_string(),
            ));
        }

        let first_byte = data[0];

        if (first_byte & 0x80) == 0 {
            // 1 byte: 0xxxxxxx (0-127)
            Ok((first_byte as u64, 1))
        } else if (first_byte & 0xE0) == 0xC0 {
            // 2 bytes: 110xxxxx 10xxxxxx
            if data.len() < 2 {
                return Err(CueFlacError::Flac("Incomplete UTF-8 sequence".to_string()));
            }
            let value = ((first_byte & 0x1F) as u64) << 6 | ((data[1] & 0x3F) as u64);
            Ok((value, 2))
        } else if (first_byte & 0xF0) == 0xE0 {
            // 3 bytes: 1110xxxx 10xxxxxx 10xxxxxx
            if data.len() < 3 {
                return Err(CueFlacError::Flac("Incomplete UTF-8 sequence".to_string()));
            }
            let value = ((first_byte & 0x0F) as u64) << 12
                | ((data[1] & 0x3F) as u64) << 6
                | ((data[2] & 0x3F) as u64);
            Ok((value, 3))
        } else if (first_byte & 0xF8) == 0xF0 {
            // 4 bytes: 11110xxx 10xxxxxx 10xxxxxx 10xxxxxx
            if data.len() < 4 {
                return Err(CueFlacError::Flac("Incomplete UTF-8 sequence".to_string()));
            }
            let value = ((first_byte & 0x07) as u64) << 18
                | ((data[1] & 0x3F) as u64) << 12
                | ((data[2] & 0x3F) as u64) << 6
                | ((data[3] & 0x3F) as u64);
            Ok((value, 4))
        } else if (first_byte & 0xFC) == 0xF8 {
            // 5 bytes: 111110xx 10xxxxxx 10xxxxxx 10xxxxxx 10xxxxxx
            if data.len() < 5 {
                return Err(CueFlacError::Flac("Incomplete UTF-8 sequence".to_string()));
            }
            let value = ((first_byte & 0x03) as u64) << 24
                | ((data[1] & 0x3F) as u64) << 18
                | ((data[2] & 0x3F) as u64) << 12
                | ((data[3] & 0x3F) as u64) << 6
                | ((data[4] & 0x3F) as u64);
            Ok((value, 5))
        } else if (first_byte & 0xFE) == 0xFC {
            // 6 bytes: 1111110x 10xxxxxx 10xxxxxx 10xxxxxx 10xxxxxx 10xxxxxx
            if data.len() < 6 {
                return Err(CueFlacError::Flac("Incomplete UTF-8 sequence".to_string()));
            }
            let value = ((first_byte & 0x01) as u64) << 30
                | ((data[1] & 0x3F) as u64) << 24
                | ((data[2] & 0x3F) as u64) << 18
                | ((data[3] & 0x3F) as u64) << 12
                | ((data[4] & 0x3F) as u64) << 6
                | ((data[5] & 0x3F) as u64);
            Ok((value, 6))
        } else {
            Err(CueFlacError::Flac(format!(
                "Invalid UTF-8 start byte: 0x{:02x}",
                first_byte
            )))
        }
    }

    /// Encode a number in FLAC's UTF-8 format
    ///
    /// Returns the UTF-8 encoded bytes for the given number.
    pub(crate) fn encode_utf8_number(value: u64) -> Vec<u8> {
        if value < 128 {
            // 1 byte: 0xxxxxxx
            vec![value as u8]
        } else if value < 2048 {
            // 2 bytes: 110xxxxx 10xxxxxx
            vec![
                0xC0 | ((value >> 6) & 0x1F) as u8,
                0x80 | (value & 0x3F) as u8,
            ]
        } else if value < 65536 {
            // 3 bytes: 1110xxxx 10xxxxxx 10xxxxxx
            vec![
                0xE0 | ((value >> 12) & 0x0F) as u8,
                0x80 | ((value >> 6) & 0x3F) as u8,
                0x80 | (value & 0x3F) as u8,
            ]
        } else if value < 2097152 {
            // 4 bytes: 11110xxx 10xxxxxx 10xxxxxx 10xxxxxx
            vec![
                0xF0 | ((value >> 18) & 0x07) as u8,
                0x80 | ((value >> 12) & 0x3F) as u8,
                0x80 | ((value >> 6) & 0x3F) as u8,
                0x80 | (value & 0x3F) as u8,
            ]
        } else if value < 67108864 {
            // 5 bytes: 111110xx 10xxxxxx 10xxxxxx 10xxxxxx 10xxxxxx
            vec![
                0xF8 | ((value >> 24) & 0x03) as u8,
                0x80 | ((value >> 18) & 0x3F) as u8,
                0x80 | ((value >> 12) & 0x3F) as u8,
                0x80 | ((value >> 6) & 0x3F) as u8,
                0x80 | (value & 0x3F) as u8,
            ]
        } else {
            // 6 bytes: 1111110x 10xxxxxx 10xxxxxx 10xxxxxx 10xxxxxx 10xxxxxx
            vec![
                0xFC | ((value >> 30) & 0x01) as u8,
                0x80 | ((value >> 24) & 0x3F) as u8,
                0x80 | ((value >> 18) & 0x3F) as u8,
                0x80 | ((value >> 12) & 0x3F) as u8,
                0x80 | ((value >> 6) & 0x3F) as u8,
                0x80 | (value & 0x3F) as u8,
            ]
        }
    }

    /// Find the nearest FLAC frame boundary near the given byte position
    ///
    /// FLAC frames start with a sync code (0xFF followed by a byte with upper 6 bits = 111110).
    /// This function searches forward and backward from the estimated position to find
    /// the nearest valid frame boundary.
    fn find_nearest_frame_boundary(
        file_data: &[u8],
        estimated_pos: u64,
        audio_start_byte: u64,
    ) -> Result<u64, CueFlacError> {
        let estimated_pos = estimated_pos as usize;

        // Ensure we're searching within audio data (after headers)
        if estimated_pos < audio_start_byte as usize {
            return Ok(audio_start_byte);
        }

        // Search window: look up to 64KB forward and backward
        // (FLAC frames are typically much smaller, but this gives us a reasonable window)
        let search_window = 64 * 1024;
        let start_search = audio_start_byte as usize;
        let end_search = file_data.len();

        // Search backward from estimated position
        let backward_start = estimated_pos
            .saturating_sub(search_window)
            .max(start_search);
        let forward_end = (estimated_pos + search_window).min(end_search);

        // Find the nearest sync code before the estimated position
        let mut best_backward: Option<usize> = None;
        for i in (backward_start..estimated_pos).rev() {
            // FLAC sync code: 0xFF followed by byte with upper 6 bits = 111110 (0xFC..0xFF)
            if i + 1 < file_data.len()
                && file_data[i] == 0xFF
                && (file_data[i + 1] & 0xFC) == 0xFC
                && Self::validate_flac_frame_header(file_data, i)
            {
                best_backward = Some(i);
                break;
            }
        }

        // Find the nearest sync code after the estimated position
        let mut best_forward: Option<usize> = None;
        for i in estimated_pos..forward_end {
            if i + 1 < file_data.len()
                && file_data[i] == 0xFF
                && (file_data[i + 1] & 0xFC) == 0xFC
                && Self::validate_flac_frame_header(file_data, i)
            {
                best_forward = Some(i);
                break;
            }
        }

        // Choose the closer one
        match (best_backward, best_forward) {
            (Some(backward), Some(forward)) => {
                let backward_dist = estimated_pos - backward;
                let forward_dist = forward - estimated_pos;
                Ok(if backward_dist <= forward_dist {
                    backward
                } else {
                    forward
                } as u64)
            }
            (Some(backward), None) => Ok(backward as u64),
            (None, Some(forward)) => Ok(forward as u64),
            (None, None) => {
                // No frame found - fall back to estimated position
                // This shouldn't happen in valid FLAC files, but handle gracefully
                Ok(estimated_pos as u64)
            }
        }
    }

    /// Basic validation of a FLAC frame header at the given position
    fn validate_flac_frame_header(file_data: &[u8], pos: usize) -> bool {
        if pos + 4 >= file_data.len() {
            return false;
        }

        // Check sync code: 0xFF followed by byte with upper 6 bits = 111110
        if file_data[pos] != 0xFF || (file_data[pos + 1] & 0xFC) != 0xFC {
            return false;
        }

        // Basic sanity check: the frame header should be reasonable
        // We can't fully validate without parsing the variable-length header,
        // but we can check that we're not reading out of bounds
        true
    }

    /// Get accurate byte position aligned to FLAC frame boundaries
    ///
    /// This function estimates the byte position from time, then finds the nearest
    /// FLAC frame boundary to ensure we don't cut frames in the middle.
    pub fn byte_position(
        flac_path: &Path,
        time_ms: u64,
        flac_headers: &FlacHeaders,
        file_size: u64,
    ) -> Result<u64, CueFlacError> {
        if flac_headers.total_samples == 0 {
            return Ok(flac_headers.audio_start_byte);
        }

        let total_duration_ms =
            (flac_headers.total_samples * 1000) / flac_headers.sample_rate as u64;
        if total_duration_ms == 0 {
            return Ok(flac_headers.audio_start_byte);
        }

        // Estimate position using linear interpolation
        let audio_size = file_size - flac_headers.audio_start_byte;
        let estimated_audio_byte = (time_ms * audio_size) / total_duration_ms;
        let estimated_pos = flac_headers.audio_start_byte + estimated_audio_byte;

        // Read only a window around the estimated position (64KB search window on each side)
        // This avoids loading entire large FLAC files into memory
        let search_window = 64 * 1024;
        let read_start = estimated_pos
            .saturating_sub(search_window)
            .max(flac_headers.audio_start_byte);
        let read_end = (estimated_pos + search_window).min(file_size);

        // Read the window
        let mut file = std::fs::File::open(flac_path)?;
        let mut file_data = vec![0u8; (read_end - read_start) as usize];
        use std::io::{Read, Seek, SeekFrom};
        file.seek(SeekFrom::Start(read_start))?;
        file.read_exact(&mut file_data)?;

        // Adjust estimated position to be relative to the window we read
        let estimated_pos_in_window = estimated_pos - read_start;

        // Find nearest frame boundary (returns position relative to window)
        let frame_pos_in_window = Self::find_nearest_frame_boundary(
            &file_data,
            estimated_pos_in_window,
            0, // audio_start is at position 0 in our window
        )?;

        // Convert back to absolute file position
        Ok(read_start + frame_pos_in_window)
    }

    /// Generate corrected FLAC headers for a track with the track's actual duration
    ///
    /// Takes the original album FLAC headers and creates clean headers with:
    /// - STREAMINFO block with corrected total_samples, zeroed MD5, and zeroed min/max frame sizes
    /// - All other metadata blocks removed (SEEKTABLE, VORBIS_COMMENT, PADDING, APPLICATION)
    ///
    /// We remove all metadata except STREAMINFO. Track metadata lives in the database.
    /// SEEKTABLE would need rebuilding with correct offsets. Tags can be added later if needed.
    ///
    /// # Arguments
    /// * `original_headers` - The FLAC headers from the original album file
    /// * `track_duration_ms` - The track's duration in milliseconds
    /// * `sample_rate` - The audio sample rate (e.g., 44100 Hz)
    ///
    /// # Returns
    /// Cleaned FLAC headers containing only STREAMINFO with correct track-specific values
    pub fn write_corrected_flac_headers(
        original_headers: &[u8],
        track_duration_ms: i64,
        sample_rate: u32,
    ) -> Result<Vec<u8>, CueFlacError> {
        // Validate FLAC signature
        if original_headers.len() < 4 || &original_headers[0..4] != b"fLaC" {
            return Err(CueFlacError::Flac("Invalid FLAC signature".to_string()));
        }

        let mut pos = 4;
        let mut streaminfo_data: Option<Vec<u8>> = None;

        // Find and extract STREAMINFO block (type 0) - it's always the first metadata block
        loop {
            if pos + 4 > original_headers.len() {
                return Err(CueFlacError::Flac("STREAMINFO block not found".to_string()));
            }

            let header = u32::from_be_bytes([
                original_headers[pos],
                original_headers[pos + 1],
                original_headers[pos + 2],
                original_headers[pos + 3],
            ]);

            let is_last = (header & 0x80000000) != 0;
            let block_type = (header >> 24) & 0x7F;
            let block_size = (header & 0x00FFFFFF) as usize;

            pos += 4; // Skip header

            if block_type == 0 {
                // STREAMINFO block found
                if pos + 34 > original_headers.len() {
                    return Err(CueFlacError::Flac(
                        "Invalid STREAMINFO block size".to_string(),
                    ));
                }

                // Extract STREAMINFO data (34 bytes)
                streaminfo_data = Some(original_headers[pos..pos + 34].to_vec());
                break;
            }

            pos += block_size; // Skip block data

            if is_last {
                break;
            }
        }

        let mut streaminfo = streaminfo_data
            .ok_or_else(|| CueFlacError::Flac("STREAMINFO block not found".to_string()))?;

        // Calculate correct total_samples for the track
        let track_total_samples = ((track_duration_ms as u64) * sample_rate as u64) / 1000;

        // Update total_samples in STREAMINFO (bytes 13-17)
        // Byte 13: preserve upper 4 bits (bits_per_sample info), set lower 4 bits to high 4 bits of total_samples
        let byte13_mask = streaminfo[13] & 0xF0; // Preserve upper 4 bits
        let total_samples_high_4_bits = ((track_total_samples >> 32) & 0x0F) as u8;
        streaminfo[13] = byte13_mask | total_samples_high_4_bits;

        // Bytes 14-17: 32-bit big-endian representation of lower 32 bits
        let total_samples_low_32 = (track_total_samples & 0xFFFFFFFF) as u32;
        let bytes_14_17 = total_samples_low_32.to_be_bytes();
        streaminfo[14] = bytes_14_17[0];
        streaminfo[15] = bytes_14_17[1];
        streaminfo[16] = bytes_14_17[2];
        streaminfo[17] = bytes_14_17[3];

        // Zero min_frame_size (bytes 4-6) - signals "unknown"
        streaminfo[4] = 0;
        streaminfo[5] = 0;
        streaminfo[6] = 0;

        // Zero max_frame_size (bytes 7-9) - signals "unknown"
        streaminfo[7] = 0;
        streaminfo[8] = 0;
        streaminfo[9] = 0;

        // Zero MD5 signature (bytes 18-33) - signals "no signature"
        for i in 18..34 {
            streaminfo[i] = 0;
        }

        // Build new header structure: "fLaC" + STREAMINFO block (marked as last)
        let mut cleaned_headers = vec![b'f', b'L', b'a', b'C'];

        // STREAMINFO block header: type 0, size 34, last block flag set
        let block_header: u32 = 0x80000000 | (0 << 24) | 34; // Last block, type 0, size 34
        cleaned_headers.extend_from_slice(&block_header.to_be_bytes());
        cleaned_headers.extend_from_slice(&streaminfo);

        Ok(cleaned_headers)
    }

    /// Calculate CRC-8 checksum for FLAC frame headers
    ///
    /// Uses polynomial 0x07. Processes all header bytes except the CRC byte itself.
    pub(crate) fn calculate_crc8(data: &[u8]) -> u8 {
        let mut crc: u8 = 0;
        for &byte in data {
            crc ^= byte;
            for _ in 0..8 {
                if crc & 0x80 != 0 {
                    crc = (crc << 1) ^ 0x07;
                } else {
                    crc <<= 1;
                }
            }
        }
        crc
    }

    /// Parse a FLAC frame header
    ///
    /// Validates sync code, parses all header fields, and returns parsed information.
    fn parse_frame_header(data: &[u8]) -> Result<FrameHeader, CueFlacError> {
        // Minimum frame header size is about 10 bytes (before variable-length number)
        if data.len() < 10 {
            return Err(CueFlacError::Flac("Frame header too short".to_string()));
        }

        // Validate sync code (bits 0-13 = 0x3FFE)
        let sync = u16::from_be_bytes([data[0], data[1]]);
        if (sync & 0xFFFC) != 0xFFF8 {
            return Err(CueFlacError::Flac(format!(
                "Invalid sync code: 0x{:04x}",
                sync
            )));
        }

        // Parse blocking strategy (bit 15)
        let blocking_strategy = if (data[1] & 0x01) == 0 {
            BlockingStrategy::Fixed
        } else {
            BlockingStrategy::Variable
        };

        // Track position through header (starts at 4 after sync+metadata bytes)
        let mut pos = 4;

        // Parse block size (bits 16-19)
        let block_size_code = (data[2] >> 4) & 0x0F;
        let block_size = if block_size_code == 0 {
            return Err(CueFlacError::Flac("Reserved block size code 0".to_string()));
        } else if block_size_code == 6 {
            // Read 8-bit value from next byte + 1
            if data.len() < pos + 1 {
                return Err(CueFlacError::Flac(
                    "Incomplete block size extension".to_string(),
                ));
            }
            let size = (data[pos] as u16) + 1;
            pos += 1;
            size
        } else if block_size_code == 7 {
            // Read 16-bit value from next 2 bytes + 1
            if data.len() < pos + 2 {
                return Err(CueFlacError::Flac(
                    "Incomplete block size extension".to_string(),
                ));
            }
            let size = u16::from_be_bytes([data[pos], data[pos + 1]]) + 1;
            pos += 2;
            size
        } else {
            // Lookup table for common block sizes
            match block_size_code {
                1 => 192,
                2 => 576,
                3 => 1152,
                4 => 2304,
                5 => 4608,
                8 => 256,
                9 => 512,
                10 => 1024,
                11 => 2048,
                12 => 4096,
                13 => 8192,
                14 => 16384,
                15 => 32768,
                _ => {
                    return Err(CueFlacError::Flac(format!(
                        "Invalid block size code: {}",
                        block_size_code
                    )))
                }
            }
        };

        // Parse sample rate (bits 20-23)
        let sample_rate_code = data[2] & 0x0F;
        let sample_rate = if sample_rate_code == 12 {
            // Read 8-bit value * 1000
            if data.len() < pos + 1 {
                return Err(CueFlacError::Flac(
                    "Incomplete sample rate extension".to_string(),
                ));
            }
            let rate = (data[pos] as u32) * 1000;
            pos += 1;
            rate
        } else if sample_rate_code == 13 {
            // Read 16-bit value
            if data.len() < pos + 2 {
                return Err(CueFlacError::Flac(
                    "Incomplete sample rate extension".to_string(),
                ));
            }
            let rate = u16::from_be_bytes([data[pos], data[pos + 1]]) as u32;
            pos += 2;
            rate
        } else if sample_rate_code == 14 {
            // Read 16-bit value * 10
            if data.len() < pos + 2 {
                return Err(CueFlacError::Flac(
                    "Incomplete sample rate extension".to_string(),
                ));
            }
            let rate = (u16::from_be_bytes([data[pos], data[pos + 1]]) as u32) * 10;
            pos += 2;
            rate
        } else if sample_rate_code == 15 {
            return Err(CueFlacError::Flac(
                "Invalid sample rate code 15".to_string(),
            ));
        } else {
            // Common rates lookup
            match sample_rate_code {
                0 => return Err(CueFlacError::Flac("Invalid sample rate code 0".to_string())),
                1 => 88200,
                2 => 176400,
                3 => 192000,
                4 => 8000,
                5 => 16000,
                6 => 22050,
                7 => 24000,
                8 => 32000,
                9 => 44100,
                10 => 48000,
                11 => 96000,
                _ => unreachable!(),
            }
        };

        // Parse channel assignment (bits 24-27)
        let channel_code = (data[3] >> 4) & 0x0F;
        let channels = match channel_code {
            0..=7 => channel_code as u8 + 1,
            8 => 2,  // Left + Side
            9 => 2,  // Right + Side
            10 => 2, // Mid + Side
            11 => 3, // Left + Right + Side
            12 => 4, // Left + Right + Mid + Side
            _ => {
                return Err(CueFlacError::Flac(format!(
                    "Invalid channel assignment: {}",
                    channel_code
                )))
            }
        };

        // Parse sample size (bits 28-30)
        let sample_size_code = ((data[3] & 0x0E) >> 1) as u8;
        let bits_per_sample = if sample_size_code == 0 {
            // Get from STREAMINFO (we don't have that here, so this is an error)
            return Err(CueFlacError::Flac(
                "Sample size from STREAMINFO not supported".to_string(),
            ));
        } else if sample_size_code >= 4 && sample_size_code <= 7 {
            // Reserved
            return Err(CueFlacError::Flac(format!(
                "Reserved sample size code: {}",
                sample_size_code
            )));
        } else {
            // 8, 12, 16, 20, 24 bits
            match sample_size_code {
                1 => 8,
                2 => 12,
                4 => 16,
                5 => 20,
                6 => 24,
                _ => unreachable!(),
            }
        };

        // Decode frame/sample number (UTF-8) at current position
        if data.len() < pos + 1 {
            return Err(CueFlacError::Flac(
                "Frame header too short for number".to_string(),
            ));
        }
        let (frame_or_sample_number, num_bytes) = Self::decode_utf8_number(&data[pos..])?;
        pos += num_bytes;

        // Parse CRC-8 (last byte of header)
        if data.len() < pos + 1 {
            return Err(CueFlacError::Flac(
                "Frame header too short for CRC".to_string(),
            ));
        }
        let header_size = pos + 1;
        let header_without_crc = &data[..header_size - 1];
        let expected_crc = data[header_size - 1];
        let calculated_crc = Self::calculate_crc8(header_without_crc);

        if calculated_crc != expected_crc {
            return Err(CueFlacError::Flac(format!(
                "Frame header CRC mismatch: expected 0x{:02x}, got 0x{:02x}",
                expected_crc, calculated_crc
            )));
        }

        Ok(FrameHeader {
            blocking_strategy,
            block_size,
            sample_rate,
            channels,
            bits_per_sample,
            frame_or_sample_number,
            header_size,
        })
    }

    /// Rewrite a single FLAC frame header with new frame/sample number
    ///
    /// Takes a frame's data (header + body), parses the header, adjusts the number,
    /// and rebuilds the header with new CRC-8.
    fn rewrite_frame_header(
        frame_data: &[u8],
        track_start_sample: u64,
        track_start_frame: u64,
    ) -> Result<Vec<u8>, CueFlacError> {
        // Parse existing header
        let header = Self::parse_frame_header(frame_data)?;

        // Calculate new number
        let new_number = match header.blocking_strategy {
            BlockingStrategy::Fixed => {
                if header.frame_or_sample_number < track_start_frame {
                    return Err(CueFlacError::Flac(format!(
                        "Frame number {} is before track start {}",
                        header.frame_or_sample_number, track_start_frame
                    )));
                }
                header.frame_or_sample_number - track_start_frame
            }
            BlockingStrategy::Variable => {
                if header.frame_or_sample_number < track_start_sample {
                    return Err(CueFlacError::Flac(format!(
                        "Sample number {} is before track start {}",
                        header.frame_or_sample_number, track_start_sample
                    )));
                }
                header.frame_or_sample_number - track_start_sample
            }
        };

        // Encode new number
        let new_number_bytes = Self::encode_utf8_number(new_number);

        // Rebuild header
        // Start with sync code and blocking strategy
        let mut new_header = vec![0xFF, 0xF8];
        if header.blocking_strategy == BlockingStrategy::Variable {
            new_header[1] |= 0x01;
        }

        // Block size code - encode back to FLAC code
        let (block_size_code, block_size_ext): (u8, Option<Vec<u8>>) = match header.block_size {
            192 => (1, None),
            576 => (2, None),
            1152 => (3, None),
            2304 => (4, None),
            4608 => (5, None),
            256 => (8, None),
            512 => (9, None),
            1024 => (10, None),
            2048 => (11, None),
            4096 => (12, None),
            8192 => (13, None),
            16384 => (14, None),
            32768 => (15, None),
            size if size < 256 && size > 0 => {
                // Use code 6 with 8-bit extension
                (6, Some(vec![(size - 1) as u8]))
            }
            size if size > 256 && size <= 65535 => {
                // Use code 7 with 16-bit extension
                (7, Some((size - 1).to_be_bytes().to_vec()))
            }
            _ => {
                return Err(CueFlacError::Flac(format!(
                    "Unsupported block size: {}",
                    header.block_size
                )));
            }
        };

        // Encode sample rate - try common codes first, then extensions
        let (sample_rate_code, sample_rate_ext): (u8, Option<Vec<u8>>) = match header.sample_rate {
            88200 => (1, None),
            176400 => (2, None),
            192000 => (3, None),
            8000 => (4, None),
            16000 => (5, None),
            22050 => (6, None),
            24000 => (7, None),
            32000 => (8, None),
            44100 => (9, None),
            48000 => (10, None),
            96000 => (11, None),
            rate if rate % 1000 == 0 && rate >= 1000 && rate <= 255000 => {
                (12, Some(vec![(rate / 1000) as u8]))
            }
            rate if rate <= 65535 => (13, Some((rate as u16).to_be_bytes().to_vec())),
            rate if rate % 10 == 0 && rate <= 655350 => {
                (14, Some(((rate / 10) as u16).to_be_bytes().to_vec()))
            }
            _ => {
                return Err(CueFlacError::Flac(format!(
                    "Unsupported sample rate: {}",
                    header.sample_rate
                )));
            }
        };

        new_header.push((block_size_code << 4) | sample_rate_code);

        // Channel assignment and sample size
        let channel_code = if header.channels >= 1 && header.channels <= 8 {
            (header.channels - 1) as u8
        } else {
            return Err(CueFlacError::Flac(format!(
                "Unsupported channel count: {}",
                header.channels
            )));
        };

        let sample_size_code = match header.bits_per_sample {
            8 => 1,
            12 => 2,
            16 => 4,
            20 => 5,
            24 => 6,
            _ => {
                return Err(CueFlacError::Flac(format!(
                    "Unsupported sample size: {}",
                    header.bits_per_sample
                )))
            }
        };

        new_header.push((channel_code << 4) | (sample_size_code << 1));

        // Add block size extensions if needed
        if let Some(ext) = block_size_ext {
            new_header.extend_from_slice(&ext);
        }

        // Add sample rate extensions if needed
        if let Some(ext) = sample_rate_ext {
            new_header.extend_from_slice(&ext);
        }

        // Add frame/sample number (UTF-8)
        new_header.extend_from_slice(&new_number_bytes);

        // Calculate and append CRC-8
        let crc = Self::calculate_crc8(&new_header);
        new_header.push(crc);

        // Append frame body (everything after original header)
        let mut result = new_header;
        result.extend_from_slice(&frame_data[header.header_size..]);

        Ok(result)
    }

    /// Rewrite all frame headers in audio data
    ///
    /// Scans for FLAC frame boundaries and rewrites each header with adjusted numbers.
    pub fn rewrite_all_frame_headers(
        audio_data: &[u8],
        track_start_sample: u64,
        track_start_frame: u64,
        _sample_rate: u32, // Reserved for future use
        _block_size: u16,  // Reserved for future use
    ) -> Result<Vec<u8>, CueFlacError> {
        let mut result = Vec::new();
        let mut pos = 0;

        while pos < audio_data.len() {
            // Look for frame sync code
            if pos + 2 > audio_data.len() {
                break;
            }

            if audio_data[pos] == 0xFF && (audio_data[pos + 1] & 0xFC) == 0xFC {
                // Found potential frame start
                // Try to parse header
                if let Ok(header) = Self::parse_frame_header(&audio_data[pos..]) {
                    // Found valid frame, rewrite it

                    // Determine frame size (we need to parse the frame to know its size)
                    // For now, we'll look for the next frame sync code
                    let mut frame_size = header.header_size;
                    let mut search_pos = pos + header.header_size;

                    // Search for next frame or end of data
                    while search_pos + 2 < audio_data.len() {
                        if audio_data[search_pos] == 0xFF
                            && (audio_data[search_pos + 1] & 0xFC) == 0xFC
                        {
                            // Found next frame
                            frame_size = search_pos - pos;
                            break;
                        }
                        search_pos += 1;
                    }

                    // If we didn't find next frame, use remaining data
                    if frame_size == header.header_size {
                        frame_size = audio_data.len() - pos;
                    }

                    let frame_data = &audio_data[pos..pos + frame_size];
                    let rewritten = Self::rewrite_frame_header(
                        frame_data,
                        track_start_sample,
                        track_start_frame,
                    )?;
                    result.extend_from_slice(&rewritten);
                    pos += frame_size;
                } else {
                    // Not a valid frame, copy byte and continue
                    result.push(audio_data[pos]);
                    pos += 1;
                }
            } else {
                // Not a frame sync code, copy byte and continue
                result.push(audio_data[pos]);
                pos += 1;
            }
        }

        Ok(result)
    }
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
    fn test_write_corrected_flac_headers() {
        // Basic smoke test: verify function doesn't error and preserves signature
        // Full correctness testing would require real FLAC file headers

        // Create minimal valid FLAC headers structure
        let mut original_headers = vec![
            b'f', b'L', b'a', b'C', // FLAC signature
            0x00, 0x00, 0x00, 0x22, // STREAMINFO: type 0, size 34 (0x22), last block
        ];

        // STREAMINFO data (34 bytes) - minimal valid structure
        original_headers.resize(42, 0); // 4 + 4 + 34 = 42 bytes

        // Set total_samples to a known value at correct offset
        // STREAMINFO offset 13 (byte 8+13=21): lower 4 bits of byte 21 + bytes 22-25
        let album_total_samples: u64 = 44100 * 120; // 2 minutes
        let streaminfo_start = 8;

        // Byte 13: preserve upper 4 bits, set lower 4 to high 4 bits of total_samples
        original_headers[streaminfo_start + 13] = 0xF0 | ((album_total_samples >> 32) & 0x0F) as u8;
        // Bytes 14-17: lower 32 bits of total_samples
        let samples_low_32 = (album_total_samples & 0xFFFFFFFF) as u32;
        let bytes = samples_low_32.to_be_bytes();
        original_headers[streaminfo_start + 14] = bytes[0];
        original_headers[streaminfo_start + 15] = bytes[1];
        original_headers[streaminfo_start + 16] = bytes[2];
        original_headers[streaminfo_start + 17] = bytes[3];

        // Test correction for 60-second track
        let track_duration_ms = 60 * 1000;
        let sample_rate = 44100;

        let corrected = CueFlacProcessor::write_corrected_flac_headers(
            &original_headers,
            track_duration_ms as i64,
            sample_rate,
        )
        .unwrap();

        // Verify signature preserved
        assert_eq!(&corrected[0..4], b"fLaC");

        // Verify total_samples was modified
        let expected_samples: u64 = (track_duration_ms as u64 * sample_rate as u64) / 1000;
        let corrected_samples_high_4 = (corrected[streaminfo_start + 13] & 0x0F) as u64;
        let corrected_samples_low_32 = u32::from_be_bytes([
            corrected[streaminfo_start + 14],
            corrected[streaminfo_start + 15],
            corrected[streaminfo_start + 16],
            corrected[streaminfo_start + 17],
        ]) as u64;
        let corrected_total_samples = (corrected_samples_high_4 << 32) | corrected_samples_low_32;
        assert_eq!(corrected_total_samples, expected_samples);
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

    #[test]
    fn test_utf8_encode_decode() {
        // Test various number sizes
        let test_cases = vec![
            (0u64, 1usize),
            (127u64, 1usize),
            (128u64, 2usize),
            (2047u64, 2usize),
            (2048u64, 3usize),
            (65535u64, 3usize),
            (65536u64, 4usize),
            (2097151u64, 4usize),
            (2097152u64, 5usize),
            (67108863u64, 5usize),
            (67108864u64, 6usize),
        ];

        for (value, expected_bytes) in test_cases {
            let encoded = CueFlacProcessor::encode_utf8_number(value);
            assert_eq!(
                encoded.len(),
                expected_bytes,
                "Value {} should encode to {} bytes, got {}",
                value,
                expected_bytes,
                encoded.len()
            );

            let (decoded, bytes_consumed) = CueFlacProcessor::decode_utf8_number(&encoded).unwrap();
            assert_eq!(decoded, value, "Round-trip failed for value {}", value);
            assert_eq!(
                bytes_consumed, expected_bytes,
                "Decode should consume {} bytes, got {}",
                expected_bytes, bytes_consumed
            );
        }
    }

    #[test]
    fn test_crc8_calculation() {
        // Test with known CRC-8 values
        // FLAC CRC-8 polynomial is 0x07
        // Test with simple data
        let data1 = vec![0x00, 0x00];
        let crc1 = CueFlacProcessor::calculate_crc8(&data1);
        // CRC should be non-zero for non-empty data
        assert_ne!(crc1, 0);

        // Test that CRC changes with data
        let data2 = vec![0xFF, 0xFF];
        let crc2 = CueFlacProcessor::calculate_crc8(&data2);
        assert_ne!(crc1, crc2);

        // Test that same data gives same CRC
        let crc1_again = CueFlacProcessor::calculate_crc8(&data1);
        assert_eq!(crc1, crc1_again);
    }

    #[test]
    fn test_clean_streaminfo() {
        // Create headers with multiple blocks: STREAMINFO, PADDING, SEEKTABLE
        let mut original_headers = vec![
            b'f', b'L', b'a', b'C', // FLAC signature
        ];

        // STREAMINFO block (type 0, size 34, not last)
        original_headers.extend_from_slice(&0x00000022u32.to_be_bytes()); // Type 0, size 34
        original_headers.resize(42, 0); // 4 + 4 + 34 bytes

        // Set some values in STREAMINFO
        let streaminfo_start = 8;
        original_headers[streaminfo_start + 4] = 0x12; // min_frame_size
        original_headers[streaminfo_start + 5] = 0x34;
        original_headers[streaminfo_start + 6] = 0x56;
        original_headers[streaminfo_start + 7] = 0x78; // max_frame_size
        original_headers[streaminfo_start + 8] = 0x9A;
        original_headers[streaminfo_start + 9] = 0xBC;

        // Set MD5 signature
        for i in 18..34 {
            original_headers[streaminfo_start + i] = 0xFF;
        }

        // Add PADDING block (type 1, size 100, not last)
        original_headers.extend_from_slice(&0x01000064u32.to_be_bytes()); // Type 1, size 100
        original_headers.resize(original_headers.len() + 100, 0xAA);

        // Add SEEKTABLE block (type 3, size 18, last)
        original_headers.extend_from_slice(&0x83000012u32.to_be_bytes()); // Type 3, size 18, last
        original_headers.resize(original_headers.len() + 18, 0xBB);

        // Test cleanup
        let track_duration_ms = 60 * 1000;
        let sample_rate = 44100;
        let corrected = CueFlacProcessor::write_corrected_flac_headers(
            &original_headers,
            track_duration_ms as i64,
            sample_rate,
        )
        .unwrap();

        // Verify signature
        assert_eq!(&corrected[0..4], b"fLaC");

        // Verify only STREAMINFO remains (should be 4 + 4 + 34 = 42 bytes)
        assert_eq!(corrected.len(), 42);

        // Verify STREAMINFO is marked as last
        let block_header =
            u32::from_be_bytes([corrected[4], corrected[5], corrected[6], corrected[7]]);
        assert!(block_header & 0x80000000 != 0); // Last bit set
        assert_eq!(block_header & 0x7F000000, 0); // Type 0
        assert_eq!(block_header & 0x00FFFFFF, 34); // Size 34

        // Verify min/max frame sizes are zeroed
        let streaminfo = &corrected[8..];
        assert_eq!(streaminfo[4], 0); // min_frame_size byte 0
        assert_eq!(streaminfo[5], 0); // min_frame_size byte 1
        assert_eq!(streaminfo[6], 0); // min_frame_size byte 2
        assert_eq!(streaminfo[7], 0); // max_frame_size byte 0
        assert_eq!(streaminfo[8], 0); // max_frame_size byte 1
        assert_eq!(streaminfo[9], 0); // max_frame_size byte 2

        // Verify MD5 is zeroed
        for i in 18..34 {
            assert_eq!(streaminfo[i], 0, "MD5 byte {} should be zero", i);
        }

        // Verify total_samples is set correctly
        let expected_samples = (track_duration_ms as u64 * sample_rate as u64) / 1000;
        let total_samples_high_4 = (streaminfo[13] & 0x0F) as u64;
        let total_samples_low_32 = u32::from_be_bytes([
            streaminfo[14],
            streaminfo[15],
            streaminfo[16],
            streaminfo[17],
        ]) as u64;
        let total_samples = (total_samples_high_4 << 32) | total_samples_low_32;
        assert_eq!(total_samples, expected_samples);
    }
}
