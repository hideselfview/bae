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
use tracing::debug;

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

        let mut format_reader = probed.format;

        // Find the audio track
        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or(DecoderError::NoAudioTracks)?;

        let track_id = track.id;
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);

        // Try to get duration from track parameters or metadata
        let duration = if let Some(n_frames) = track.codec_params.n_frames {
            // Duration from number of frames
            Some(std::time::Duration::from_secs_f64(
                n_frames as f64 / sample_rate as f64,
            ))
        } else {
            // Try to get duration from metadata tags
            // Note: This is a fallback - ideally duration should be stored during import
            // FLAC files don't always have duration in metadata tags, so this may return None
            // In that case, the progress bar will still work but won't show total duration
            None
        };

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

    /// Get the number of decoded samples
    pub fn decoded_samples(&self) -> u64 {
        self.decoded_samples.load(Ordering::Relaxed)
    }

    /// Seek to a specific position
    pub fn seek(&mut self, position: std::time::Duration) -> Result<(), DecoderError> {
        let seconds = position.as_secs();
        let nanos = position.subsec_nanos();
        let seek_time = Time::from_ss(seconds.min(255) as u8, nanos)
            .ok_or_else(|| DecoderError::UnsupportedFormat)?;

        self.format_reader.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time: seek_time,
                track_id: Some(self.track_id),
            },
        )?;

        // Calculate the sample position from the seek time and update decoded_samples
        // This ensures position() returns the correct value after seeking
        let position_seconds = position.as_secs_f64();
        let samples_at_position = (position_seconds * self.sample_rate as f64) as u64;
        self.decoded_samples
            .store(samples_at_position, Ordering::Relaxed);

        Ok(())
    }

    /// Get the track duration, if available
    pub fn duration(&self) -> Option<std::time::Duration> {
        self.duration
    }

    /// Check if we've reached the end of the track
    pub fn is_finished(&self) -> bool {
        if let Some(duration) = self.duration {
            self.position() >= duration
        } else {
            false
        }
    }

    /// Reset the decoder to the beginning
    pub fn reset(&mut self) -> Result<(), DecoderError> {
        self.seek(std::time::Duration::ZERO)
    }
}
