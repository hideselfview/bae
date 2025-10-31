use crate::cache::CacheManager;
use crate::cloud_storage::CloudStorageManager;
use crate::db::DbTrack;
use crate::encryption::EncryptionService;
use crate::library::LibraryManager;
use crate::playback::cpal_output::AudioOutput;
use crate::playback::progress::{PlaybackProgress, PlaybackProgressHandle};
use crate::playback::symphonia_decoder::TrackDecoder;
use cpal::traits::StreamTrait;
use std::collections::VecDeque;
use std::sync::{mpsc, Arc};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{error, info};

/// Playback commands sent to the service
#[derive(Debug, Clone)]
pub enum PlaybackCommand {
    Play(String),           // track_id
    PlayAlbum(Vec<String>), // list of track_ids
    Pause,
    Resume,
    Stop,
    Next,
    Previous,
    Seek(std::time::Duration),
    SetVolume(f32),
}

/// Current playback state
#[derive(Debug, Clone)]
pub enum PlaybackState {
    Stopped,
    Playing {
        track: DbTrack,
        position: std::time::Duration,
        duration: Option<std::time::Duration>,
    },
    Paused {
        track: DbTrack,
        position: std::time::Duration,
        duration: Option<std::time::Duration>,
    },
    Loading {
        track_id: String,
    },
}

/// Handle to the playback service for sending commands
#[derive(Clone)]
pub struct PlaybackHandle {
    command_tx: tokio_mpsc::UnboundedSender<PlaybackCommand>,
    progress_handle: PlaybackProgressHandle,
}

impl PlaybackHandle {
    pub fn play(&self, track_id: String) {
        let _ = self.command_tx.send(PlaybackCommand::Play(track_id));
    }

    pub fn play_album(&self, track_ids: Vec<String>) {
        let _ = self.command_tx.send(PlaybackCommand::PlayAlbum(track_ids));
    }

    pub fn pause(&self) {
        let _ = self.command_tx.send(PlaybackCommand::Pause);
    }

    pub fn resume(&self) {
        let _ = self.command_tx.send(PlaybackCommand::Resume);
    }

    pub fn stop(&self) {
        let _ = self.command_tx.send(PlaybackCommand::Stop);
    }

    pub fn next(&self) {
        let _ = self.command_tx.send(PlaybackCommand::Next);
    }

    pub fn previous(&self) {
        let _ = self.command_tx.send(PlaybackCommand::Previous);
    }

    pub fn seek(&self, position: std::time::Duration) {
        let _ = self.command_tx.send(PlaybackCommand::Seek(position));
    }

    pub fn set_volume(&self, volume: f32) {
        let _ = self.command_tx.send(PlaybackCommand::SetVolume(volume));
    }

    pub async fn get_state(&self) -> PlaybackState {
        // Deprecated - use subscribe_progress instead
        PlaybackState::Stopped
    }

    pub fn subscribe_progress(&self) -> tokio_mpsc::UnboundedReceiver<PlaybackProgress> {
        self.progress_handle.subscribe_all()
    }
}

/// Playback service that manages audio playback
pub struct PlaybackService {
    library_manager: LibraryManager,
    cloud_storage: CloudStorageManager,
    cache: CacheManager,
    encryption_service: EncryptionService,
    chunk_size_bytes: usize,
    command_rx: tokio_mpsc::UnboundedReceiver<PlaybackCommand>,
    progress_tx: tokio_mpsc::UnboundedSender<PlaybackProgress>,
    queue: VecDeque<String>, // track IDs
    current_track: Option<DbTrack>,
    current_audio_data: Option<Vec<u8>>, // Cached audio data for seeking
    audio_output: AudioOutput,
    stream: Option<cpal::Stream>,
    next_decoder: Option<TrackDecoder>, // Preloaded for gapless playback
    next_audio_data: Option<Vec<u8>>,   // Preloaded audio data for gapless playback
    next_track_id: Option<String>,      // Track ID of preloaded track
    next_duration: Option<std::time::Duration>, // Duration of preloaded track
}

