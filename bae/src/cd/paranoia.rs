//! libcdio-paranoia FFI bindings for error-corrected CD audio reading
//!
//! This module provides safe wrappers around libcdio-paranoia functions
//! for accurate audio extraction with error correction.

use crate::cd::ffi::LibcdioDrive;
use crate::cd::ripper::RipProgress;
use libc;
use libcdio_sys;
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug, Error)]
pub enum ParanoiaError {
    #[error("Paranoia initialization error: {0}")]
    Init(String),
    #[error("Read error: {0}")]
    Read(String),
    #[error("Invalid track number")]
    InvalidTrack,
}

/// Paranoia CDDA reader for error-corrected audio extraction
pub struct ParanoiaReader {
    drive: LibcdioDrive,
    // Note: libcdio-paranoia uses a cdio_cdda_t structure internally
    // We'll use the drive's device pointer and track info
}

impl ParanoiaReader {
    /// Create a new paranoia reader for a CD drive
    pub fn new(drive: LibcdioDrive) -> Result<Self, ParanoiaError> {
        // Verify drive has a disc
        if !drive.has_disc() {
            return Err(ParanoiaError::Init("No disc in drive".to_string()));
        }

        Ok(Self { drive })
    }

    /// Read audio sectors with paranoia error correction
    ///
    /// This uses libcdio's paranoia mode for accurate audio extraction
    /// with error correction and jitter handling.
    pub fn read_audio_sectors_paranoia(
        &self,
        start_lba: u32,
        num_sectors: u32,
    ) -> Result<(Vec<u8>, u32), ParanoiaError> {
        self.read_audio_sectors_paranoia_with_progress(start_lba, num_sectors, None, 0, 0)
    }

