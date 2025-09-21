# bae Implementation Progress

> [!NOTE]  
> bae is in **pre-implementation**, meaning: we are defining the problem and
> sketching a solution. Our goal is to produce an minimal product. With that
> in mind, here are some things that are probably out of scope:
>
> - Scanning or monitoring files for changes
> - Checking disk space
> - Thinking about exclusive disk access
> - Permissions
> - Error handling/unhappy paths
> - Health monitoring, monitoring in general, statistics
> - Key rotation
> - Retry mechanisms
> - Caching, prefetching, optimization

## Setup & Initial Infrastructure

- [x] Set up local Rust development environment

  - [x] Create `rust-toolchain.toml` to specify Rust version and components
  - [x] Add `rustup-init.sh` for local Rust installation
  - [x] Update `.gitignore` to exclude local Rust files
  - [x] Document setup process in README
  - [x] Install dioxus-cli for Dioxus project creation

- [x] Set up Dioxus desktop project structure

  - [x] Create new Dioxus desktop project

### AI Setup

- [ ] Research and select appropriate AI provider SDK/client library
- [ ] Create configuration system for AI provider API credentials
- [ ] Implement secure credential storage and management
- [ ] Build abstraction layer for AI service communication
  - [ ] Implement adapter for initial AI provider (ChatGPT/Anthropic/Gemini)
  - [ ] Integrate AI provider SDK with appropriate error handling
- [ ] Build settings interface for AI provider configuration

## Core Features

### Storage Strategy

- [ ] Implement local library storage
  - [ ] Implement Storage struct
    - [ ] Create Storage::new() that creates ~/.bae directory structure
    - [ ] Add SQLite connection initialization
    - [ ] Add SQL for creating initial tables
  - [ ] Design initial schema
    - [ ] Create albums table (id, title, artist, year)
    - [ ] Create chunks table (id, s3_path, size, created_at)
    - [ ] Create local_chunks table (chunk_id, local_path)
- [ ] Create file chunking system
  - [ ] Design chunk format
    - [ ] Define chunk header fields (size, offset, hash)
    - [ ] Define chunk data format
  - [ ] Add schema for chunk tracking
    - [ ] Create file_chunks table (file_id, chunk_id, chunk_index)
    - [ ] Add indices for efficient chunk lookup
  - [ ] Implement Chunk struct
    - [ ] Create Chunk::new() that takes data and creates header
    - [ ] Add methods for serializing/deserializing chunks
    - [ ] Add method for reading from chunk offset
  - [ ] Implement chunking operations
    - [ ] Create function to split file into chunks
    - [ ] Create function to read from chunk at offset
    - [ ] Add chunk location tracking in SQLite
- [ ] Integrate encryption library
  - [ ] Research and select encryption library
    - [ ] List requirements (chunk encryption/decryption)
    - [ ] Compare available Rust encryption crates
    - [ ] Document selection rationale
  - [ ] Implement encryption operations
    - [ ] Create function to encrypt chunk
    - [ ] Create function to decrypt chunk
    - [ ] Add key storage in SQLite
  - [ ] Add encryption to Storage struct
    - [ ] Add encryption key management
    - [ ] Add methods for chunk encryption/decryption
- [ ] Implement S3 storage
  - [ ] Research and select S3 library
    - [ ] Compare aws-sdk-rust vs rust-s3
    - [ ] Document selection rationale
  - [ ] Create S3Storage struct
    - [ ] Implement new() with config (bucket, region, credentials)
    - [ ] Add upload_chunk(chunk_id: &str, data: &[u8]) -> Result<()>
    - [ ] Add download_chunk(chunk_id: &str) -> Result<Vec<u8>>
    - [ ] Add delete_chunk(chunk_id: &str) -> Result<()>
  - [ ] Implement chunk tracking
    - [ ] Add chunks_remote table (chunk_id, s3_path, uploaded_at)
    - [ ] Track chunk upload status
    - [ ] Track chunk deletion status
  - [ ] Add local cache management
    - [ ] Implement max local storage setting
    - [ ] Track chunk sizes and total usage
    - [ ] Add LRU-based chunk eviction
  - [ ] Add piece to chunk mapping
    - [ ] Create piece_chunks table (torrent_id, piece_index, chunk_id, chunk_offset, length)
    - [ ] Add indices for efficient piece lookup
    - [ ] Add methods to map between pieces and chunks
  - [ ] Integrate with S3 storage
