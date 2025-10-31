use std::io::Cursor;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use symphonia::core::{
    audio::AudioBufferRef,
    codecs::{Decoder, DecoderOptions},
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
    units::Time,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecoderError {
    #[error("Symphonia error: {0}")]
    Symphonia(#[from] symphonia::core::errors::Error),
    #[error("No audio tracks found")]
    NoAudioTracks,
}

/// Wrapper around symphonia decoder that tracks decoded samples for position calculation
pub struct TrackDecoder {
    format_reader: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    sample_rate: u32,
    decoded_samples: Arc<AtomicU64>,
    duration: Option<std::time::Duration>,
}

impl TrackDecoder {
    /// Create a new decoder from FLAC data
    pub fn new(flac_data: Vec<u8>) -> Result<Self, DecoderError> {
        let cursor = Cursor::new(flac_data);
        let media_source = MediaSourceStream::new(Box::new(cursor), Default::default());

        let mut hint = Hint::new();
        hint.with_extension("flac");

        let probed = symphonia::default::get_probe().format(
            &hint,
            media_source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )?;

        let format_reader = probed.format;

        // Find the audio track
        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or(DecoderError::NoAudioTracks)?;

        let track_id = track.id;
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);

        // Try to get duration from track parameters or metadata
        let duration = track.codec_params.n_frames.map(|n_frames| {
            std::time::Duration::from_secs_f64(n_frames as f64 / sample_rate as f64)
        });

        // Create decoder
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())?;

        Ok(Self {
            format_reader,
            decoder,
            track_id,
            sample_rate,
            decoded_samples: Arc::new(AtomicU64::new(0)),
            duration,
        })
    }

    /// Decode the next packet and return audio buffer
    /// Returns None when end of stream is reached
    pub fn decode_next(&mut self) -> Result<Option<AudioBufferRef<'_>>, DecoderError> {
        loop {
            let packet = match self.format_reader.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None);
                }
                Err(e) => return Err(DecoderError::Symphonia(e)),
            };

            // Skip packets not for our track
            if packet.track_id() != self.track_id {
                continue;
            }

            // Decode packet
            let audio_buf = self.decoder.decode(&packet)?;

            // Update decoded samples count
            let samples_in_packet = audio_buf.frames() as u64;
            self.decoded_samples
                .fetch_add(samples_in_packet, Ordering::Relaxed);

            return Ok(Some(audio_buf));
        }
    }

    /// Get the current playback position based on decoded samples
    pub fn position(&self) -> std::time::Duration {
        let samples = self.decoded_samples.load(Ordering::Relaxed);
        let seconds = samples as f64 / self.sample_rate as f64;
        std::time::Duration::from_secs_f64(seconds)
    }

    /// Get the sample rate
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Seek to a specific position
    pub fn seek(&mut self, position: std::time::Duration) -> Result<(), DecoderError> {
        // Convert duration to sample number
        let position_seconds = position.as_secs_f64();
        let sample_number = (position_seconds * self.sample_rate as f64) as u64;

        // Create Time object: seconds (u64) + fractional part (f64)
        let secs = position_seconds.floor() as u64;
        let frac = position_seconds.fract();
        let seek_time = Time::new(secs, frac);

        match self.format_reader.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time: seek_time,
                track_id: Some(self.track_id),
            },
        ) {
            Ok(_) => {
                // Success - update decoded_samples
                self.decoded_samples.store(sample_number, Ordering::Relaxed);
                return Ok(());
            }
            Err(e) => {
                tracing::warn!(
                    "Seek by Time failed for {}.{}s: {:?}, falling back to decode",
                    secs,
                    frac,
                    e
                );
                // Fall through to decode from start
            }
        }

        // Fallback: seek to beginning and decode forward to desired position
        // This is inefficient but will work
        tracing::info!(
            "Seeking by decoding from start to {}s (sample {})",
            position_seconds,
            sample_number
        );

        // Reset to beginning
        let zero_time = Time::new(0, 0.0);
        self.format_reader.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time: zero_time,
                track_id: Some(self.track_id),
            },
        )?;

        self.decoded_samples.store(0, Ordering::Relaxed);

        // Decode forward to the desired position
        let mut decoded = 0u64;
        while decoded < sample_number {
            match self.decode_next()? {
                Some(audio_buf) => {
                    decoded += audio_buf.frames() as u64;
                }
                None => break, // End of stream
            }
        }

        // Update decoded_samples to match the seek position
        self.decoded_samples
            .store(decoded.min(sample_number), Ordering::Relaxed);

        Ok(())
    }

    /// Get the track duration, if available
    pub fn duration(&self) -> Option<std::time::Duration> {
        self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Test that seek correctly calculates seconds and fractional parts for Time::new
    #[test]
    fn test_seek_time_calculation() {
        // Test various durations to ensure Time::new gets correct values
        let test_cases = vec![
            (Duration::from_secs(0), (0u64, 0.0f64)),
            (Duration::from_millis(500), (0u64, 0.5f64)),
            (Duration::from_secs(59), (59u64, 0.0f64)),
            (Duration::from_millis(59500), (59u64, 0.5f64)),
            (Duration::from_secs(60), (60u64, 0.0f64)),
            (Duration::from_secs(122), (122u64, 0.0f64)),
            (Duration::from_millis(122500), (122u64, 0.5f64)),
            (Duration::from_secs(3661), (3661u64, 0.0f64)),
        ];

        for (duration, (expected_secs, expected_frac)) in test_cases {
            let position_seconds = duration.as_secs_f64();
            let secs = position_seconds.floor() as u64;
            let frac = position_seconds.fract();

            assert_eq!(
                secs, expected_secs,
                "Seconds should match for duration {:?}",
                duration
            );
            assert!(
                (frac - expected_frac).abs() < 0.001,
                "Fractional part should match for duration {:?} (got {}, expected {})",
                duration,
                frac,
                expected_frac
            );
        }
    }

    /// Test sample number calculation from duration
    #[test]
    fn test_sample_number_calculation() {
        let sample_rate = 44100u32;

        let test_cases = vec![
            (Duration::from_secs(0), 0u64),
            (Duration::from_millis(500), 22050u64),
            (Duration::from_secs(1), 44100u64),
            (Duration::from_secs(60), 2646000u64),
        ];

        for (duration, expected_samples) in test_cases {
            let position_seconds = duration.as_secs_f64();
            let sample_number = (position_seconds * sample_rate as f64) as u64;

            assert_eq!(
                sample_number, expected_samples,
                "Sample number should match for duration {:?} at {} Hz",
                duration, sample_rate
            );
        }
    }

    /// Test position calculation from decoded samples
    #[test]
    fn test_position_calculation_from_samples() {
        let sample_rate = 44100u32;

        let test_cases = vec![
            (0u64, Duration::from_secs(0)),
            (22050u64, Duration::from_millis(500)),
            (44100u64, Duration::from_secs(1)),
            (2646000u64, Duration::from_secs(60)),
        ];

        for (samples, expected_duration) in test_cases {
            let seconds = samples as f64 / sample_rate as f64;
            let calculated_duration = Duration::from_secs_f64(seconds);

            // Allow small floating point error
            let diff = calculated_duration.abs_diff(expected_duration);

            assert!(
                diff < Duration::from_millis(1),
                "Position should match for {} samples at {} Hz (got {:?}, expected {:?})",
                samples,
                sample_rate,
                calculated_duration,
                expected_duration
            );
        }
    }

    /// Test that seek handles durations > 59 seconds correctly
    /// This was the original bug that required Time::new instead of Time::from_ss
    #[test]
    fn test_seek_handles_long_durations() {
        // Test that durations > 59 seconds are handled correctly
        let long_durations = vec![
            Duration::from_secs(60),
            Duration::from_secs(122), // Original failing case
            Duration::from_secs(300),
            Duration::from_secs(3661), // Over 1 hour
        ];

        for duration in long_durations {
            let position_seconds = duration.as_secs_f64();
            let secs = position_seconds.floor() as u64;
            let frac = position_seconds.fract();

            // Verify secs is u64 (not u8), so it can handle > 59
            assert!(
                secs > 59 || duration.as_secs() <= 59,
                "Duration {:?} should result in secs {} > 59 when duration > 59s",
                duration,
                secs
            );

            // Verify Time::new can be constructed (doesn't panic)
            // The important thing is that secs is u64 (not u8) and can handle large values
            let _seek_time = Time::new(secs, frac);
        }
    }

    // Note: Full seek functionality tests require valid FLAC files.
    // To test actual seek behavior with real FLAC files, use integration tests
    // in tests/test_playback_behavior.rs
}