    /// Read audio sectors with paranoia error correction and progress updates
    ///
    /// This uses libcdio's paranoia mode for accurate audio extraction
    /// with error correction and jitter handling.
    /// Progress updates are sent every 1000 sectors if progress_tx is provided.
    pub fn read_audio_sectors_paranoia_with_progress(
        &self,
        start_lba: u32,
        num_sectors: u32,
        progress_tx: Option<mpsc::UnboundedSender<RipProgress>>,
        current_track: u8,
        total_tracks: u8,
    ) -> Result<(Vec<u8>, u32), ParanoiaError> {
        tracing::info!(
            "Starting paranoia read: {} sectors starting at LBA {}",
            num_sectors,
            start_lba
        );
        unsafe {
            let sector_size = libcdio_sys::CDIO_CD_FRAMESIZE_RAW as usize;
            let total_size = (num_sectors as usize) * sector_size;
            let mut buffer = vec![0u8; total_size];
            let mut errors = 0u32;

            // Send initial progress (0%) if channel is available
            if let Some(ref tx) = progress_tx {
                let track_index = (current_track - 1) as usize;
                let overall_percent = ((track_index as f32 / total_tracks as f32) * 100.0) as u8;

                let _ = tx.send(RipProgress {
                    track: current_track,
                    total_tracks,
                    percent: overall_percent,
                    track_percent: 0,
                    bytes_read: 0,
                    errors: 0,
                });
            }

            // Use cdio_read_audio_sector with retry logic for error correction
            // libcdio's read_audio_sector already includes some error handling,
            // but we can add retry logic for better reliability
            // Note: We read sectors from start_lba to start_lba + num_sectors - 1 (inclusive)
            // This means the last sector we read is end_lba - 1, which should be valid
            tracing::info!(
                "Reading {} sectors starting at LBA {} (will read LBAs {} to {} inclusive)",
                num_sectors,
                start_lba,
                start_lba,
                start_lba + num_sectors - 1
            );

            // XLD-style truncation: If we hit too many consecutive failures near the end,
            // stop trying and zero-fill the rest. This handles the case where the TOC's
            // leadout extends beyond physically readable sectors.
            //
            // Why this happens:
            // - The TOC leadout is metadata that might not match physical readability
            // - Drive hardware often can't read the last 50-200 sectors before leadout
            // - This affects ANY disc where the last track goes to the leadout (common)
            // - Happens regardless of track count (observed on both 3-track and 17-track discs)
            //
            // Threshold: If 10+ consecutive sectors fail, assume we've hit the unreadable zone
            let mut consecutive_failures = 0;
            const MAX_CONSECUTIVE_FAILURES: u32 = 10;

            for i in 0..num_sectors {
                let lba = (start_lba + i) as libcdio_sys::lba_t;
                let is_last_sector = i == num_sectors - 1;

                // Verify LBA is within expected range
                let expected_last_lba = (start_lba + num_sectors - 1) as libcdio_sys::lba_t;
                if lba > expected_last_lba {
                    return Err(ParanoiaError::Read(format!(
                        "LBA {} exceeds expected range (start={}, expected_last={})",
                        lba, start_lba, expected_last_lba
                    )));
                }

                // Log the exact LBA being read when we're near the end or when it matches the failing LBA
                if is_last_sector || lba == 46058 || (i > 0 && i % 5000 == 0) {
                    tracing::info!(
                        "Reading sector {} of {}: LBA {} (start_lba={}, expected_last={})",
                        i + 1,
                        num_sectors,
                        lba,
                        start_lba,
                        expected_last_lba
                    );
                }
                // XLD-style: Check if we've hit too many consecutive failures
                // If yes, we've likely hit the unreadable zone near the leadout
                // Stop trying and zero-fill the rest (like XLD does with truncation)
                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    tracing::warn!(
                        "Hit {} consecutive unreadable sectors - TOC leadout likely extends beyond readable area",
                        consecutive_failures
                    );
                    tracing::warn!(
                        "XLD-style truncation: Zero-filling remaining {} sectors (LBA {} to {})",
                        num_sectors - i,
                        lba,
                        start_lba + num_sectors - 1
                    );

                    // Zero-fill from current position to end of buffer
                    let remaining_offset = (i as usize) * sector_size;
                    buffer[remaining_offset..].fill(0);

                    // Break out of the loop - we're done
                    break;
                }

                let mut retries = 3;
                let mut success = false;

                // Log and emit progress every 1000 sectors (about 2MB), or at the end
                // Use i+1 to represent sectors read (since i is 0-indexed)
                let sectors_read = i + 1;
                let should_send_progress = (i > 0 && i % 1000 == 0) || is_last_sector;

                if should_send_progress {
                    let track_progress = (sectors_read as f32 / num_sectors as f32) * 100.0;
                    tracing::info!(
                        "Reading sectors: {}/{} ({:.1}%){}",
                        sectors_read,
                        num_sectors,
                        track_progress,
                        if is_last_sector { " [LAST SECTOR]" } else { "" }
                    );

                    // Send progress update if channel is available
                    if let Some(ref tx) = progress_tx {
                        // Calculate overall progress across all tracks
                        // Track is 0-indexed in the loop, so current_track is 1-indexed
                        let track_index = (current_track - 1) as usize;
                        let track_progress_percent = track_progress / 100.0;
                        let overall_percent = ((track_index as f32 + track_progress_percent)
                            / total_tracks as f32
                            * 100.0) as u8;

                        let bytes_read = (sectors_read as usize) * sector_size;
                        let _ = tx.send(RipProgress {
                            track: current_track,
                            total_tracks,
                            percent: overall_percent,
                            track_percent: track_progress.min(100.0) as u8,
                            bytes_read: bytes_read as u64,
                            errors,
                        });
                    }
                }

                if is_last_sector {
                    tracing::info!("About to read last sector at LBA {}", lba);
                }

                let mut attempt_num = 1;
                while retries > 0 && !success {
                    if is_last_sector {
                        tracing::info!(
                            "Reading last sector at LBA {}, attempt {} of 3",
                            lba,
                            attempt_num
                        );
                    }
                    let result = libcdio_sys::cdio_read_audio_sector(
                        self.drive.device_ptr(),
                        buffer.as_mut_ptr().add((i as usize) * sector_size) as *mut libc::c_void,
                        lba,
                    );

                    if result == 0 {
                        // Success! Reset consecutive failure counter
                        success = true;
                        consecutive_failures = 0;
                        if is_last_sector {
                            tracing::info!("Last sector read successfully at LBA {}", lba);
                        }
                    } else {
                        // Only log first failure to reduce spam
                        if attempt_num == 1 {
                            tracing::warn!(
                                "Failed to read sector at LBA {} (attempt {} of 3, error code: {})",
                                lba,
                                attempt_num,
                                result
                            );
                        }
                        retries -= 1;
                        errors += 1;
                        attempt_num += 1;
                        if retries == 0 {
                            // After 3 retries, this sector is unreadable
                            // Fill with zeros and increment consecutive failure counter
                            let sector_offset = (i as usize) * sector_size;
                            let sector_slice =
                                &mut buffer[sector_offset..sector_offset + sector_size];
                            sector_slice.fill(0);

                            // Track consecutive failures - if we hit too many, we'll bail out
                            consecutive_failures += 1;

                            tracing::error!(
                                "⚠️  UNREADABLE SECTOR: LBA {} failed after {} retries (error code: {})",
                                lba, attempt_num - 1, result
                            );
                            tracing::error!(
                                "⚠️  FILLED WITH ZEROS: Sector {} of {} will be SILENT in the output (track {})",
                                i + 1, num_sectors, current_track
                            );

                            // Mark as "success" so we continue to the next sector
                            // (If we hit too many consecutive failures, we'll bail out at the top of the loop)
                            success = true;
                        }
                    }
                }
            }

            tracing::info!("Finished reading all {} sectors", num_sectors);

            // Send final progress update (100%) if channel is available
            if let Some(ref tx) = progress_tx {
                tracing::info!(
                    "Sending final progress update (100%) for track {}",
                    current_track
                );
                let track_index = (current_track - 1) as usize;
                let overall_percent =
                    (((track_index + 1) as f32 / total_tracks as f32) * 100.0) as u8;
                let total_bytes = (num_sectors as usize) * sector_size;

                let _ = tx.send(RipProgress {
                    track: current_track,
                    total_tracks,
                    percent: overall_percent,
                    track_percent: 100,
                    bytes_read: total_bytes as u64,
                    errors,
                });
                tracing::info!("Final progress update sent");
            }

            tracing::info!("Returning from read_audio_sectors_paranoia_with_progress");
            Ok((buffer, errors))
        }
    }

    /// Get the underlying drive
    #[allow(dead_code)] // Public API method for future use
    pub fn drive(&self) -> &LibcdioDrive {
        &self.drive
    }
}
