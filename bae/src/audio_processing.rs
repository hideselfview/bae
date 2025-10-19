use std::io::Cursor;
use symphonia::core::{
    audio::Signal, codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream,
    meta::MetadataOptions, probe::Hint, units::Time,
};
use thiserror::Error;
use tracing::debug;

#[derive(Debug, Error)]
pub enum AudioProcessingError {
    #[error("Symphonia error: {0}")]
    Symphonia(#[from] symphonia::core::errors::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("No audio tracks found")]
    NoAudioTracks,
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("Seeking failed: {0}")]
    SeekingFailed(String),
}

/// Audio processor for precise track boundary extraction
pub struct AudioProcessor;

impl AudioProcessor {
    /// Extract precise track boundaries from FLAC data
    /// Returns the exact audio data for the specified time range
    pub fn extract_track_from_flac(
        flac_data: &[u8],
        start_time_ms: u64,
        end_time_ms: u64,
    ) -> Result<Vec<u8>, AudioProcessingError> {
        debug!(
            "Extracting track from {}ms to {}ms",
            start_time_ms, end_time_ms
        );

        // Create a cursor over the FLAC data
        let cursor = Cursor::new(flac_data.to_vec()); // Convert to owned Vec to fix lifetime
        let media_source = MediaSourceStream::new(Box::new(cursor), Default::default());

        // Create a hint for FLAC format
        let mut hint = Hint::new();
        hint.with_extension("flac");

        // Probe the format
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
            .ok_or(AudioProcessingError::NoAudioTracks)?;

        let track_id = track.id;

        // Create decoder
        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())?;

        // Convert milliseconds to sample time
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100) as f64;
        let start_sample = ((start_time_ms as f64 / 1000.0) * sample_rate) as u64;
        let end_sample = ((end_time_ms as f64 / 1000.0) * sample_rate) as u64;

        debug!("Seeking to sample {} ({}ms)", start_sample, start_time_ms);

        // Seek to start position
        let seconds = (start_time_ms / 1000) as u8;
        let nanoseconds = ((start_time_ms % 1000) * 1000000) as u32;
        let seek_time = Time::from_ss(seconds, nanoseconds).ok_or_else(|| {
            AudioProcessingError::SeekingFailed("Invalid time format".to_string())
        })?;
        format_reader.seek(
            symphonia::core::formats::SeekMode::Accurate,
            symphonia::core::formats::SeekTo::Time {
                time: seek_time,
                track_id: Some(track_id),
            },
        )?;

        // Collect audio samples
        let mut audio_samples = Vec::new();
        let mut current_sample = start_sample;

        loop {
            // Read packet
            let packet = match format_reader.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break; // End of stream
                }
                Err(e) => return Err(AudioProcessingError::Symphonia(e)),
            };

            // Skip packets not for our track
            if packet.track_id() != track_id {
                continue;
            }

            // Decode packet
            let audio_buf = decoder.decode(&packet)?;

            // Extract samples based on the audio buffer type
            match audio_buf {
                symphonia::core::audio::AudioBufferRef::S16(buf) => {
                    let samples = buf.chan(0); // Get first channel for now
                    for &sample in samples {
                        if current_sample >= end_sample {
                            break;
                        }
                        audio_samples.push(sample);
                        current_sample += 1;
                    }
                }
                symphonia::core::audio::AudioBufferRef::S32(buf) => {
                    let samples = buf.chan(0);
                    for &sample in samples {
                        if current_sample >= end_sample {
                            break;
                        }
                        // Convert S32 to S16 for simplicity
                        audio_samples.push((sample >> 16) as i16);
                        current_sample += 1;
                    }
                }
                symphonia::core::audio::AudioBufferRef::F32(buf) => {
                    let samples = buf.chan(0);
                    for &sample in samples {
                        if current_sample >= end_sample {
                            break;
                        }
                        // Convert F32 to S16
                        let sample_i16 = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                        audio_samples.push(sample_i16);
                        current_sample += 1;
                    }
                }
                _ => {
                    return Err(AudioProcessingError::UnsupportedFormat(
                        "Unsupported audio buffer format".to_string(),
                    ));
                }
            }

            if current_sample >= end_sample {
                break;
            }
        }

        debug!("Extracted {} samples", audio_samples.len());

        // Convert samples back to bytes (simple PCM for now)
        // TODO: Re-encode as FLAC for better compression
        let mut result = Vec::new();
        for sample in audio_samples {
            result.extend_from_slice(&sample.to_le_bytes());
        }

        Ok(result)
    }
}
