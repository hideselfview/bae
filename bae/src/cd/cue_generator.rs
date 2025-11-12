//! CUE sheet generation during ripping

use crate::cd::drive::CdToc;
use crate::cd::ripper::RipResult;
use crate::cue_flac::CueSheet;
use std::path::PathBuf;

/// Generates CUE sheets from CD TOC and rip results
pub struct CueGenerator;

impl CueGenerator {
    /// Generate a CUE sheet from CD TOC and rip results
    pub fn generate_cue_sheet(
        toc: &CdToc,
        rip_results: &[RipResult],
        _flac_filename: &str,
        performer: &str,
        title: &str,
    ) -> CueSheet {
        let mut tracks = Vec::new();

        // Convert sector offsets to time (MM:SS:FF format)
        // CD audio sectors are 75 sectors per second
        for (idx, result) in rip_results.iter().enumerate() {
            let track_num = result.track_number;
            let start_sector = if idx < toc.track_offsets.len() {
                toc.track_offsets[idx]
            } else {
                0
            };

            // Convert sector to milliseconds (sector * 1000 / 75)
            let start_time_ms = (start_sector as u64 * 1000) / 75;

            // Calculate end time from duration
            let end_time_ms = start_time_ms + result.duration_ms;

            tracks.push(crate::cue_flac::CueTrack {
                number: track_num as u32,
                title: format!("Track {}", track_num),
                performer: Some(performer.to_string()),
                start_time_ms,
                pregap_time_ms: None, // TODO: Extract from CD TOC if available
                end_time_ms: Some(end_time_ms),
            });
        }

        CueSheet {
            title: title.to_string(),
            performer: performer.to_string(),
            tracks,
        }
    }

    /// Write CUE sheet to file
    pub fn write_cue_file(
        cue_sheet: &CueSheet,
        output_path: &PathBuf,
    ) -> Result<(), std::io::Error> {
        use std::fs::File;
        use std::io::Write;

        let mut file = File::create(output_path)?;
        writeln!(file, "REM GENRE \"\"")?;
        writeln!(file, "REM DATE \"\"")?;
        writeln!(file, "REM DISCID")?; // TODO: Add DiscID
        writeln!(file, "REM COMMENT \"\"")?;
        writeln!(file, "PERFORMER \"{}\"", cue_sheet.performer)?;
        writeln!(file, "TITLE \"{}\"", cue_sheet.title)?;
        writeln!(file, "FILE \"\" WAVE")?; // TODO: Add FLAC filename

        for track in &cue_sheet.tracks {
            writeln!(file, "  TRACK {:02} AUDIO", track.number)?;
            if let Some(ref performer) = track.performer {
                writeln!(file, "    PERFORMER \"{}\"", performer)?;
            }
            writeln!(file, "    TITLE \"{}\"", track.title)?;

            // Convert milliseconds to MM:SS:FF
            let start_ms = track.start_time_ms;
            let minutes = (start_ms / 60000) % 60;
            let seconds = ((start_ms / 1000) % 60) as u8;
            let frames = ((start_ms % 1000) * 75 / 1000) as u8;
            writeln!(
                file,
                "    INDEX 01 {:02}:{:02}:{:02}",
                minutes, seconds, frames
            )?;
        }

        Ok(())
    }
}
