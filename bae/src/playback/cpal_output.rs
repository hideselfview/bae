use crate::playback::symphonia_decoder::{DecoderError, TrackDecoder};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream, StreamConfig};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use symphonia::core::audio::{AudioBufferRef, Signal};
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub enum AudioCommand {
    Play,
    Pause,
    Resume,
    Stop,
    SetVolume(f32),
}

#[derive(Debug)]
pub enum AudioError {
    DeviceNotFound,
    StreamConfigError(String),
    StreamBuildError(String),
    DecoderError(DecoderError),
}

impl From<DecoderError> for AudioError {
    fn from(e: DecoderError) -> Self {
        AudioError::DecoderError(e)
    }
}

/// Audio output manager using CPAL
pub struct AudioOutput {
    device: Device,
    stream_config: StreamConfig,
    command_tx: mpsc::Sender<AudioCommand>,
    is_playing: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    volume: Arc<AtomicU32>, // 0-10000 (0.0-1.0 scaled)
}

impl AudioOutput {
    /// Create a new audio output manager
    pub fn new() -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(AudioError::DeviceNotFound)?;

        let default_config = device
            .default_output_config()
            .map_err(|e| AudioError::StreamConfigError(e.to_string()))?;

        let sample_format = default_config.sample_format();
        let stream_config = StreamConfig::from(default_config.clone());

        info!(
            "Audio device: {} channels, {} Hz, {:?}",
            stream_config.channels, stream_config.sample_rate.0, sample_format
        );

        let (command_tx, _command_rx) = mpsc::channel();