impl PlaybackService {
    pub fn start(
        library_manager: LibraryManager,
        cloud_storage: CloudStorageManager,
        cache: CacheManager,
        encryption_service: EncryptionService,
        chunk_size_bytes: usize,
        runtime_handle: tokio::runtime::Handle,
    ) -> PlaybackHandle {
        let (command_tx, command_rx) = tokio_mpsc::unbounded_channel();
        let (progress_tx, progress_rx) = tokio_mpsc::unbounded_channel();

        let progress_handle = PlaybackProgressHandle::new(progress_rx, runtime_handle.clone());

        let handle = PlaybackHandle {
            command_tx: command_tx.clone(),
            progress_handle: progress_handle.clone(),
        };

        // Spawn task to listen for track completion and auto-advance
        let command_tx_for_completion = command_tx.clone();
        let progress_handle_for_completion = progress_handle.clone();
        runtime_handle.spawn(async move {
            let mut progress_rx = progress_handle_for_completion.subscribe_all();
            while let Some(progress) = progress_rx.recv().await {
                if let PlaybackProgress::TrackCompleted { .. } = progress {
                    // Auto-advance to next track
                    let _ = command_tx_for_completion.send(PlaybackCommand::Next);
                }
            }
        });

        // Spawn the service task on a dedicated thread (CPAL Stream isn't Send-safe)
        std::thread::spawn(move || {
            // Create a new tokio runtime for this thread
            let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");

            rt.block_on(async move {
                let audio_output = match AudioOutput::new() {
                    Ok(output) => output,
                    Err(e) => {
                        error!("Failed to initialize audio output: {:?}", e);
                        return;
                    }
                };

                let mut service = PlaybackService {
                    library_manager,
                    cloud_storage,
                    cache,
                    encryption_service,
                    chunk_size_bytes,
                    command_rx,
                    progress_tx,
                    queue: VecDeque::new(),
                    current_track: None,
                    current_audio_data: None,
                    audio_output,
                    stream: None,
                    next_decoder: None,
                    next_audio_data: None,
                    next_track_id: None,
                    next_duration: None,
                };

                service.run().await;
            });
        });

        handle
    }

    async fn run(&mut self) {
        info!("PlaybackService started");

        while let Some(command) = self.command_rx.recv().await {
            match command {
                PlaybackCommand::Play(track_id) => {
                    self.queue.clear();
                    self.play_track(&track_id).await;
                }
                PlaybackCommand::PlayAlbum(track_ids) => {
                    self.queue.clear();
                    for track_id in track_ids {
                        self.queue.push_back(track_id);
                    }
                    if let Some(first_track) = self.queue.pop_front() {
                        self.play_track(&first_track).await;
                    }
                }
                PlaybackCommand::Pause => {
                    self.pause().await;
                }
                PlaybackCommand::Resume => {
                    self.resume().await;
                }
                PlaybackCommand::Stop => {
                    self.stop().await;
                }
                PlaybackCommand::Next => {
                    // Check if we have a preloaded track ready for gapless playback
                    if let Some((preloaded_decoder, preloaded_audio_data, preloaded_track_id)) =
                        self.next_decoder
                            .take()
                            .zip(self.next_audio_data.take())
                            .zip(self.next_track_id.take())
                            .map(|((decoder, audio_data), track_id)| {
                                (decoder, audio_data, track_id)
                            })
                    {
                        let preloaded_duration = self.next_duration.take();

                        // Use preloaded decoder for gapless playback
                        let track = match self.library_manager.get_track(&preloaded_track_id).await
                        {
                            Ok(Some(track)) => track,
                            Ok(None) => {
                                error!("Preloaded track not found: {}", preloaded_track_id);
                                self.stop().await;
                                continue;
                            }
                            Err(e) => {
                                error!("Failed to get preloaded track metadata: {}", e);
                                self.stop().await;
                                continue;
                            }
                        };

                        self.play_track_with_decoder(
                            &preloaded_track_id,
                            track,
                            preloaded_decoder,
                            preloaded_audio_data,
                            preloaded_duration,
                        )
                        .await;
                    } else if let Some(next_track) = self.queue.pop_front() {
                        // No preloaded track, reassemble from scratch
                        self.play_track(&next_track).await;
                    } else {
                        self.stop().await;
                    }
                }
                PlaybackCommand::Previous => {
                    // For now, just restart the current track
                    if let Some(track) = &self.current_track {
                        let track_id = track.id.clone();
                        self.play_track(&track_id).await;
                    }
                }
                PlaybackCommand::Seek(position) => {
                    self.seek(position).await;
                }
                PlaybackCommand::SetVolume(volume) => {
                    self.audio_output.set_volume(volume);
                }
            }
        }

        info!("PlaybackService stopped");
    }