- [ ] Build storage controller
  - [ ] Create StorageController struct
    - [ ] Implement new() with local and S3 storage configs
    - [ ] Add write_chunk(data: &[u8]) -> Result<ChunkId>
    - [ ] Add read_chunk(chunk_id: &str) -> Result<Vec<u8>>
    - [ ] Add ensure_local(chunk_id: &str) -> Result<PathBuf>
  - [ ] Implement storage operations
    - [ ] Write chunk locally and upload to S3
    - [ ] Check local storage before downloading
    - [ ] Handle chunk eviction when storage limit reached
    - [ ] Add buffered reading for streaming
- [ ] Build library manager
  - [ ] Create LibraryManager struct
    - [ ] Implement new() with storage controller and SQLite connection
    - [ ] Add import_album(path: &Path, metadata: AlbumMetadata) -> Result<AlbumId>
    - [ ] Add import_track(path: &Path, metadata: TrackMetadata) -> Result<TrackId>
    - [ ] Add get_track_reader(track_id: &str) -> Result<TrackReader>
  - [ ] Implement album operations
    - [ ] Split files into chunks using storage controller
    - [ ] Store album and track metadata
    - [ ] Map chunks to tracks in database
    - [ ] Handle reading track data for playback
- [ ] Create storage settings UI
  - [ ] Create Dioxus components
    - [ ] Create StorageSettings.rs
      - [ ] Add S3 settings form using Dioxus form handling
      - [ ] Add local storage settings form
    - [ ] Create StorageUsage.rs for displaying current chunk usage
  - [ ] Add backend functions in main.rs
    - [ ] Add get_storage_settings() -> Result<StorageSettings>
    - [ ] Add save_storage_settings(settings: StorageSettings) -> Result<()>
  - [ ] Implement settings persistence
    - [ ] Add settings table to SQLite
    - [ ] Add secure credential storage using keyring crate or OS keychain

### Album Management

- [x] Implement Discogs API client
  - [x] Build custom Discogs HTTP client using reqwest
    - [ ] Compare available Rust Discogs clients
    - [ ] Document selection rationale
  - [x] Create DiscogsClient struct
    - [x] Implement new() with API key
    - [x] Add search_releases(query: &str) -> Result<Vec<Release>>
    - [x] Add get_release(id: &str) -> Result<Release>
  - [x] Add API key management
    - [x] Store API key in secure storage
    - [x] Add key validation
- [x] Create album metadata model
  - [x] Write model tests
    - [x] Test album struct serialization
    - [x] Test DiscogsTrack duration parsing
  - [x] Create Album struct
    - [x] Add required fields (id, title, artist, year)
    - [x] Add optional fields (genre, cover art)
    - [x] Add track list
  - [x] Create Track struct
    - [x] Add required fields (id, title, duration)
    - [x] Add optional fields (track number, artist)
  - [x] Create Artist struct
    - [x] Add required fields (id, name)
    - [x] Add optional fields (bio, image)
- [x] Build album search UI
  - [x] Create Dioxus components in main.rs
    - [x] Add search input 
    - [x] Add basic release result cards
    - [x] Create AlbumDetails.rs
      - [x] Display release information
      - [x] Show track list
      - [x] Add cover art display
  - [x] Add backend functions
    - [x] Add search_albums(query: &str) -> Result<Vec<Album>>
    - [x] Add get_album_details(id: &str) -> Result<Album>
