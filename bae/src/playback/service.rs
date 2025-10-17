use crate::cache::CacheManager;
use crate::cloud_storage::CloudStorageManager;
use crate::database::DbTrack;
use crate::encryption::EncryptionService;
use crate::library::LibraryManager;
use rodio::{OutputStream, OutputStreamBuilder, Sink};
use std::collections::VecDeque;
use std::io::{BufReader, Cursor};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

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
    },
    Paused {
        track: DbTrack,
        position: std::time::Duration,
    },
    Loading {
        track_id: String,
    },
}

/// Handle to the playback service for sending commands
#[derive(Clone)]
pub struct PlaybackHandle {
    command_tx: mpsc::UnboundedSender<PlaybackCommand>,
    state: Arc<Mutex<PlaybackState>>,
    is_playing: Arc<AtomicBool>,
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
        self.state.lock().await.clone()
    }

    pub fn is_playing(&self) -> bool {
        self.is_playing.load(Ordering::SeqCst)
    }
}

/// Playback service that manages audio playback
pub struct PlaybackService {
    library_manager: LibraryManager,
    cloud_storage: CloudStorageManager,
    cache: CacheManager,
    encryption_service: EncryptionService,
    chunk_size_bytes: usize,
    command_rx: mpsc::UnboundedReceiver<PlaybackCommand>,
    queue: VecDeque<String>, // track IDs
    current_track: Option<DbTrack>,
    state: Arc<Mutex<PlaybackState>>,
    is_playing: Arc<AtomicBool>,

    // Rodio audio components
    _stream: OutputStream,
    sink: Option<Sink>,
}

impl PlaybackService {
    pub fn start(
        library_manager: LibraryManager,
        cloud_storage: CloudStorageManager,
        cache: CacheManager,
        encryption_service: EncryptionService,
        chunk_size_bytes: usize,
        _runtime_handle: tokio::runtime::Handle,
    ) -> PlaybackHandle {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let state = Arc::new(Mutex::new(PlaybackState::Stopped));
        let is_playing = Arc::new(AtomicBool::new(false));

        let handle = PlaybackHandle {
            command_tx: command_tx.clone(),
            state: state.clone(),
            is_playing: is_playing.clone(),
        };

        // Spawn the service task on a dedicated thread (OutputStream isn't Send-safe)
        std::thread::spawn(move || {
            // Create a new tokio runtime for this thread
            let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");

            rt.block_on(async move {
                // Initialize rodio output stream in the task
                let stream = OutputStreamBuilder::open_default_stream()
                    .expect("Failed to initialize audio output");

                let mut service = PlaybackService {
                    library_manager,
                    cloud_storage,
                    cache,
                    encryption_service,
                    chunk_size_bytes,
                    command_rx,
                    queue: VecDeque::new(),
                    current_track: None,
                    state,
                    is_playing,
                    _stream: stream,
                    sink: None,
                };

                service.run().await;
            });
        });

        handle
    }

    async fn run(&mut self) {
        println!("PlaybackService started");

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
                    if let Some(sink) = &self.sink {
                        sink.pause();
                        self.is_playing.store(false, Ordering::SeqCst);

                        if let Some(track) = &self.current_track {
                            let position = self.get_current_position();
                            *self.state.lock().await = PlaybackState::Paused {
                                track: track.clone(),
                                position,
                            };
                        }
                    }
                }
                PlaybackCommand::Resume => {
                    if let Some(sink) = &self.sink {
                        sink.play();
                        self.is_playing.store(true, Ordering::SeqCst);

                        if let Some(track) = &self.current_track {
                            let position = self.get_current_position();
                            *self.state.lock().await = PlaybackState::Playing {
                                track: track.clone(),
                                position,
                            };
                        }
                    }
                }
                PlaybackCommand::Stop => {
                    self.stop();
                }
                PlaybackCommand::Next => {
                    if let Some(next_track) = self.queue.pop_front() {
                        self.play_track(&next_track).await;
                    } else {
                        self.stop();
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
                    if let Some(sink) = &self.sink {
                        let _ = sink.try_seek(position);
                    }
                }
                PlaybackCommand::SetVolume(volume) => {
                    if let Some(sink) = &self.sink {
                        sink.set_volume(volume.clamp(0.0, 1.0));
                    }
                }
            }
        }

        println!("PlaybackService stopped");
    }

    async fn play_track(&mut self, track_id: &str) {
        println!("Playing track: {}", track_id);

        // Update state to loading
        *self.state.lock().await = PlaybackState::Loading {
            track_id: track_id.to_string(),
        };

        // Fetch track metadata
        let track = match self.library_manager.get_track(track_id).await {
            Ok(Some(track)) => track,
            Ok(None) => {
                eprintln!("Track not found: {}", track_id);
                self.stop();
                return;
            }
            Err(e) => {
                eprintln!("Failed to fetch track: {}", e);
                self.stop();
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
                eprintln!("Failed to reassemble track: {}", e);
                self.stop();
                return;
            }
        };

        println!("Track loaded: {} bytes", audio_data.len());

        // Validate FLAC header
        if audio_data.len() < 4 {
            eprintln!("Audio data too small: {} bytes", audio_data.len());
            self.stop();
            return;
        }

        if &audio_data[0..4] != b"fLaC" {
            eprintln!(
                "Invalid FLAC header: expected 'fLaC', got {:?}",
                &audio_data[0..4.min(audio_data.len())]
            );
            eprintln!(
                "First 16 bytes: {:?}",
                &audio_data[0..16.min(audio_data.len())]
            );
            self.stop();
            return;
        }

        println!("Valid FLAC header detected");

        // Create audio source from buffer with buffered reading
        let cursor = Cursor::new(audio_data);
        let buf_reader = BufReader::new(cursor);
        let source = match rodio::Decoder::new(buf_reader) {
            Ok(decoder) => decoder,
            Err(e) => {
                eprintln!("Failed to decode audio: {}", e);
                self.stop();
                return;
            }
        };

        // Create a new sink connected to the existing output stream
        let sink = rodio::Sink::connect_new(self._stream.mixer());
        sink.append(source);
        sink.play();

        self.sink = Some(sink);
        self.current_track = Some(track.clone());
        self.is_playing.store(true, Ordering::SeqCst);

        *self.state.lock().await = PlaybackState::Playing {
            track,
            position: std::time::Duration::ZERO,
        };

        // TODO: Auto-advance to next track when current track finishes
        // Need to find a way to monitor sink completion without cloning
    }

    fn stop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        self.current_track = None;
        self.is_playing.store(false, Ordering::SeqCst);
        let state = self.state.clone();
        tokio::spawn(async move {
            *state.lock().await = PlaybackState::Stopped;
        });
    }

    fn get_current_position(&self) -> std::time::Duration {
        // Rodio doesn't provide easy position tracking, so we'll return zero for now
        // This can be improved with manual position tracking
        std::time::Duration::ZERO
    }
}
