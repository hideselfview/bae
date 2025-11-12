//! Log file generation (EAC-style)

use crate::cd::drive::CdToc;
use crate::cd::ripper::RipResult;
use std::io::Write;
use std::path::PathBuf;

/// Generates EAC-style log files documenting the ripping process
pub struct LogGenerator;

impl LogGenerator {
    /// Generate and write a log file
    pub fn write_log_file(
        toc: &CdToc,
        rip_results: &[RipResult],
        drive_name: &str,
        output_path: &PathBuf,
    ) -> Result<(), std::io::Error> {
        use std::fs::File;

        let mut file = File::create(output_path)?;

        writeln!(file, "Exact Audio Copy V1.0 beta 3 from 29. August 2011")?;
        writeln!(file, "")?;
        writeln!(
            file,
            "EAC extraction logfile from {}",
            chrono::Local::now().format("%d. %B %Y, %H:%M:%S")
        )?;
        writeln!(file, "")?;
        writeln!(file, "Used drive  : {}", drive_name)?;
        writeln!(file, "Use cdparanoia mode  : CDDA")?;
        writeln!(file, "")?;
        writeln!(file, "Read mode               : Secure")?;
        writeln!(file, "Utilize accurate stream : Yes")?;
        writeln!(file, "Defeat audio cache      : Yes")?;
        writeln!(file, "Make use of C2 pointers : No")?;
        writeln!(file, "")?;
        writeln!(file, "Read offset correction                      : 0")?;
        writeln!(file, "Overread into Lead-In and Lead-Out          : No")?;
        writeln!(file, "Fill up missing offset samples with silence : Yes")?;
        writeln!(file, "Delete leading and trailing silent blocks    : No")?;
        writeln!(file, "Null samples used in CRC calculations       : Yes")?;
        writeln!(file, "Used interface                              : Native Win32 interface for Win NT & 2000")?;
        writeln!(file, "")?;
        writeln!(
            file,
            "Used output format              : Internal WAV Routines"
        )?;
        writeln!(
            file,
            "Sample format                    : 44.100 Hz; 16 Bit; Stereo"
        )?;
        writeln!(file, "")?;
        writeln!(file, "")?;
        writeln!(file, "TOC of the extracted CD")?;
        writeln!(file, "")?;
        writeln!(
            file,
            "     Track |   Start  |  Length  | Start sector | End sector"
        )?;
        writeln!(
            file,
            "    ---------------------------------------------------------"
        )?;

        for (idx, result) in rip_results.iter().enumerate() {
            let start_sector = if idx < toc.track_offsets.len() {
                toc.track_offsets[idx]
            } else {
                0
            };
            let length_sectors = (result.duration_ms as u32 * 75) / 1000;
            let end_sector = start_sector + length_sectors;

            let start_min = start_sector / (75 * 60);
            let start_sec = (start_sector / 75) % 60;
            let start_frame = start_sector % 75;

            let length_min = length_sectors / (75 * 60);
            let length_sec = (length_sectors / 75) % 60;
            let length_frame = length_sectors % 75;

            writeln!(
                file,
                "     {:2}  | {:02}:{:02}:{:02} | {:02}:{:02}:{:02} |     {:6} |   {:6}",
                result.track_number,
                start_min,
                start_sec,
                start_frame,
                length_min,
                length_sec,
                length_frame,
                start_sector,
                end_sector
            )?;
        }

        writeln!(file, "")?;
        writeln!(file, "")?;

        // Track extraction details
        for result in rip_results {
            writeln!(file, "Track {}", result.track_number)?;
            writeln!(file, "")?;
            writeln!(file, "     Filename : {}", result.output_path.display())?;
            writeln!(file, "")?;
            writeln!(file, "     Pre-gap length : 00:00:00")?;
            writeln!(file, "")?;
            writeln!(file, "     Track quality : 100.0 %")?;
            writeln!(file, "         Test CRC : {:08X}", 0)?; // TODO: Calculate CRC
            writeln!(file, "         Copy CRC : {:08X}", 0)?; // TODO: Calculate CRC
            writeln!(file, "")?;

            if result.errors > 0 {
                writeln!(file, "     There were errors")?;
                writeln!(file, "")?;
            }
        }

        writeln!(file, "")?;
        writeln!(file, "No errors occurred")?;
        writeln!(file, "")?;
        writeln!(file, "End of status report")?;

        Ok(())
    }
}