- [x] Build album import UI
  - [x] Create Dioxus components
    - [x] Create AlbumImport.rs
      - [x] Show selected release details
      - [ ] Add data source selection
        - [ ] Local folder picker
        - [ ] Remote source input (torrent/custom)
      - [ ] Add import progress
        - [ ] Show download/verification progress
        - [ ] Display any errors
      - [ ] Add success/failure states
  - [ ] Add backend functions
    - [ ] Add select_folder() -> Result<PathBuf>
    - [ ] Add import_album(album_id: &str, source: DataSource) -> Result<AlbumId>
    - [ ] Add get_import_progress(album_id: &str) -> Result<ImportProgress>
- [ ] Implement album browser
  - [ ] Create AlbumBrowser struct
    - [ ] Implement new() with SQLite connection
    - [ ] Add get_artists() -> Result<Vec<Artist>>
    - [ ] Add get_artist_albums(artist_id: &str) -> Result<Vec<Album>>
    - [ ] Add search_albums(query: &str) -> Result<Vec<Album>>
  - [ ] Build browser UI
    - [ ] Create ArtistView.rs
      - [ ] Display artist info
      - [ ] Show album grid
      - [ ] Add sorting options
    - [ ] Create AlbumGrid.rs
      - [ ] Display album covers
      - [ ] Add hover effects
      - [ ] Handle selection

### Track Mapping

- [ ] Implement track mapping core

  - [ ] Create TrackMapper trait
    - [ ] Define map_tracks(files: Vec<PathBuf>, metadata: AlbumMetadata) -> Result<TrackMapping>
    - [ ] Define verify_mapping(mapping: &TrackMapping) -> Result<()>

- [ ] Implement one-file-per-track mapper

  - [ ] Create SimpleTrackMapper struct
    - [ ] Add duration detection using ffmpeg
    - [ ] Integrate with AI for track matching
    - [ ] Add format validation
  - [ ] Add mapping persistence
    - [ ] Create track_files table (track_id, file_id, offset, duration)
    - [ ] Store mapping results in database

- [ ] Implement CUE sheet parser

  - [ ] Research and select CUE parser library
    - [ ] Compare available Rust CUE sheet parsers
    - [ ] Document selection rationale
  - [ ] Create CueSheet struct
    - [ ] Add methods to parse CUE file
    - [ ] Add track index calculation
    - [ ] Add duration calculation

- [ ] Implement FLAC + CUE mapper

  - [ ] Create FlacCueMapper struct
    - [ ] Add FLAC format validation
    - [ ] Parse CUE sheet track numbers and times
    - [ ] Map CUE tracks to Discogs tracks by position
  - [ ] Add mapping persistence
    - [ ] Update track_files schema for CUE offsets
    - [ ] Store CUE sheet reference

- [ ] Design AI track mapping prompts
  - [ ] Create prompts for one-file-per-track matching
    - [ ] Use AI to identify which files correspond to which Discogs tracks
    - [ ] Use duration as additional validation
  - [ ] Create prompts for CUE sheet validation
    - [ ] Use AI to map CUE sheet entries to Discogs tracks
    - [ ] Validate track timings match CUE sheet
  - [ ] Add fallback for manual mapping when AI fails

### Remote Sources

- [ ] Integrate libtorrent-rs

  - [ ] Research and select libtorrent-rs version
    - [ ] Compare available versions
    - [ ] Document selection rationale
  - [ ] Create TorrentClient struct
    - [ ] Add new() with config
    - [ ] Add add_torrent() for magnet/file
    - [ ] Add get_files() -> Vec<TorrentFile>
    - [ ] Add start/stop/pause controls

- [ ] Implement custom storage backend

  - [ ] Create TorrentStorage struct
    - [ ] Implement libtorrent storage interface
    - [ ] Add piece to chunk mapping
    - [ ] Integrate with S3 storage
    - [ ] Handle local caching

- [ ] Design AI prompts for torrent matching

  - [ ] Create prompts for file identification
    - [ ] Match torrent files to Discogs tracks
    - [ ] Handle common torrent structures
    - [ ] Consider file sizes and formats
  - [ ] Add verification prompts
    - [ ] Verify downloaded files match expected
    - [ ] Handle missing/corrupt files

- [ ] Implement download manager

  - [ ] Create DownloadManager struct
    - [ ] Add queue management
    - [ ] Add progress tracking
    - [ ] Add state persistence
    - [ ] Handle download failures

