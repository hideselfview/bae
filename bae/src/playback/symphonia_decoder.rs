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
    #[error("Unsupported format")]
    UnsupportedFormat,
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
    pub fn decode_next(&mut self) -> Result<Option<AudioBufferRef>, DecoderError> {
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

        // For FLAC, timebase is typically (1, sample_rate), so timestamp = sample_number
        // Calculate timestamp: time = timestamp * numer / denom
        // For our case: seconds = timestamp * numer / denom
        // So: timestamp = seconds * denom / numer = sample_number * denom / (sample_rate * numer)
        // But since timebase is usually (1, sample_rate), timestamp = sample_number

        // Try to construct Time - for FLAC with timebase (1, sample_rate), we can use timestamp directly
        // Calculate the timestamp value
        let seconds = sample_number as f64 / self.sample_rate as f64;

        // Use Time::from_ss for durations <= 255 seconds (its limit)
        // For longer durations, we'll need to seek in chunks or use a different approach
        // Since most tracks are shorter than 255 seconds, this should work for most cases
        // For longer tracks, we could seek to 0 and decode forward, but that's inefficient
        let seek_time = if seconds <= 255.0 {
            let secs = seconds as u8;
            let nanos = ((seconds.fract() * 1_000_000_000.0) as u32).min(999_999_999);
            Time::from_ss(secs, nanos).ok_or(DecoderError::UnsupportedFormat)?
        } else {
            // For tracks longer than 255 seconds, seek to maximum possible (255 seconds) and decode forward
            // This is not ideal but will work
            let seek_to_secs = 255u8;
            let seek_nanos = 999_999_999u32;
            Time::from_ss(seek_to_secs, seek_nanos).ok_or(DecoderError::UnsupportedFormat)?
        };

        self.format_reader.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time: seek_time,
                track_id: Some(self.track_id),
            },
        )?;

        // If we couldn't seek to the exact position (due to 255s limit),
        // decode forward to the actual position
        let seeked_seconds = if seconds <= 255.0 {
            seconds
        } else {
            // We seeked to 255 seconds, now decode forward
            let current_position = 255.0;
            let samples_to_decode = ((seconds - current_position) * self.sample_rate as f64) as u64;

            // Decode samples until we reach the desired position
            let mut decoded = 0u64;
            while decoded < samples_to_decode {
                match self.decode_next()? {
                    Some(audio_buf) => {
                        decoded += audio_buf.frames() as u64;
                    }
                    None => break, // End of stream
                }
            }
            seconds
        };

        // Update decoded_samples to match the seek position
        let final_sample_number = (seeked_seconds * self.sample_rate as f64) as u64;
        self.decoded_samples
            .store(final_sample_number, Ordering::Relaxed);

        Ok(())
    }

    /// Get the track duration, if available
    pub fn duration(&self) -> Option<std::time::Duration> {
        self.duration
    }
}
