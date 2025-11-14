//! CD ripping logic - streams bytes directly to FLAC encoder

use crate::cd::drive::{CdDrive, CdToc};
use flacenc::bitsink::ByteSink;
use flacenc::component::BitRepr;
use flacenc::config;
use flacenc::error::Verify;
use flacenc::source::MemSource;
use std::path::PathBuf;
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug, Error)]
pub enum RipError {
    #[error("Drive error: {0}")]
    Drive(String),
    #[error("Read error: {0}")]
    Read(String),
    #[error("FLAC encoding error: {0}")]
    Flac(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Progress update during ripping
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields are used for UI progress reporting
pub struct RipProgress {
    pub track: u8,
    pub total_tracks: u8,
    /// Overall album progress (0-100%)
    pub percent: u8,
    /// Progress within the current track (0-100%)
    pub track_percent: u8,
    pub bytes_read: u64,
    pub errors: u32,
}

/// Result of ripping a single track
#[derive(Debug, Clone)]
pub struct RipResult {
    pub track_number: u8,
    pub output_path: PathBuf,
    pub bytes_written: u64,
    pub errors: u32,
    pub duration_ms: u64,
    pub crc32: u32,
}

/// CD ripper that streams audio directly to FLAC encoder
pub struct CdRipper {
    drive: CdDrive,
    toc: CdToc,
    output_dir: PathBuf,
}

impl CdRipper {
    /// Create a new CD ripper
    pub fn new(drive: CdDrive, toc: CdToc, output_dir: PathBuf) -> Self {
        Self {
            drive,
            toc,
            output_dir,
        }
    }

    /// Rip all tracks from the CD
    ///
    /// Streams raw audio bytes from CD directly through FLAC encoder
    /// (no intermediate WAV file)
    pub async fn rip_all_tracks(
        &self,
        progress_tx: Option<mpsc::UnboundedSender<RipProgress>>,
    ) -> Result<Vec<RipResult>, RipError> {
        use tracing::info;

        let mut results = Vec::new();
        let total_tracks = (self.toc.last_track - self.toc.first_track + 1) as u8;
        info!(
            "Starting to rip {} tracks ({} to {})",
            total_tracks, self.toc.first_track, self.toc.last_track
        );

        for (idx, track_num) in (self.toc.first_track..=self.toc.last_track).enumerate() {
            info!("Ripping track {} ({}/{})", track_num, idx + 1, total_tracks);

            // Send progress update before starting track
            if let Some(ref tx) = progress_tx {
                let percent = ((idx * 100) / total_tracks as usize) as u8;
                let _ = tx.send(RipProgress {
                    track: track_num,
                    total_tracks,
                    percent,
                    track_percent: 0,
                    bytes_read: 0,
                    errors: 0,
                });
            }

            info!("Calling rip_track for track {}", track_num);
            let result = self.rip_track(track_num, progress_tx.as_ref()).await?;
            info!(
                "Track {} ripped successfully, {} bytes written",
                track_num, result.bytes_written
            );
            results.push(result);

            // Send progress update after completing track
            if let Some(ref tx) = progress_tx {
                let percent = (((idx + 1) * 100) / total_tracks as usize) as u8;
                let _ = tx.send(RipProgress {
                    track: track_num,
                    total_tracks,
                    percent,
                    track_percent: 100,
                    bytes_read: results.last().unwrap().bytes_written,
                    errors: results.last().unwrap().errors,
                });
            }
        }

        info!("All tracks ripped successfully");
        Ok(results)
    }

    /// Rip a single track
    async fn rip_track(
        &self,
        track_num: u8,
        progress_tx: Option<&mpsc::UnboundedSender<RipProgress>>,
    ) -> Result<RipResult, RipError> {
        // 1. Read raw audio bytes from CD via libcdio-paranoia (with error correction)
        // 2. Convert bytes to interleaved i32 samples (CD audio is 16-bit stereo)
        // 3. Stream samples through FLAC encoder
        // 4. Write FLAC file to output directory

        let output_path = self.output_dir.join(format!("{:02}.flac", track_num));

        let sample_rate = 44100u32;
        let channels = 2u32;
        let bits_per_sample = 16u32;

        // Read audio data with paranoia error correction
        let total_tracks = (self.toc.last_track - self.toc.first_track + 1) as u8;
        let (samples, errors) = match self
            .read_track_samples(track_num, progress_tx, total_tracks)
            .await
        {
            Ok(data) => {
                tracing::info!("read_track_samples returned Ok for track {}", track_num);
                data
            }
            Err(e) => {
                tracing::info!(
                    "read_track_samples returned Err for track {}: {}",
                    track_num,
                    e
                );
                return Err(e);
            }
        };

        // Encode to FLAC
        let flac_data = self.encode_to_flac(&samples, sample_rate, channels, bits_per_sample)?;

        // Calculate CRC32 of FLAC data (for EAC-style log file)
        let crc32 = crc32fast::hash(&flac_data);

        // Write to file
        tokio::fs::write(&output_path, &flac_data)
            .await
            .map_err(RipError::Io)?;

        // Calculate duration (samples / sample_rate / channels)
        let duration_ms = (samples.len() as u64 * 1000) / (sample_rate as u64 * channels as u64);

        Ok(RipResult {
            track_number: track_num,
            output_path,
            bytes_written: flac_data.len() as u64,
            errors, // Error count from paranoia reader
            duration_ms,
            crc32,
        })
    }

    /// Read raw samples from a track using libcdio-paranoia
    /// Returns samples and error count
    async fn read_track_samples(
        &self,
        track_num: u8,
        progress_tx: Option<&mpsc::UnboundedSender<RipProgress>>,
        total_tracks: u8,
    ) -> Result<(Vec<i32>, u32), RipError> {
        use crate::cd::ffi::LibcdioDrive;
        use crate::cd::paranoia::ParanoiaReader;
        use tracing::info;

        info!(
            "Reading track {} samples from drive {:?}",
            track_num, self.drive.device_path
        );

        // XLD-style approach: Get track boundaries from TOC
        // The TOC (Table of Contents) defines where each track starts/ends
        // For the last track, the end is the leadout position from the TOC
        //
        // IMPORTANT: The leadout position might extend beyond physically readable sectors!
        // This happens because:
        // 1. The TOC is metadata stored separately from audio data
        // 2. Drive hardware can't always read right up to the leadout boundary
        // 3. Some discs have padding/unrecorded sectors near the leadout
        // 4. Manufacturing variations mean actual audio might end before the TOC leadout
        //
        // XLD's solution: Trust the TOC, but check before each read if we'd extend past
        // the track boundary. If yes, truncate the read and zero-fill the rest.

        let device_path_for_lba = self.drive.device_path.clone();
        let last_track = self.toc.last_track;

        let (start_lba, end_lba) = tokio::task::spawn_blocking(move || {
            let drive = LibcdioDrive::open(&device_path_for_lba)
                .map_err(|e| RipError::Drive(format!("Failed to open drive for LBA: {}", e)))?;

            let start = drive
                .track_start_lba(track_num)
                .map_err(|e| RipError::Read(format!("Failed to get start LBA: {}", e)))?;

            // Calculate end LBA from TOC
            // - For non-last tracks: end is the start of the next track
            // - For last track: end is the leadout position from TOC
            // Note: end_lba is exclusive - we read from start_lba to end_lba-1 (inclusive)
            let end = if track_num < last_track {
                // Not the last track: use start of next track as boundary
                let next_track_start = drive
                    .track_start_lba(track_num + 1)
                    .map_err(|e| RipError::Read(format!("Failed to get end LBA: {}", e)))?;
                info!(
                    "Track {} (not last): using next track start {} as end_lba",
                    track_num, next_track_start
                );
                next_track_start
            } else {
                // Last track: use leadout from TOC as boundary
                // This leadout value might be beyond what the drive can physically read!
                let leadout = drive
                    .leadout_lba()
                    .map_err(|e| RipError::Read(format!("Failed to get leadout: {}", e)))?;
                info!(
                    "Track {} (last track): using TOC leadout={} as end_lba",
                    track_num, leadout
                );
                leadout
            };

            // Log the calculated range for debugging
            info!(
                "Track {} LBA calculation: start={}, end={} (exclusive), will read {} sectors ({} to {} inclusive)",
                track_num, start, end, end - start, start, end - 1
            );

            Ok::<(u32, u32), RipError>((start, end))
        })
        .await
        .map_err(|e| RipError::Read(format!("LBA task failed: {}", e)))??;

        let num_sectors = end_lba - start_lba;
        info!(
            "Track {}: LBA range {} to {} ({} sectors) - will read sectors {} to {} (inclusive)",
            track_num,
            start_lba,
            end_lba,
            num_sectors,
            start_lba,
            start_lba + num_sectors - 1
        );

        // Verify we're not trying to read beyond the track boundary
        if num_sectors == 0 {
            return Err(RipError::Read(format!(
                "Track {} has zero sectors (start_lba={}, end_lba={})",
                track_num, start_lba, end_lba
            )));
        }

        // For the last track, if the leadout seems to include unreadable sectors,
        // we might need to reduce the sector count. However, we'll try the full range first
        // and only reduce if we get a read error near the end.

        // Read audio sectors with paranoia error correction (run in blocking task since libcdio is synchronous)
        let device_path = self.drive.device_path.clone();
        let start_lba_for_read = start_lba;
        let num_sectors_for_read = num_sectors;
        // Clone progress_tx for the blocking task (UnboundedSender is Send)
        let progress_tx_for_blocking = progress_tx.cloned();
        info!("Spawning blocking task to read audio sectors...");
        let blocking_task = tokio::task::spawn_blocking(move || {
            info!("Blocking task started, opening drive...");
            let drive = LibcdioDrive::open(&device_path)
                .map_err(|e| RipError::Drive(format!("Failed to open drive: {}", e)))?;
            info!("Drive opened, initializing paranoia reader...");
            let paranoia_reader = ParanoiaReader::new(drive).map_err(|e| {
                RipError::Read(format!("Failed to initialize paranoia reader: {}", e))
            })?;
            info!(
                "Paranoia reader initialized, reading {} sectors...",
                num_sectors_for_read
            );
            info!("Calling read_audio_sectors_paranoia_with_progress...");
            let result = paranoia_reader.read_audio_sectors_paranoia_with_progress(
                start_lba_for_read,
                num_sectors_for_read,
                progress_tx_for_blocking,
                track_num,
                total_tracks,
            );
            info!("Paranoia read completed, got result, checking if Ok...");
            match &result {
                Ok((buf, errs)) => {
                    info!("Result is Ok: {} bytes, {} errors", buf.len(), errs);
                }
                Err(e) => {
                    info!("Result is Err: {}", e);
                }
            }
            info!("Unwrapping result...");
            let mapped_result = result.map_err(|e| {
                info!("Mapping error: {}", e);
                RipError::Read(format!("Failed to read sectors: {}", e))
            });
            info!(
                "Result mapped, returning from blocking task (buffer size: {:?})",
                mapped_result.as_ref().ok().map(|(buf, _)| buf.len())
            );
            mapped_result
        });

        info!("Blocking task spawned, awaiting result...");
        let result = blocking_task
            .await
            .map_err(|e| RipError::Read(format!("Task failed: {}", e)))?;

        info!("Blocking task awaited successfully, unwrapping result...");
        let (audio_data, errors) = match result {
            Ok(data) => {
                info!("Result is Ok, unwrapping tuple...");
                data
            }
            Err(e) => {
                info!("Result is Err: {}, returning error", e);
                return Err(e);
            }
        };

        info!("Blocking task completed, audio data received");

        info!(
            "Audio data read: {} bytes, {} errors",
            audio_data.len(),
            errors
        );

        // Convert raw PCM bytes to i32 samples
        // CD audio is 16-bit little-endian stereo (44100 Hz)
        // Each sample is 2 bytes, interleaved L/R
        let mut samples = Vec::with_capacity(audio_data.len() / 2);

        for chunk in audio_data.chunks_exact(2) {
            // Read 16-bit little-endian sample
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]) as i32;
            samples.push(sample);
        }

        Ok((samples, errors))
    }

    /// Encode samples to FLAC using flacenc
    fn encode_to_flac(
        &self,
        samples: &[i32],
        sample_rate: u32,
        channels: u32,
        bits_per_sample: u32,
    ) -> Result<Vec<u8>, RipError> {
        // Convert samples to the format flacenc expects (interleaved i32)
        let source = MemSource::from_samples(
            samples,
            channels as usize,
            bits_per_sample as usize,
            sample_rate as usize,
        );

        // Create and verify encoder config
        let config = config::Encoder::default();
        let config = config.into_verified().map_err(|(_, e)| {
            RipError::Flac(format!("Failed to verify encoder config: {:?}", e))
        })?;

        // Encode with default block size (4096)
        let flac_stream = flacenc::encode_with_fixed_block_size(&config, source, 4096)
            .map_err(|e| RipError::Flac(format!("Failed to encode FLAC: {:?}", e)))?;

        // Write stream to a ByteSink
        let mut sink = ByteSink::new();
        flac_stream
            .write(&mut sink)
            .map_err(|e| RipError::Flac(format!("Failed to write stream to sink: {:?}", e)))?;

        Ok(sink.as_slice().to_vec())
    }
}