    async fn play_track(&mut self, track_id: &str) {
        info!("Playing track: {}", track_id);

        // Update state to loading
        let _ = self.progress_tx.send(PlaybackProgress::StateChanged {
            state: PlaybackState::Loading {
                track_id: track_id.to_string(),
            },
        });

        // Fetch track metadata
        let track = match self.library_manager.get_track(track_id).await {
            Ok(Some(track)) => track,
            Ok(None) => {
                error!("Track not found: {}", track_id);
                self.stop().await;
                return;
            }
            Err(e) => {
                error!("Failed to fetch track: {}", e);
                self.stop().await;
                return;
            }
        };

        // Reassemble track chunks
        let audio_data = match super::reassembly::reassemble_track(
            track_id,
            &self.library_manager,
            &self.cloud_storage,
            &self.cache,
            &self.encryption_service,
            self.chunk_size_bytes,
        )
        .await
        {
            Ok(data) => data,
            Err(e) => {
                error!("Failed to reassemble track: {}", e);
                self.stop().await;
                return;
            }
        };

        info!("Track loaded: {} bytes", audio_data.len());

        // Validate FLAC header
        if audio_data.len() < 4 {
            error!("Audio data too small: {} bytes", audio_data.len());
            self.stop().await;
            return;
        }

        if &audio_data[0..4] != b"fLaC" {
            error!(
                "Invalid FLAC header: expected 'fLaC', got {:?}",
                &audio_data[0..4.min(audio_data.len())]
            );
            self.stop().await;
            return;
        }

        info!("Valid FLAC header detected");

        // Create decoder
        let decoder = match TrackDecoder::new(audio_data.clone()) {
            Ok(decoder) => decoder,
            Err(e) => {
                error!("Failed to create decoder: {:?}", e);
                self.stop().await;
                return;
            }
        };

        info!("Decoder created, sample rate: {} Hz", decoder.sample_rate());
        let track_duration = decoder.duration();
        if let Some(dur) = track_duration {
            info!("Track duration: {:?}", dur);
        } else {
            info!("Track duration not available");
        }

        // Cache audio data for seeking
        self.current_audio_data = Some(audio_data.clone());

        self.play_track_with_decoder(track_id, track, decoder, audio_data, track_duration)
            .await;
    }

    async fn play_track_with_decoder(
        &mut self,
        track_id: &str,
        track: DbTrack,
        decoder: TrackDecoder,
        audio_data: Vec<u8>,
        track_duration: Option<std::time::Duration>,
    ) {
        info!("Starting playback with decoder for track: {}", track_id);

        // Cache audio data for seeking
        self.current_audio_data = Some(audio_data);

        // Stop current stream if playing
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }

        // Create channels for position updates and completion
        let (position_tx, position_rx) = mpsc::channel();
        let (completion_tx, completion_rx) = mpsc::channel();

