use crate::cache::CacheManager;
use crate::cloud_storage::CloudStorageManager;
use crate::db::{DbAudioFormat, DbTrack};
use crate::encryption::EncryptionService;
use crate::library::LibraryManager;
use crate::playback::cpal_output::AudioOutput;
use crate::playback::progress::{PlaybackProgress, PlaybackProgressHandle};
use crate::playback::symphonia_decoder::TrackDecoder;
use crate::playback::{ChunkBuffer, StreamingChunkSource};
use cpal::traits::StreamTrait;
use std::collections::VecDeque;
use std::sync::{mpsc, Arc};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{error, info, trace, warn};

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
    runtime_handle: tokio::runtime::Handle,
    command_rx: tokio_mpsc::UnboundedReceiver<PlaybackCommand>,
    progress_tx: tokio_mpsc::UnboundedSender<PlaybackProgress>,
    queue: VecDeque<String>,           // track IDs
    previous_track_id: Option<String>, // Track ID of the previous track
    current_track: Option<DbTrack>,
    current_audio_data: Option<Vec<u8>>, // Cached audio data for seeking (legacy, for non-streaming)
    current_audio_format: Option<DbAudioFormat>, // Current track's audio format (for seeking)
    current_coords: Option<crate::db::DbTrackChunkCoords>, // Current track's chunk coordinates
    current_chunk_buffer: Option<Arc<ChunkBuffer>>, // Current track's chunk buffer
    current_position: Option<std::time::Duration>, // Current playback position
    current_duration: Option<std::time::Duration>, // Current track duration
    is_paused: bool,                     // Whether playback is currently paused
    current_position_shared: Arc<std::sync::Mutex<Option<std::time::Duration>>>, // Shared position for bridge tasks
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
                if let PlaybackProgress::TrackCompleted { track_id } = progress {
                    info!(
                        "Auto-advance: Track completed, sending Next command: {}",
                        track_id
                    );
                    // Auto-advance to next track
                    let _ = command_tx_for_completion.send(PlaybackCommand::Next);
                }
            }
        });

        // Check if we're in test mode before spawning thread
        let is_test_mode = std::env::var("BAE_TEST_MODE").is_ok();
        if is_test_mode {
            info!("BAE_TEST_MODE detected - will use mock audio output");
        }

        // Spawn the service task on a dedicated thread (CPAL Stream isn't Send-safe)
        std::thread::spawn(move || {
            // Create a new tokio runtime for this thread
            let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
            let rt_handle = rt.handle().clone();

            rt.block_on(async move {
                let audio_output = if is_test_mode {
                    info!("Test mode enabled - using mock audio output");
                    AudioOutput::new_mock()
                } else {
                    match AudioOutput::new() {
                        Ok(output) => output,
                        Err(e) => {
                            error!("Failed to initialize audio output: {:?}", e);
                            return;
                        }
                    }
                };

                let mut service = PlaybackService {
                    library_manager,
                    cloud_storage,
                    cache,
                    encryption_service,
                    chunk_size_bytes,
                    runtime_handle: rt_handle,
                    command_rx,
                    progress_tx,
                    queue: VecDeque::new(),
                    previous_track_id: None,
                    current_track: None,
                    current_audio_data: None,
                    current_audio_format: None,
                    current_coords: None,
                    current_chunk_buffer: None,
                    current_position: None,
                    current_duration: None,
                    is_paused: false,
                    current_position_shared: Arc::new(std::sync::Mutex::new(None)),
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
                    // Stop current playback before switching tracks (without state change)
                    if let Some(stream) = self.stream.take() {
                        drop(stream);
                    }
                    self.audio_output
                        .send_command(crate::playback::cpal_output::AudioCommand::Stop);
                    // Clear preloaded data
                    self.next_decoder = None;
                    self.next_audio_data = None;
                    self.next_track_id = None;
                    self.next_duration = None;

                    // Save current track as previous before switching
                    if let Some(current_track) = &self.current_track {
                        self.previous_track_id = Some(current_track.id.clone());
                    }

                    // Clear queue
                    self.queue.clear();

                    // Fetch track to get release_id
                    if let Ok(Some(track)) = self.library_manager.get_track(&track_id).await {
                        // Get all tracks for this release
                        if let Ok(mut release_tracks) =
                            self.library_manager.get_tracks(&track.release_id).await
                        {
                            // Sort tracks by track_number for proper ordering
                            release_tracks.sort_by(|a, b| match (a.track_number, b.track_number) {
                                (Some(a_num), Some(b_num)) => a_num.cmp(&b_num),
                                (Some(_), None) => std::cmp::Ordering::Less,
                                (None, Some(_)) => std::cmp::Ordering::Greater,
                                (None, None) => std::cmp::Ordering::Equal,
                            });

                            // If we don't have a previous track (starting fresh), set it based on album order
                            if self.previous_track_id.is_none() {
                                let mut previous_track_id = None;
                                for release_track in &release_tracks {
                                    if release_track.id == track_id {
                                        break;
                                    }
                                    previous_track_id = Some(release_track.id.clone());
                                }
                                self.previous_track_id = previous_track_id;
                            }

                            // Add remaining tracks to queue (tracks after the current one)
                            let mut found_current = false;
                            for release_track in release_tracks {
                                if found_current {
                                    self.queue.push_back(release_track.id);
                                } else if release_track.id == track_id {
                                    found_current = true;
                                }
                            }
                        }
                    }

                    // Play the selected track
                    self.play_track(&track_id).await;
                }
                PlaybackCommand::PlayAlbum(track_ids) => {
                    // Save current track as previous before switching
                    if let Some(current_track) = &self.current_track {
                        self.previous_track_id = Some(current_track.id.clone());
                    }

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
                    info!("Next command received, queue length: {}", self.queue.len());
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
                        let preloaded_duration = self
                            .next_duration
                            .take()
                            .expect("Preloaded track has no duration");
                        info!("Using preloaded track: {}", preloaded_track_id);

                        // Save current track as previous before switching
                        if let Some(current_track) = &self.current_track {
                            self.previous_track_id = Some(current_track.id.clone());
                        }

                        // Remove the preloaded track from the queue if it's at the front
                        // This ensures play_track_with_decoder preloads the NEXT track
                        if self
                            .queue
                            .front()
                            .map(|id| id == &preloaded_track_id)
                            .unwrap_or(false)
                        {
                            self.queue.pop_front();
                        }

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
                        info!("No preloaded track, playing from queue: {}", next_track);
                        // Save current track as previous before switching
                        if let Some(current_track) = &self.current_track {
                            self.previous_track_id = Some(current_track.id.clone());
                        }
                        // No preloaded track, reassemble from scratch
                        self.play_track(&next_track).await;
                    } else {
                        info!("No next track available, stopping");
                        self.stop().await;
                    }
                }
                PlaybackCommand::Previous => {
                    if let Some(track) = &self.current_track {
                        let current_position = self
                            .current_position_shared
                            .lock()
                            .unwrap()
                            .unwrap_or(std::time::Duration::ZERO);

                        // If we're less than 3 seconds into the track, go to previous track
                        // Otherwise restart the current track
                        if current_position < std::time::Duration::from_secs(3) {
                            if let Some(previous_track_id) = self.previous_track_id.clone() {
                                info!("Going to previous track: {}", previous_track_id);

                                // Update previous_track_id for the track we're navigating to
                                // based on album order, similar to Play command
                                if let Ok(Some(previous_track)) =
                                    self.library_manager.get_track(&previous_track_id).await
                                {
                                    if let Ok(mut release_tracks) = self
                                        .library_manager
                                        .get_tracks(&previous_track.release_id)
                                        .await
                                    {
                                        release_tracks.sort_by(|a, b| {
                                            match (a.track_number, b.track_number) {
                                                (Some(a_num), Some(b_num)) => a_num.cmp(&b_num),
                                                (Some(_), None) => std::cmp::Ordering::Less,
                                                (None, Some(_)) => std::cmp::Ordering::Greater,
                                                (None, None) => std::cmp::Ordering::Equal,
                                            }
                                        });

                                        // Find the previous track for the track we're navigating to
                                        let mut new_previous_track_id = None;
                                        for release_track in &release_tracks {
                                            if release_track.id == previous_track_id {
                                                break;
                                            }
                                            new_previous_track_id = Some(release_track.id.clone());
                                        }
                                        self.previous_track_id = new_previous_track_id;

                                        // Rebuild queue for the track we're navigating to
                                        self.queue.clear();
                                        let mut found_current = false;
                                        for release_track in release_tracks {
                                            if found_current {
                                                self.queue.push_back(release_track.id);
                                            } else if release_track.id == previous_track_id {
                                                found_current = true;
                                            }
                                        }
                                    }
                                }

                                // Clear preloaded data before switching tracks
                                self.next_decoder = None;
                                self.next_audio_data = None;
                                self.next_track_id = None;
                                self.next_duration = None;

                                self.play_track(&previous_track_id).await;
                            } else {
                                // No previous track, restart current track
                                info!("No previous track, restarting current track");
                                let track_id = track.id.clone();
                                self.play_track(&track_id).await;
                            }
                        } else {
                            // More than 3 seconds in, restart current track
                            // Preserve previous_track_id so we can still go back after restarting
                            info!("Restarting current track from beginning");
                            let track_id = track.id.clone();
                            let saved_previous = self.previous_track_id.clone();
                            self.play_track(&track_id).await;
                            // Restore previous_track_id after play_track updates it
                            // This allows going back to the original previous track after restart
                            if saved_previous.is_some() {
                                self.previous_track_id = saved_previous;
                            }
                        }
                    }
                }
                PlaybackCommand::Seek(position) => {
                    info!("Seek command received: {:?}", position);
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

        // Get track chunk coordinates
        let coords = match self.library_manager.get_track_chunk_coords(track_id).await {
            Ok(Some(coords)) => coords,
            Ok(None) => {
                error!("No chunk coordinates found for track {}", track_id);
                self.stop().await;
                return;
            }
            Err(e) => {
                error!("Failed to get chunk coordinates: {}", e);
                self.stop().await;
                return;
            }
        };

        // Get audio format (has FLAC headers if needed)
        let audio_format = match self
            .library_manager
            .get_audio_format_by_track_id(track_id)
            .await
        {
            Ok(Some(format)) => format,
            Ok(None) => {
                error!("No audio format found for track {}", track_id);
                self.stop().await;
                return;
            }
            Err(e) => {
                error!("Failed to get audio format: {}", e);
                self.stop().await;
                return;
            }
        };

        // Create chunk buffer for this release
        let chunk_buffer = Arc::new(ChunkBuffer::new(
            self.library_manager.clone(),
            self.cloud_storage.clone(),
            self.cache.clone(),
            self.encryption_service.clone(),
            track.release_id.clone(),
        ));

        // Fetch first 5 chunks before starting playback (cache them)
        // Prefetched chunks will "graduate" to cache when track starts playing
        info!("Fetching first 5 chunks for streaming playback...");
        match chunk_buffer
            .ensure_chunks_loaded(
                coords.start_chunk_index,
                coords.end_chunk_index,
                5,    // Minimum 5 chunks before starting
                true, // Cache chunks for currently playing track
            )
            .await
        {
            Ok(count) => {
                info!("Loaded {} chunks, starting playback", count);
            }
            Err(e) => {
                error!("Failed to load initial chunks: {}", e);
                self.stop().await;
                return;
            }
        }

        // Log the total_samples from stored headers (for debugging)
        if let Some(ref headers) = audio_format.flac_headers {
            if headers.len() >= 26 {
                let byte_21 = headers[21];
                let total_samples = ((byte_21 & 0x0F) as u64) << 32
                    | (headers[22] as u64) << 24
                    | (headers[23] as u64) << 16
                    | (headers[24] as u64) << 8
                    | (headers[25] as u64);
                info!("📊 Loaded headers for track {} - STREAMINFO total_samples: {} (~{:.1}s at 44100Hz)",
                    track_id, total_samples, total_samples as f64 / 44100.0);
            }
        }

        // Create streaming chunk source
        let streaming_source = StreamingChunkSource::new(
            chunk_buffer.clone(),
            coords.clone(),
            self.chunk_size_bytes,
            audio_format.flac_headers.clone(),
            self.runtime_handle.clone(),
        );

        // Create decoder from streaming source
        let decoder = match TrackDecoder::from_streaming_source(streaming_source) {
            Ok(decoder) => decoder,
            Err(e) => {
                error!("Failed to create decoder: {:?}", e);
                self.stop().await;
                return;
            }
        };

        info!("Decoder created, sample rate: {} Hz", decoder.sample_rate());

        // Use stored duration from database - required for playback
        let track_duration = track
            .duration_ms
            .map(|ms| std::time::Duration::from_millis(ms as u64))
            .unwrap_or_else(|| panic!("Cannot play track {} without duration", track_id));

        info!("Track duration: {:?}", track_duration);

        // Store audio format and coords for seeking
        // Note: We don't cache full audio_data anymore with streaming
        self.current_audio_data = None;

        // Spawn background task to continue fetching chunks as playback progresses
        // This ensures chunks are ready before they're needed
        // IMPORTANT: This task is seek-aware - it bases prefetching on current playback position,
        // not sequential loading, so seeks don't cause sequential chunk loads from the beginning
        let chunk_buffer_clone = chunk_buffer.clone();
        let coords_clone = coords.clone();
        let position_shared = self.current_position_shared.clone();
        let track_duration_clone = track_duration;
        let chunks_span = (coords.end_chunk_index - coords.start_chunk_index + 1) as u64;
        tokio::spawn(async move {
            let mut last_prefetched_chunk: Option<i32> = None;

            loop {
                // Check position periodically and ensure chunks ahead are loaded
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;

                let current_position = {
                    let pos = position_shared.lock().unwrap();
                    *pos
                };

                if current_position.is_none() {
                    // Playback stopped
                    break;
                }

                // Calculate which chunk we're currently in based on playback position
                // This makes the prefetch seek-aware - after a seek, it will prefetch from the new position
                if let Some(position) = current_position {
                    let position_ms = position.as_millis() as u64;
                    let track_duration_ms = track_duration_clone.as_millis() as u64;

                    if track_duration_ms > 0 && chunks_span > 0 {
                        let chunk_offset = (position_ms * chunks_span) / track_duration_ms;
                        let current_chunk = (coords_clone.start_chunk_index as u64
                            + chunk_offset.min(chunks_span))
                            as i32;

                        // Only prefetch if we've moved to a different chunk or this is the first check
                        // This prevents constant database queries when position hasn't changed
                        let should_prefetch = match last_prefetched_chunk {
                            None => true,                        // First time - always prefetch
                            Some(last) => current_chunk != last, // Only if we've moved to a different chunk
                        };

                        if should_prefetch {
                            // Prefetch 5 chunks ahead of current position
                            let prefetch_start =
                                (current_chunk + 1).max(coords_clone.start_chunk_index);
                            let prefetch_end =
                                (current_chunk + 5).min(coords_clone.end_chunk_index);

                            if prefetch_start <= coords_clone.end_chunk_index {
                                let _ = chunk_buffer_clone
                                    .ensure_chunks_loaded(prefetch_start, prefetch_end, 5, true)
                                    .await;
                                last_prefetched_chunk = Some(current_chunk);
                            }
                        }
                    }
                }
            }
        });

        // Spawn background task to prefetch adjacent tracks for gapless playback
        let library_manager = self.library_manager.clone();
        let cloud_storage = self.cloud_storage.clone();
        let cache = self.cache.clone();
        let encryption_service = self.encryption_service.clone();
        let current_release_id = track.release_id.clone();
        let previous_track_id = self.previous_track_id.clone();
        let next_track_id = self.queue.front().cloned();
        let current_chunk_buffer = chunk_buffer.clone();

        tokio::spawn(async move {
            // Get chunk coordinates for previous and next tracks
            let (previous_coords, prev_id_opt) = if let Some(prev_id) = previous_track_id.clone() {
                (
                    Self::get_track_coords_for_prefetch(&prev_id, &library_manager)
                        .await
                        .ok(),
                    Some(prev_id),
                )
            } else {
                (None, None)
            };

            let (next_coords, next_id_opt) = if let Some(next_id) = next_track_id.clone() {
                (
                    Self::get_track_coords_for_prefetch(&next_id, &library_manager)
                        .await
                        .ok(),
                    Some(next_id),
                )
            } else {
                (None, None)
            };

            // Prefetch using existing buffer if same release, otherwise create new buffers
            if let (Some(prev_coords), Some(prev_id)) = (previous_coords, prev_id_opt) {
                let prev_track = library_manager
                    .get_track(&prev_id)
                    .await
                    .expect("Previous track not found")
                    .expect("Previous track not found");

                let prev_buffer = if prev_track.release_id == current_release_id {
                    current_chunk_buffer.clone()
                } else {
                    Arc::new(ChunkBuffer::new(
                        library_manager.clone(),
                        cloud_storage.clone(),
                        cache.clone(),
                        encryption_service.clone(),
                        prev_track.release_id.clone(),
                    ))
                };

                if let Err(e) = prev_buffer
                    .prefetch_adjacent_tracks(Some(&prev_coords), None)
                    .await
                {
                    warn!("Failed to prefetch previous track chunks: {}", e);
                }
            }

            if let (Some(next_coords), Some(next_id)) = (next_coords, next_id_opt) {
                let next_track = library_manager
                    .get_track(&next_id)
                    .await
                    .expect("Next track not found")
                    .expect("Next track not found");

                let next_buffer = if next_track.release_id == current_release_id {
                    current_chunk_buffer.clone()
                } else {
                    Arc::new(ChunkBuffer::new(
                        library_manager.clone(),
                        cloud_storage.clone(),
                        cache.clone(),
                        encryption_service.clone(),
                        next_track.release_id.clone(),
                    ))
                };

                if let Err(e) = next_buffer
                    .prefetch_adjacent_tracks(None, Some(&next_coords))
                    .await
                {
                    warn!("Failed to prefetch next track chunks: {}", e);
                }
            }
        });

        // For now, we'll need to adapt play_track_with_decoder to not require audio_data
        // Let's create a simplified version that works with streaming
        self.play_track_with_decoder_streaming(
            track_id,
            track,
            decoder,
            track_duration,
            audio_format,
            coords,
            chunk_buffer,
        )
        .await;
    }

    async fn play_track_with_decoder_streaming(
        &mut self,
        track_id: &str,
        track: DbTrack,
        decoder: TrackDecoder,
        track_duration: std::time::Duration,
        audio_format: DbAudioFormat,
        coords: crate::db::DbTrackChunkCoords,
        chunk_buffer: Arc<ChunkBuffer>,
    ) {
        info!(
            "Starting streaming playback with decoder for track: {}",
            track_id
        );

        // Stop current stream if playing
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }

        // Create channels for position updates and completion
        let (position_tx, position_rx) = mpsc::channel();
        let (completion_tx, completion_rx) = mpsc::channel();

        // Bridge blocking channels to async channels
        let (position_tx_async, position_rx_async) = tokio_mpsc::unbounded_channel();
        let (completion_tx_async, completion_rx_async) = tokio_mpsc::unbounded_channel();

        // Bridge position updates
        let position_rx_clone = position_rx;
        tokio::spawn(async move {
            let position_rx = Arc::new(std::sync::Mutex::new(position_rx_clone));
            loop {
                let rx = position_rx.clone();
                match tokio::task::spawn_blocking(move || rx.lock().unwrap().recv()).await {
                    Ok(Ok(pos)) => {
                        let _ = position_tx_async.send(pos);
                    }
                    Ok(Err(_)) | Err(_) => break,
                }
            }
        });

        // Bridge completion signals
        let completion_rx_clone = completion_rx;
        tokio::spawn(async move {
            let completion_rx = Arc::new(std::sync::Mutex::new(completion_rx_clone));
            loop {
                let rx = completion_rx.clone();
                match tokio::task::spawn_blocking(move || rx.lock().unwrap().recv()).await {
                    Ok(Ok(())) => {
                        let _ = completion_tx_async.send(());
                    }
                    Ok(Err(_)) | Err(_) => break,
                }
            }
        });

        // Create audio stream (skip if mock)
        if self.audio_output.is_mock() {
            info!("Mock mode - skipping audio stream creation");
            self.stream = None;
        } else {
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
        }

        self.current_track = Some(track.clone());
        self.current_position = Some(std::time::Duration::ZERO);
        self.current_duration = Some(track_duration);
        self.is_paused = false;
        // Initialize shared position
        *self.current_position_shared.lock().unwrap() = Some(std::time::Duration::ZERO);

        // Update state
        let _ = self.progress_tx.send(PlaybackProgress::StateChanged {
            state: PlaybackState::Playing {
                track: track.clone(),
                position: std::time::Duration::ZERO,
                duration: Some(track_duration),
            },
        });

        // Store streaming metadata for seeking
        self.current_audio_format = Some(audio_format);
        self.current_coords = Some(coords);
        self.current_chunk_buffer = Some(chunk_buffer);

        // Listen for position updates and completion
        let track_id_for_task = track_id.to_string();
        let mut position_rx = position_rx_async;
        let mut completion_rx = completion_rx_async;
        let progress_tx = self.progress_tx.clone();
        let current_position_shared = self.current_position_shared.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(position) = position_rx.recv() => {
                        *current_position_shared.lock().unwrap() = Some(position);
                        let _ = progress_tx.send(PlaybackProgress::PositionUpdate {
                            position,
                            track_id: track_id_for_task.clone(),
                        });
                    }
                    Some(()) = completion_rx.recv() => {
                        info!("Track {} completed", track_id_for_task);
                        let _ = progress_tx.send(PlaybackProgress::TrackCompleted {
                            track_id: track_id_for_task.clone(),
                        });
                        break;
                    }
                    else => {
                        trace!("Position listener task exiting for track: {}", track_id_for_task);
                        break;
                    }
                }
            }
            trace!(
                "Completion listener task exiting for track: {}",
                track_id_for_task
            );
        });
    }

    async fn play_track_with_decoder(
        &mut self,
        track_id: &str,
        track: DbTrack,
        decoder: TrackDecoder,
        audio_data: Vec<u8>,
        track_duration: std::time::Duration,
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

        // Bridge blocking channels to async channels
        let (position_tx_async, mut position_rx_async) = tokio_mpsc::unbounded_channel();
        let (completion_tx_async, mut completion_rx_async) = tokio_mpsc::unbounded_channel();

        // Bridge position updates
        let position_rx_clone = position_rx;
        tokio::spawn(async move {
            let position_rx = Arc::new(std::sync::Mutex::new(position_rx_clone));
            loop {
                let rx = position_rx.clone();
                match tokio::task::spawn_blocking(move || rx.lock().unwrap().recv()).await {
                    Ok(Ok(pos)) => {
                        let _ = position_tx_async.send(pos);
                    }
                    Ok(Err(_)) | Err(_) => break,
                }
            }
        });

        // Bridge completion signals
        let completion_rx_clone = completion_rx;
        tokio::spawn(async move {
            let completion_rx = Arc::new(std::sync::Mutex::new(completion_rx_clone));
            loop {
                let rx = completion_rx.clone();
                match tokio::task::spawn_blocking(move || rx.lock().unwrap().recv()).await {
                    Ok(Ok(())) => {
                        let _ = completion_tx_async.send(());
                    }
                    Ok(Err(_)) | Err(_) => break,
                }
            }
        });

        // Create audio stream (skip if mock)
        if self.audio_output.is_mock() {
            info!("Mock mode - skipping audio stream creation");
            self.stream = None;
        } else {
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
        }

        self.current_track = Some(track.clone());
        self.current_position = Some(std::time::Duration::ZERO);
        self.current_duration = Some(track_duration);
        self.is_paused = false;
        // Initialize shared position
        *self.current_position_shared.lock().unwrap() = Some(std::time::Duration::ZERO);

        // Update state
        let _ = self.progress_tx.send(PlaybackProgress::StateChanged {
            state: PlaybackState::Playing {
                track: track.clone(),
                position: std::time::Duration::ZERO,
                duration: Some(track_duration),
            },
        });

        // Spawn task to handle position updates and completion
        let progress_tx = self.progress_tx.clone();
        let track_id = track_id.to_string();
        let track_duration_for_completion = track_duration;
        let current_position_for_listener = self.current_position_shared.clone();
        tokio::spawn(async move {
            info!(
                "Play: Spawning completion listener task for track: {}",
                track_id
            );

            info!("Play: Task started, waiting for position updates and completion");

            loop {
                tokio::select! {
                    Some(position) = position_rx_async.recv() => {
                        // Update shared position
                        *current_position_for_listener.lock().unwrap() = Some(position);
                        // Send PositionUpdate event
                        let _ = progress_tx.send(PlaybackProgress::PositionUpdate {
                            position,
                            track_id: track_id.clone(),
                        });
                    }
                    Some(()) = completion_rx_async.recv() => {
                        info!("Track completed: {}", track_id);
                        // Send final position update matching duration to ensure progress bar reaches 100%
                        let _ = progress_tx.send(PlaybackProgress::PositionUpdate {
                            position: track_duration_for_completion,
                            track_id: track_id.clone(),
                        });
                        let _ = progress_tx.send(PlaybackProgress::TrackCompleted {
                            track_id: track_id.clone(),
                        });
                        break;
                    }
                    else => {
                        info!("Play: Channels closed, exiting");
                        break;
                    }
                }
            }
            info!(
                "Play: Completion listener task exiting for track: {}",
                track_id
            );
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

        // Fetch track to get stored duration (correct for CUE/FLAC)
        // Duration is calculated once during import and stored in database - required for playback
        let track = match self.library_manager.get_track(track_id).await {
            Ok(Some(track)) => track,
            Ok(None) => panic!("Cannot preload track {} without track record", track_id),
            Err(e) => panic!(
                "Cannot preload track {} due to database error: {}",
                track_id, e
            ),
        };

        let duration = track
            .duration_ms
            .map(|ms| std::time::Duration::from_millis(ms as u64))
            .unwrap_or_else(|| panic!("Cannot preload track {} without duration", track_id));

        let decoder = TrackDecoder::new(audio_data.clone())
            .unwrap_or_else(|_| panic!("Cannot preload track {} without decoder", track_id));

        self.next_decoder = Some(decoder);
        self.next_audio_data = Some(audio_data);
        self.next_track_id = Some(track_id.to_string());
        self.next_duration = Some(duration);
        info!("Preloaded next track: {}", track_id);
    }

    async fn pause(&mut self) {
        self.audio_output
            .send_command(crate::playback::cpal_output::AudioCommand::Pause);

        // Send StateChanged event so UI can update button state
        // Position/duration are maintained via PositionUpdate events, but we include
        // current position here for cases where PositionUpdate hasn't arrived yet
        if let Some(track) = &self.current_track {
            let position = self
                .current_position_shared
                .lock()
                .unwrap()
                .unwrap_or(std::time::Duration::ZERO);
            let duration = self.current_duration;
            self.is_paused = true;
            let _ = self.progress_tx.send(PlaybackProgress::StateChanged {
                state: PlaybackState::Paused {
                    track: track.clone(),
                    position,
                    duration,
                },
            });
        }
    }

    async fn resume(&mut self) {
        self.audio_output
            .send_command(crate::playback::cpal_output::AudioCommand::Resume);

        // Send StateChanged event so UI can update button state
        // Position/duration are maintained via PositionUpdate events, but we include
        // current position here for cases where PositionUpdate hasn't arrived yet
        if let Some(track) = &self.current_track {
            let position = self
                .current_position_shared
                .lock()
                .unwrap()
                .unwrap_or(std::time::Duration::ZERO);
            let duration = self.current_duration;
            self.is_paused = false;
            let _ = self.progress_tx.send(PlaybackProgress::StateChanged {
                state: PlaybackState::Playing {
                    track: track.clone(),
                    position,
                    duration,
                },
            });
        }
    }

    async fn stop(&mut self) {
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }
        self.current_track = None;
        self.current_audio_data = None;
        self.current_audio_format = None;
        self.current_coords = None;
        self.current_chunk_buffer = None;
        self.current_position = None;
        self.current_duration = None;
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

    async fn seek_legacy_fallback(&mut self, position: std::time::Duration) {
        // Legacy seek using cached audio_data (for backwards compatibility)
        let (track_id, audio_data) = match (&self.current_track, &self.current_audio_data) {
            (Some(track), Some(audio_data)) => (track.id.clone(), audio_data.clone()),
            _ => {
                error!("Cannot seek: no cached audio data available");
                return;
            }
        };

        let current_position = self
            .current_position_shared
            .lock()
            .unwrap()
            .unwrap_or(std::time::Duration::ZERO);

        let position_diff = position.abs_diff(current_position);

        info!(
            "Legacy seek to position: {:?}, current position: {:?}, difference: {:?}",
            position, current_position, position_diff
        );

        // If seeking to roughly the same position (within 100ms), skip
        if position_diff < std::time::Duration::from_millis(100) {
            let _ = self.progress_tx.send(PlaybackProgress::SeekSkipped {
                requested_position: position,
                current_position,
            });
            return;
        }

        // Stop current stream
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }

        // Create new decoder with seeked position
        let mut decoder = match TrackDecoder::new(audio_data.clone()) {
            Ok(decoder) => decoder,
            Err(e) => {
                error!("Failed to create decoder for seek: {:?}", e);
                self.stop().await;
                return;
            }
        };

        let track_duration = self
            .current_duration
            .expect("Cannot seek: track has no duration");

        if position > track_duration {
            error!(
                "Cannot seek past end of track: requested {}, track duration {}",
                position.as_secs_f64(),
                track_duration.as_secs_f64()
            );
            let _ = self.progress_tx.send(PlaybackProgress::SeekError {
                requested_position: position,
                track_duration,
            });
            return;
        }

        // Seek decoder to desired position
        if let Err(e) = decoder.seek(position) {
            error!("Failed to seek decoder: {:?}", e);
            self.stop().await;
            return;
        }

        // Continue with rest of seek logic (creating stream, etc.)
        // This is the same as the original seek logic after decoder creation
        self.seek_with_decoder(track_id, decoder, position).await;
    }

    async fn seek_with_decoder(
        &mut self,
        track_id: String,
        decoder: TrackDecoder,
        position: std::time::Duration,
    ) {
        trace!("Seek: Decoder seeked successfully, creating new channels");

        // Create channels for position updates and completion
        let (position_tx, position_rx) = mpsc::channel();
        let (completion_tx, completion_rx) = mpsc::channel();

        // Bridge blocking channels to async channels
        let (position_tx_async, mut position_rx_async) = tokio_mpsc::unbounded_channel();
        let (completion_tx_async, mut completion_rx_async) = tokio_mpsc::unbounded_channel();

        trace!("Seek: Created new channels for position updates");

        // Bridge position updates
        let position_rx_clone = position_rx;
        tokio::spawn(async move {
            let position_rx = Arc::new(std::sync::Mutex::new(position_rx_clone));
            trace!("Seek: Bridge position task started");
            loop {
                let rx = position_rx.clone();
                match tokio::task::spawn_blocking(move || rx.lock().unwrap().recv()).await {
                    Ok(Ok(pos)) => {
                        trace!("Seek: Bridge received position update: {:?}", pos);
                        if let Err(e) = position_tx_async.send(pos) {
                            error!("Seek: Bridge failed to forward position update: {:?}", e);
                            break;
                        }
                    }
                    Ok(Err(_)) | Err(_) => {
                        trace!("Seek: Bridge position channel closed");
                        break;
                    }
                }
            }
            trace!("Seek: Bridge position task exiting");
        });

        // Bridge completion signals
        let completion_rx_clone = completion_rx;
        tokio::spawn(async move {
            let completion_rx = Arc::new(std::sync::Mutex::new(completion_rx_clone));
            loop {
                let rx = completion_rx.clone();
                match tokio::task::spawn_blocking(move || rx.lock().unwrap().recv()).await {
                    Ok(Ok(())) => {
                        trace!("Seek: Bridge received completion signal");
                        let _ = completion_tx_async.send(());
                    }
                    Ok(Err(_)) | Err(_) => {
                        trace!("Seek: Bridge completion channel closed");
                        break;
                    }
                }
            }
        });

        // Spawn task to handle position updates and completion BEFORE creating stream
        let progress_tx_for_task = self.progress_tx.clone();
        let track_id_for_task = track_id.clone();
        let _track_duration_for_completion = self
            .current_duration
            .expect("Cannot seek: track has no duration");
        let current_position_for_seek_listener = self.current_position_shared.clone();
        let was_paused = self.is_paused;
        tokio::spawn(async move {
            trace!(
                "Seek: Completion listener task started for track: {}",
                track_id_for_task
            );
            loop {
                tokio::select! {
                    Some(position) = position_rx_async.recv() => {
                        *current_position_for_seek_listener.lock().unwrap() = Some(position);
                        let _ = progress_tx_for_task.send(PlaybackProgress::PositionUpdate {
                            position,
                            track_id: track_id_for_task.clone(),
                        });
                    }
                    Some(()) = completion_rx_async.recv() => {
                        info!("Seek: Track {} completed", track_id_for_task);
                        let _ = progress_tx_for_task.send(PlaybackProgress::TrackCompleted {
                            track_id: track_id_for_task.clone(),
                        });
                        break;
                    }
                    else => {
                        trace!("Seek: Completion listener task exiting for track: {}", track_id_for_task);
                        break;
                    }
                }
            }
            trace!(
                "Seek: Completion listener task exiting for track: {}",
                track_id_for_task
            );
        });

        trace!("Seek: Creating new audio stream");

        // Create audio stream (skip if mock)
        if self.audio_output.is_mock() {
            trace!("Mock mode - skipping audio stream creation for seek");
            self.stream = None;
        } else {
            // Reuse existing audio output (don't create a new one)
            let stream = match self
                .audio_output
                .create_stream(decoder, position_tx, completion_tx)
            {
                Ok(stream) => {
                    trace!("Seek: Audio stream created successfully");
                    stream
                }
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

            trace!("Seek: Stream started successfully");

            self.stream = Some(stream);
        }

        // Update shared position
        *self.current_position_shared.lock().unwrap() = Some(position);
        trace!("Seek: Updated shared position to: {:?}", position);

        // If we were paused, keep it paused; otherwise play
        if was_paused {
            trace!("Seek: Was paused, keeping paused");
            self.audio_output
                .send_command(crate::playback::cpal_output::AudioCommand::Pause);
        } else {
            trace!("Seek: Was playing, sending Play command");
            self.audio_output
                .send_command(crate::playback::cpal_output::AudioCommand::Play);
        }

        // Send Seeked event with new position
        if let Some(track) = &self.current_track {
            trace!("Seek: Sending Seeked event with position: {:?}", position);
            let _ = self.progress_tx.send(PlaybackProgress::Seeked {
                position,
                track_id: track.id.clone(),
                was_paused,
            });
        }
    }

    async fn seek(&mut self, position: std::time::Duration) {
        // Can only seek if we have a current track
        let track_id = match &self.current_track {
            Some(track) => track.id.clone(),
            None => {
                error!("Cannot seek: no track playing");
                return;
            }
        };

        let current_position = self
            .current_position_shared
            .lock()
            .unwrap()
            .unwrap_or(std::time::Duration::ZERO);

        let position_diff = position.abs_diff(current_position);

        info!(
            "Seeking to position: {:?}, current position: {:?}, difference: {:?}",
            position, current_position, position_diff
        );

        // If seeking to roughly the same position (within 100ms), skip to avoid disrupting playback
        if position_diff < std::time::Duration::from_millis(100) {
            trace!(
                "Seek: Skipping seek to same position (difference: {:?} < 100ms)",
                position_diff
            );
            // Send SeekSkipped event to clear is_seeking flag in UI
            trace!("Seek: Sending SeekSkipped event to clear is_seeking flag");
            let _ = self.progress_tx.send(PlaybackProgress::SeekSkipped {
                requested_position: position,
                current_position,
            });
            return;
        }

        // Use stored track duration for validation (required - should always be Some if playing)
        let track_duration = self
            .current_duration
            .expect("Cannot seek: track has no duration");

        // Check if seeking past the end - return error instead of clamping
        if position > track_duration {
            error!(
                "Cannot seek past end of track: requested {}, track duration {}",
                position.as_secs_f64(),
                track_duration.as_secs_f64()
            );
            // Send error notification through progress channel
            let _ = self.progress_tx.send(PlaybackProgress::SeekError {
                requested_position: position,
                track_duration,
            });
            return;
        }

        // Stop current stream
        if let Some(stream) = self.stream.take() {
            trace!("Seek: Dropping old stream");
            drop(stream);
            trace!("Seek: Old stream dropped");
        } else {
            trace!("Seek: No stream to drop");
        }

        // Try streaming seek first, fallback to legacy if needed
        let decoder = if let (Some(audio_format), Some(coords), Some(chunk_buffer)) = (
            &self.current_audio_format,
            &self.current_coords,
            &self.current_chunk_buffer,
        ) {
            // Ensure initial chunks are loaded (for Symphonia probing) - this must complete
            // We need these chunks for the decoder to probe the format
            match chunk_buffer
                .ensure_chunks_loaded(
                    coords.start_chunk_index,
                    coords.start_chunk_index + 5,
                    0,
                    true,
                )
                .await
            {
                Ok(count) => {
                    if count == 0 {
                        warn!("No initial chunks loaded for seek, falling back");
                        return self.seek_legacy_fallback(position).await;
                    }
                }
                Err(e) => {
                    error!("Failed to load initial chunks for seek: {}", e);
                    return self.seek_legacy_fallback(position).await;
                }
            }

            // Calculate which chunk we'll need for the seek position and start loading it immediately
            // This ensures chunks are loading while we create the decoder
            let track_duration_ms = (coords.end_time_ms - coords.start_time_ms) as u64;
            let chunks_span = (coords.end_chunk_index - coords.start_chunk_index) as u64;

            // Load chunks around the seek position BEFORE creating the decoder
            // This prevents Symphonia from blocking on sequential chunk loads
            if track_duration_ms > 0 && chunks_span > 0 {
                let seek_time_ms = position.as_millis() as u64;
                let chunk_offset = (seek_time_ms * chunks_span) / track_duration_ms;
                let estimated_chunk =
                    (coords.start_chunk_index as u64 + chunk_offset.min(chunks_span)) as i32;

                let start_chunk = (estimated_chunk - 10).max(coords.start_chunk_index);
                let end_chunk = (estimated_chunk + 10).min(coords.end_chunk_index);

                tracing::debug!(
                    "Pre-loading chunks {}-{} for seek to {}s (estimated chunk: {})",
                    start_chunk,
                    end_chunk,
                    position.as_secs(),
                    estimated_chunk
                );

                // WAIT for chunks to load before creating decoder
                // This prevents Symphonia from triggering sequential chunk loads
                match chunk_buffer
                    .ensure_chunks_loaded(start_chunk, end_chunk, 0, true)
                    .await
                {
                    Ok(count) => {
                        tracing::debug!("Pre-loaded {} chunks around seek target", count);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to pre-load chunks for seek: {}", e);
                        // Continue anyway, chunks will load on-demand
                    }
                }

                // Also pre-load the last few chunks of the track
                // Symphonia seeks to End(0) to determine file size, so we need these loaded
                let last_chunk_start = (coords.end_chunk_index - 5).max(coords.start_chunk_index);
                if last_chunk_start > end_chunk {
                    tracing::debug!(
                        "Pre-loading last chunks {}-{} for Symphonia's End(0) seek",
                        last_chunk_start,
                        coords.end_chunk_index
                    );
                    match chunk_buffer
                        .ensure_chunks_loaded(last_chunk_start, coords.end_chunk_index, 0, true)
                        .await
                    {
                        Ok(count) => {
                            tracing::debug!("Pre-loaded {} chunks at track end", count);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to pre-load end chunks: {}", e);
                        }
                    }
                }
            }

            // Create streaming source and try to seek
            // If seek fails, recreate decoder with stream positioned at target
            let streaming_source = StreamingChunkSource::new(
                chunk_buffer.clone(),
                coords.clone(),
                self.chunk_size_bytes,
                audio_format.flac_headers.clone(),
                self.runtime_handle.clone(),
            );

            // Check if this is a backward seek before creating the decoder
            // We need to compare against current playback position, not the fresh decoder's position
            let is_backward_seek = position < current_position;

            // Try to create decoder and seek normally
            // With track-specific seektables injected into headers, Symphonia should handle seeks efficiently
            match TrackDecoder::from_streaming_source(streaming_source) {
                Ok(mut decoder) => {
                    if is_backward_seek {
                        info!(
                            "Backward seek detected (from {}s to {}s), recreating decoder",
                            current_position.as_secs(),
                            position.as_secs()
                        );

                        // Drop the old decoder and create a new one
                        drop(decoder);

                        // Create fresh streaming source
                        let new_streaming_source = StreamingChunkSource::new(
                            chunk_buffer.clone(),
                            coords.clone(),
                            self.chunk_size_bytes,
                            audio_format.flac_headers.clone(),
                            self.runtime_handle.clone(),
                        );

                        // Create new decoder
                        match TrackDecoder::from_streaming_source(new_streaming_source) {
                            Ok(mut new_decoder) => {
                                // Check if we need to seek from the beginning
                                // Fresh decoder starts at position 0, so only seek if target != 0
                                if position.as_secs() > 0 {
                                    match new_decoder.seek(position) {
                                        Ok(_) => {
                                            info!(
                                                "Backward seek to {}s succeeded",
                                                position.as_secs()
                                            );
                                            new_decoder
                                        }
                                        Err(e) => {
                                            error!(
                                                "Backward seek to {}s failed: {:?}",
                                                position.as_secs(),
                                                e
                                            );
                                            return self.seek_legacy_fallback(position).await;
                                        }
                                    }
                                } else {
                                    // Already at position 0, no need to seek
                                    info!("Backward seek to 0s - fresh decoder already at start");
                                    new_decoder
                                }
                            }
                            Err(e) => {
                                error!("Failed to create new decoder for backward seek: {:?}", e);
                                return self.seek_legacy_fallback(position).await;
                            }
                        }
                    } else {
                        // Forward seek - use existing decoder
                        match decoder.seek(position) {
                            Ok(_) => {
                                info!("Forward seek to {}s succeeded", position.as_secs());
                                decoder
                            }
                            Err(e) => {
                                error!("Forward seek to {}s failed: {:?}", position.as_secs(), e);
                                return self.seek_legacy_fallback(position).await;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to create streaming decoder for seek: {:?}", e);
                    return self.seek_legacy_fallback(position).await;
                }
            }
        } else {
            // Legacy path: use cached audio_data
            return self.seek_legacy_fallback(position).await;
        };

        // Use shared seek_with_decoder method
        // This will emit Seeked when playback actually starts
        self.seek_with_decoder(track_id, decoder, position).await;
    }

    /// Get chunk coordinates for a track (used for prefetching)
    ///
    /// Returns chunk coordinates for the track, panicking if track or coords are not found.
    async fn get_track_coords_for_prefetch(
        track_id: &str,
        library_manager: &LibraryManager,
    ) -> Result<crate::db::DbTrackChunkCoords, String> {
        let coords = library_manager
            .get_track_chunk_coords(track_id)
            .await
            .map_err(|e| format!("Database error: {}", e))?
            .expect("No chunk coordinates found");

        Ok(coords)
    }
}