- [ ] Add backend functions
  - [ ] Add parse_torrent(magnet_or_file: String) -> Result<TorrentInfo>
  - [ ] Add start_download(torrent_id: String, file_mapping: FileMapping) -> Result<()>
  - [ ] Add get_download_progress(torrent_id: String) -> Result<Progress>
  - [ ] Add get_active_downloads() -> Result<Vec<Download>>

## Browsing & Streaming

### Transcoding

- [ ] Research and select ffmpeg Rust bindings

  - [ ] Compare available options (ffmpeg-next, ffmpeg-sys-next)
  - [ ] Document selection rationale

- [ ] Implement TranscodingService

  - [ ] Create TranscodingService struct
    - [ ] Add new() with ffmpeg configuration
    - [ ] Add get_stream_info(path: &Path) -> Result<StreamInfo>
    - [ ] Add create_transcoder(input: &Path, format: Format) -> Result<Transcoder>
    - [ ] Add frame processing components
      - [ ] Create AVCodecContext for decoding
      - [ ] Create SwrContext for resampling if needed
      - [ ] Create AVCodecContext for encoding
  - [ ] Implement transcoding operations
    - [ ] Add format detection and validation
    - [ ] Add quality/bitrate configuration
    - [ ] Add progress tracking
    - [ ] Handle seeking within stream
    - [ ] Implement frame pipeline
      - [ ] Decode input into AVFrames
      - [ ] Resample frames if needed
      - [ ] Encode frames to output format

- [ ] Build streaming buffer system

  - [ ] Create StreamBuffer struct
    - [ ] Add new() with buffer size configuration
    - [ ] Add write_chunk(chunk: &[u8]) -> Result<()>
    - [ ] Add read_frame() -> Result<AudioFrame>
      - [ ] Define AudioFrame struct (samples per channel, timestamp)
      - [ ] Handle frame size configuration based on format
      - [ ] Ensure frame boundaries align with format requirements
    - [ ] Add seek(position: Duration) -> Result<()>
  - [ ] Implement buffering operations
    - [ ] Add chunk queueing and processing
    - [ ] Add frame assembly from chunks
    - [ ] Handle buffer underrun/overrun
    - [ ] Implement efficient seeking

- [ ] Implement CUE-based streaming

  - [ ] Create CueStreamHandler struct
    - [ ] Add new() with CUE sheet and FLAC file
    - [ ] Add get_track_stream(track_id: &str) -> Result<Stream>
    - [ ] Add seek_track(position: Duration) -> Result<()>
  - [ ] Implement CUE operations
    - [ ] Calculate track boundaries and offsets
    - [ ] Handle seeking within tracks
    - [ ] Manage track transitions
    - [ ] Pass through track metadata

- [ ] Add backend functions

  - [ ] Add get_stream_formats() -> Result<Vec<Format>>
  - [ ] Add start_stream(track_id: &str, format: Format) -> Result<StreamHandle>
  - [ ] Add seek_stream(handle: StreamHandle, position: Duration) -> Result<()>
  - [ ] Add get_stream_progress(handle: StreamHandle) -> Result<Progress>

- [ ] Implement Subsonic streaming integration
  - [ ] Create StreamingEndpoint struct
    - [ ] Add new() with transcoding service
    - [ ] Add handle_stream(track_id: &str, params: StreamParams) -> Result<Response>
    - [ ] Add handle_seek(stream_id: &str, time_offset: Duration) -> Result<()>
  - [ ] Implement streaming operations
    - [ ] Create background task for stream processing
    - [ ] Connect TranscodingService output to HTTP response
    - [ ] Handle client disconnection
    - [ ] Manage concurrent streams

### Subsonic API Implementation

- [ ] Implement core system endpoints

  - [ ] Create system endpoints
    - [ ] Add ping() -> Result<Response>
    - [ ] Add getLicense() -> Result<Response>
    - [ ] Add error response handling