        Ok(Self {
            device,
            stream_config,
            command_tx,
            is_playing: Arc::new(AtomicBool::new(false)),
            is_paused: Arc::new(AtomicBool::new(false)),
            volume: Arc::new(AtomicU32::new(10000)), // 1.0
        })
    }

    /// Create a stream with decoder callback
    pub fn create_stream(
        &mut self,
        mut decoder: TrackDecoder,
        position_tx: mpsc::Sender<std::time::Duration>,
        completion_tx: mpsc::Sender<()>,
    ) -> Result<Stream, AudioError> {
        let sample_rate = self.stream_config.sample_rate.0;
        let channels = self.stream_config.channels as usize;
        let decoder_sample_rate = decoder.sample_rate();

        // Sample rate conversion factor
        let sample_rate_ratio = decoder_sample_rate as f64 / sample_rate as f64;

        let is_playing = self.is_playing.clone();
        let is_paused = self.is_paused.clone();
        let volume = self.volume.clone();

        // Create a new command channel for this stream
        // This allows us to create multiple streams without reusing the same receiver
        let (command_tx_for_stream, mut command_rx) = mpsc::channel();

        // Update our command_tx to point to the new sender
        self.command_tx = command_tx_for_stream;

        // Buffer to hold decoded samples (interleaved, converted to target format)
        let mut sample_buffer = Vec::new();
        let mut buffer_pos = 0usize;

        // Track position for updates
        let mut last_position_update = std::time::Instant::now();
        let position_update_interval = std::time::Duration::from_millis(250);

        let stream = self
            .device
            .build_output_stream(
                &self.stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // Check commands
                    while let Ok(cmd) = command_rx.try_recv() {
                        match cmd {
                            AudioCommand::Play => {
                                is_playing.store(true, Ordering::Relaxed);
                                is_paused.store(false, Ordering::Relaxed);
                            }
                            AudioCommand::Pause => {
                                is_paused.store(true, Ordering::Relaxed);
                            }
                            AudioCommand::Resume => {
                                is_paused.store(false, Ordering::Relaxed);
                            }
                            AudioCommand::Stop => {
                                is_playing.store(false, Ordering::Relaxed);
                                is_paused.store(false, Ordering::Relaxed);
                            }
                            AudioCommand::SetVolume(vol) => {
                                volume.store(
                                    (vol.clamp(0.0, 1.0) * 10000.0) as u32,
                                    Ordering::Relaxed,
                                );
                            }
                        }
                    }

                    if !is_playing.load(Ordering::Relaxed) || is_paused.load(Ordering::Relaxed) {
                        // Fill with silence
                        data.fill(0.0);
                        return;
                    }

                    let vol = volume.load(Ordering::Relaxed) as f32 / 10000.0;

                    // Fill output buffer
                    let mut output_pos = 0;
                    while output_pos < data.len() {
                        // If we need more samples, decode next packet
                        if buffer_pos >= sample_buffer.len() {
                            match decoder.decode_next() {
                                Ok(Some(audio_buf)) => {
                                    // Convert symphonia audio buffer to f32 samples
                                    sample_buffer.clear();
                                    buffer_pos = 0;

                                    let frames = audio_buf.frames();
                                    let decoder_channels = audio_buf.spec().channels.count();

                                    match audio_buf {
                                        AudioBufferRef::F32(buf) => {
                                            // Already f32, just interleave channels
                                            for frame_idx in 0..frames {
                                                for ch in 0..decoder_channels {
                                                    sample_buffer.push(buf.chan(ch)[frame_idx]);
                                                }
                                            }
                                        }
                                        AudioBufferRef::S16(buf) => {
                                            // Convert i16 to f32
                                            for frame_idx in 0..frames {
                                                for ch in 0..decoder_channels {
                                                    let sample =
                                                        buf.chan(ch)[frame_idx] as f32 / 32768.0;
                                                    sample_buffer.push(sample);
                                                }
                                            }
                                        }
                                        AudioBufferRef::S32(buf) => {
                                            // Convert i32 to f32
                                            for frame_idx in 0..frames {
                                                for ch in 0..decoder_channels {
                                                    let sample = buf.chan(ch)[frame_idx] as f32
                                                        / 2147483648.0;
                                                    sample_buffer.push(sample);
                                                }
                                            }
                                        }
                                        _ => {
                                            warn!("Unsupported audio buffer format");
                                            data.fill(0.0);
                                            return;
                                        }
                                    }

                                    // Apply sample rate conversion if needed
                                    if sample_rate_ratio != 1.0 {
                                        // Simple linear interpolation resampling
                                        let mut resampled = Vec::new();
                                        let input_channels = decoder_channels;
                                        let input_frames = frames;
                                        let output_frames =
                                            (input_frames as f64 / sample_rate_ratio) as usize;

                                        for frame_idx in 0..output_frames {
                                            let src_idx =
                                                (frame_idx as f64 * sample_rate_ratio) as usize;
                                            if src_idx < input_frames {
                                                for ch in 0..input_channels {
                                                    let idx = src_idx * input_channels + ch;
                                                    if idx < sample_buffer.len() {
                                                        resampled.push(sample_buffer[idx]);
                                                    } else {
                                                        resampled.push(0.0);
                                                    }
                                                }
                                            } else {
                                                for _ch in 0..input_channels {
                                                    resampled.push(0.0);
                                                }
                                            }
                                        }
                                        sample_buffer = resampled;
                                    }

                                    // Channel conversion if needed
                                    if decoder_channels != channels {
                                        let mut converted = Vec::new();
                                        let frames = sample_buffer.len() / decoder_channels;
                                        for frame_idx in 0..frames {
                                            let base_idx = frame_idx * decoder_channels;
                                            if channels == 1 && decoder_channels >= 1 {
                                                // Mono: take first channel
                                                converted.push(sample_buffer[base_idx]);
                                            } else if channels == 2 && decoder_channels == 1 {
                                                // Stereo from mono: duplicate
                                                let sample = sample_buffer[base_idx];
                                                converted.push(sample);
                                                converted.push(sample);
                                            } else if channels == 2 && decoder_channels >= 2 {
                                                // Stereo: take first two channels
                                                converted.push(sample_buffer[base_idx]);
                                                converted.push(sample_buffer[base_idx + 1]);
                                            } else {
                                                // Fallback: fill with zeros
                                                for _ in 0..channels {
                                                    converted.push(0.0);
                                                }
                                            }
                                        }
                                        sample_buffer = converted;
                                    }
                                }
                                Ok(None) => {
                                    // End of stream
                                    is_playing.store(false, Ordering::Relaxed);
                                    let _ = completion_tx.send(());
                                    data.fill(0.0);
                                    return;
                                }
                                Err(e) => {
                                    error!("Decoder error: {:?}", e);
                                    is_playing.store(false, Ordering::Relaxed);
                                    data.fill(0.0);
                                    return;
                                }
                            }
                        }

                        // Copy samples from buffer to output
                        while output_pos < data.len() && buffer_pos < sample_buffer.len() {
                            data[output_pos] = sample_buffer[buffer_pos] * vol;
                            output_pos += 1;
                            buffer_pos += 1;
                        }
                    }

                    // Send position update periodically
                    if last_position_update.elapsed() >= position_update_interval {
                        let _ = position_tx.send(decoder.position());
                        last_position_update = std::time::Instant::now();
                    }
                },
                |err| {
                    error!("Audio stream error: {:?}", err);
                },
                None,
            )
            .map_err(|e| AudioError::StreamBuildError(e.to_string()))?;

        Ok(stream)
    }

    pub fn send_command(&self, cmd: AudioCommand) {
        let _ = self.command_tx.send(cmd);
    }

    pub fn is_playing(&self) -> bool {
        self.is_playing.load(Ordering::SeqCst)
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::SeqCst)
    }

    pub fn set_volume(&self, volume: f32) {
        self.send_command(AudioCommand::SetVolume(volume));
    }
}

impl Default for AudioOutput {
    fn default() -> Self {
        Self::new().expect("Failed to initialize audio output")
    }
}