        // Create audio stream
        let stream = match self
            .audio_output
            .create_stream(decoder, position_tx, completion_tx)
        {
            Ok(stream) => {
                info!("Audio stream created successfully");
                stream
            }
            Err(e) => {
                error!("Failed to create audio stream: {:?}", e);
                self.stop().await;
                return;
            }
        };

        // Start playback
        if let Err(e) = stream.play() {
            error!("Failed to start stream: {:?}", e);
            self.stop().await;
            return;
        }

        info!("Stream started, sending Play command");

        // Send Play command to start audio
        self.audio_output
            .send_command(crate::playback::cpal_output::AudioCommand::Play);

        self.stream = Some(stream);
        self.current_track = Some(track.clone());

        // Update state
        let _ = self.progress_tx.send(PlaybackProgress::StateChanged {
            state: PlaybackState::Playing {
                track: track.clone(),
                position: std::time::Duration::ZERO,
                duration: track_duration,
            },
        });

        // Spawn task to handle position updates and completion
        let progress_tx = self.progress_tx.clone();
        let track_id = track_id.to_string();
        tokio::spawn(async move {
            // Use std::sync::Mutex for blocking recv
            let position_rx = Arc::new(std::sync::Mutex::new(position_rx));
            let completion_rx = Arc::new(std::sync::Mutex::new(completion_rx));

            loop {
                tokio::select! {
                    result = tokio::task::spawn_blocking({
                        let rx = position_rx.clone();
                        move || rx.lock().unwrap().recv()
                    }) => {
                        match result {
                            Ok(Ok(position)) => {
                                let _ = progress_tx.send(PlaybackProgress::PositionUpdate {
                                    position,
                                    track_id: track_id.clone(),
                                });
                            }
                            Ok(Err(_)) | Err(_) => break,
                        }
                    }
                    result = tokio::task::spawn_blocking({
                        let rx = completion_rx.clone();
                        move || rx.lock().unwrap().recv()
                    }) => {
                        match result {
                            Ok(Ok(())) => {
                                let _ = progress_tx.send(PlaybackProgress::TrackCompleted {
                                    track_id: track_id.clone(),
                                });
                                break;
                            }
                            Ok(Err(_)) | Err(_) => break,
                        }
                    }
                }
            }
        });

