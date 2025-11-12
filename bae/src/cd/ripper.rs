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
pub struct RipProgress {
    pub track: u8,
    pub total_tracks: u8,
    pub percent: u8,
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
        _progress_tx: Option<mpsc::UnboundedSender<RipProgress>>,
    ) -> Result<Vec<RipResult>, RipError> {
        let mut results = Vec::new();

        for track_num in self.toc.first_track..=self.toc.last_track {
            let result = self.rip_track(track_num).await?;
            results.push(result);
        }

        Ok(results)
    }

    /// Rip a single track
    async fn rip_track(&self, track_num: u8) -> Result<RipResult, RipError> {
        // 1. Read raw audio bytes from CD via libcdio-paranoia (with error correction)
        // 2. Convert bytes to interleaved i32 samples (CD audio is 16-bit stereo)
        // 3. Stream samples through FLAC encoder
        // 4. Write FLAC file to output directory

        let output_path = self.output_dir.join(format!("{:02}.flac", track_num));

        let sample_rate = 44100u32;
        let channels = 2u32;
        let bits_per_sample = 16u32;

        // Read audio data with paranoia error correction
        let (samples, errors) = self.read_track_samples(track_num).await?;

        // Encode to FLAC
        let flac_data = self.encode_to_flac(&samples, sample_rate, channels, bits_per_sample)?;

        // Write to file
        tokio::fs::write(&output_path, &flac_data)
            .await
            .map_err(|e| RipError::Io(e))?;

        // Calculate duration (samples / sample_rate / channels)
        let duration_ms = (samples.len() as u64 * 1000) / (sample_rate as u64 * channels as u64);

        Ok(RipResult {
            track_number: track_num,
            output_path,
            bytes_written: flac_data.len() as u64,
            errors, // Error count from paranoia reader
            duration_ms,
        })
    }

    /// Read raw samples from a track using libcdio-paranoia
    /// Returns samples and error count
    async fn read_track_samples(&self, track_num: u8) -> Result<(Vec<i32>, u32), RipError> {
        use crate::cd::ffi::LibcdioDrive;
        use crate::cd::paranoia::ParanoiaReader;

        // Open drive
        let drive = LibcdioDrive::open(&self.drive.device_path)
            .map_err(|e| RipError::Drive(format!("Failed to open drive: {}", e)))?;

        // Get track start and end LBAs
        let start_lba = drive
            .track_start_lba(track_num)
            .map_err(|e| RipError::Read(format!("Failed to get start LBA: {}", e)))?;

        // Calculate end LBA (start of next track, or leadout if last track)
        let end_lba = if track_num < self.toc.last_track {
            drive
                .track_start_lba(track_num + 1)
                .map_err(|e| RipError::Read(format!("Failed to get end LBA: {}", e)))?
        } else {
            drive
                .leadout_lba()
                .map_err(|e| RipError::Read(format!("Failed to get leadout: {}", e)))?
        };

        let num_sectors = end_lba - start_lba;

        // Read audio sectors with paranoia error correction (run in blocking task since libcdio is synchronous)
        let device_path = self.drive.device_path.clone();
        let (audio_data, errors) = tokio::task::spawn_blocking(move || {
            let drive = LibcdioDrive::open(&device_path)
                .map_err(|e| RipError::Drive(format!("Failed to open drive: {}", e)))?;
            let paranoia_reader = ParanoiaReader::new(drive).map_err(|e| {
                RipError::Read(format!("Failed to initialize paranoia reader: {}", e))
            })?;
            paranoia_reader
                .read_audio_sectors_paranoia(start_lba, num_sectors)
                .map_err(|e| RipError::Read(format!("Failed to read sectors: {}", e)))
        })
        .await
        .map_err(|e| RipError::Read(format!("Task failed: {}", e)))??;

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