- [ ] Implement browsing endpoints

  - [ ] Create browsing endpoints
    - [ ] Add getMusicFolders() -> Result<Vec<Folder>>
    - [ ] Add getIndexes() -> Result<Index>
    - [ ] Add getArtist(id: &str) -> Result<Artist>
    - [ ] Add getAlbum(id: &str) -> Result<Album>
    - [ ] Add getSong(id: &str) -> Result<Song>
  - [ ] Implement response formatting
    - [ ] Add XML response serialization
    - [ ] Add JSON response serialization
    - [ ] Add response envelope handling

- [ ] Implement media retrieval endpoints

  - [ ] Create media endpoints
    - [ ] Add stream(id: &str, format: Option<Format>) -> Result<Stream>
    - [ ] Add download(id: &str) -> Result<Response>
    - [ ] Add getCoverArt(id: &str, size: Option<u32>) -> Result<Image>
  - [ ] Integrate with storage system
    - [ ] Connect stream endpoint to TranscodingService
    - [ ] Handle range requests for seeking
    - [ ] Implement Discogs cover art integration
      - [ ] Add cover art URL caching in albums table
      - [ ] Add proxy endpoint to fetch from Discogs
      - [ ] Handle Discogs rate limiting
      - [ ] Add error fallback (placeholder image)

- [ ] Implement playlist management

  - [ ] Create playlist endpoints
    - [ ] Add getPlaylists() -> Result<Vec<Playlist>>
    - [ ] Add getPlaylist(id: &str) -> Result<Playlist>
    - [ ] Add createPlaylist(name: &str, songs: Vec<String>) -> Result<PlaylistId>
    - [ ] Add updatePlaylist(id: &str, songs: Vec<String>) -> Result<()>
    - [ ] Add deletePlaylist(id: &str) -> Result<()>
  - [ ] Add playlist persistence
    - [ ] Create playlists table in SQLite
    - [ ] Add playlist_songs table for entries
    - [ ] Handle playlist modifications

- [ ] Create authentication system

  - [ ] Implement auth system
    - [ ] Add user management
    - [ ] Add token-based authentication
    - [ ] Add password hashing with argon2
    - [ ] Add session management
  - [ ] Create auth middleware
    - [ ] Add request authentication
    - [ ] Add role-based access control
    - [ ] Handle auth errors
  - [ ] Add auth persistence
    - [ ] Create users table
    - [ ] Create sessions table
    - [ ] Add secure credential storage

- [ ] Add Subsonic response middleware
  - [ ] Create middleware
    - [ ] Add response envelope wrapper
    - [ ] Add error translation
    - [ ] Add format selection (XML/JSON)
  - [ ] Implement response formatting
    - [ ] Add XML serialization
    - [ ] Add JSON serialization
    - [ ] Add JSONP support

## Desktop Application UI

- [x] Build Dioxus desktop interface
  - [x] Replace template with music library layout
    - [x] Replace Hero component with music-focused home page
    - [ ] Keep Blog component for now (minimal product)
    - [x] Design main application layout (header, navigation, content area)
    - [x] Set up routing for core views (Library, Search, Import, Settings)
  - [ ] Implement library browsing interface
    - [ ] Create album grid/list components
    - [ ] Build artist browser with album grouping
    - [ ] Add search and filtering controls
    - [ ] Connect to SQLite database for real data
  - [x] Build basic album import workflow
    - [x] Create Discogs search interface (same as album search)
    - [x] Add placeholder import pages
    - [x] Build album selection and preview components
    - [ ] Add data source selection (local folder, torrent)
    - [ ] Implement import progress tracking UI
  - [ ] Create settings management interface
    - [ ] Build storage configuration panel (S3 settings, local cache)
    - [ ] Add AI provider configuration interface
    - [ ] Create preferences and options screens
  - [x] Add state management and data flow
    - [x] Set up Dioxus signals for application state
    - [x] Connect UI components to backend storage functions
    - [x] Implement error handling and user feedback
    - [x] Add loading states for async operations

## Deployment & Distribution

- [ ] Create installer for multiple platforms
- [ ] Implement auto-update functionality
- [ ] Create user documentation
- [ ] Build backup and restore functionality