        // Preload next track for gapless playback
        if let Some(next_track_id) = self.queue.front().cloned() {
            self.preload_next_track(&next_track_id).await;
        }
    }

    async fn preload_next_track(&mut self, track_id: &str) {
        // Reassemble track chunks
        let audio_data = match super::reassembly::reassemble_track(
            track_id,
            &self.library_manager,
            &self.cloud_storage,
            &self.cache,
            &self.encryption_service,
            self.chunk_size_bytes,
        )
        .await
        {
            Ok(data) => data,
            Err(e) => {
                error!("Failed to preload track {}: {}", track_id, e);
                return;
            }
        };

        // Create decoder
        if let Ok(decoder) = TrackDecoder::new(audio_data.clone()) {
            let duration = decoder.duration();
            self.next_decoder = Some(decoder);
            self.next_audio_data = Some(audio_data);
            self.next_track_id = Some(track_id.to_string());
            self.next_duration = duration;
            info!("Preloaded next track: {}", track_id);
        }
    }

    async fn pause(&mut self) {
        self.audio_output
            .send_command(crate::playback::cpal_output::AudioCommand::Pause);

        // State will be updated via PositionUpdate, so we don't need to send StateChanged here
        // The position updates will preserve the duration
    }

    async fn resume(&mut self) {
        self.audio_output
            .send_command(crate::playback::cpal_output::AudioCommand::Resume);

        // State will be updated via PositionUpdate, so we don't need to send StateChanged here
        // The position updates will preserve the duration
    }

    async fn stop(&mut self) {
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }
        self.current_track = None;
        self.current_audio_data = None;
        self.next_decoder = None;
        self.next_audio_data = None;
        self.next_track_id = None;
        self.next_duration = None;
        self.audio_output
            .send_command(crate::playback::cpal_output::AudioCommand::Stop);

        let _ = self.progress_tx.send(PlaybackProgress::StateChanged {
            state: PlaybackState::Stopped,
        });
    }

    async fn seek(&mut self, position: std::time::Duration) {
        // Can only seek if we have a current track and cached audio data
        let (track_id, audio_data) = match (&self.current_track, &self.current_audio_data) {
            (Some(track), Some(audio_data)) => (track.id.clone(), audio_data.clone()),
            _ => {
                error!("Cannot seek: no track playing or audio data not cached");
                return;
            }
        };

        info!("Seeking to position: {:?}", position);

        // Stop current stream
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }

        // Create new decoder with seeked position
        let decoder = match TrackDecoder::new(audio_data.clone()) {
            Ok(decoder) => decoder,
            Err(e) => {
                error!("Failed to create decoder for seek: {:?}", e);
                self.stop().await;
                return;
            }
        };

        let decoder_duration = decoder.duration();

        // Seek decoder to desired position
        let mut decoder = decoder;
        if let Err(e) = decoder.seek(position) {
            error!("Failed to seek decoder: {:?}", e);
            self.stop().await;
            return;
        }

        // Create channels for position updates and completion
        let (position_tx, position_rx) = mpsc::channel();
        let (completion_tx, completion_rx) = mpsc::channel();

        // Create new audio stream with seeked decoder
        let mut audio_output_clone = match AudioOutput::new() {
            Ok(output) => output,
            Err(e) => {
                error!("Failed to create audio output for seek: {:?}", e);
                self.stop().await;
                return;
            }
        };

        let stream = match audio_output_clone.create_stream(decoder, position_tx, completion_tx) {
            Ok(stream) => stream,
            Err(e) => {
                error!("Failed to create audio stream for seek: {:?}", e);
                self.stop().await;
                return;
            }
        };

        // Start playback from seeked position
        if let Err(e) = stream.play() {
            error!("Failed to start stream after seek: {:?}", e);
            self.stop().await;
            return;
        }

        self.stream = Some(stream);
        self.audio_output = audio_output_clone;

        // Send Play command to start audio
        self.audio_output
            .send_command(crate::playback::cpal_output::AudioCommand::Play);

        // Update state with new position
        if let Some(track) = &self.current_track {
            // Get duration from decoder if available
            let _ = self.progress_tx.send(PlaybackProgress::StateChanged {
                state: PlaybackState::Playing {
                    track: track.clone(),
                    position,
                    duration: decoder_duration,
                },
            });
        }

        // Spawn task to handle position updates and completion (same as play_track)
        let progress_tx = self.progress_tx.clone();
        let track_id_clone = track_id.clone();
        tokio::spawn(async move {
            let position_rx = Arc::new(std::sync::Mutex::new(position_rx));
            let completion_rx = Arc::new(std::sync::Mutex::new(completion_rx));

            loop {
                tokio::select! {
                    result = tokio::task::spawn_blocking({
                        let rx = position_rx.clone();
                        move || rx.lock().unwrap().recv()
                    }) => {
                        match result {
                            Ok(Ok(pos)) => {
                                let _ = progress_tx.send(PlaybackProgress::PositionUpdate {
                                    position: pos,
                                    track_id: track_id_clone.clone(),
                                });
                            }
                            Ok(Err(_)) | Err(_) => break,
                        }
                    }
                    result = tokio::task::spawn_blocking({
                        let rx = completion_rx.clone();
                        move || rx.lock().unwrap().recv()
                    }) => {
                        match result {
                            Ok(Ok(())) => {
                                let _ = progress_tx.send(PlaybackProgress::TrackCompleted {
                                    track_id: track_id_clone.clone(),
                                });
                                break;
                            }
                            Ok(Err(_)) | Err(_) => break,
                        }
                    }
                }
            }
        });
    }
}
