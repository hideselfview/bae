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
  - [ ] Write storage initialization tests
    - [ ] Write test for creating temp test directory
    - [ ] Write test that Storage::new() creates required subdirectories
    - [ ] Write test that Storage::new() creates SQLite database file
    - [ ] Write test that verifies database has required tables
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
  - [ ] Write chunking tests
    - [ ] Write test for splitting file into fixed-size chunks
    - [ ] Write test for reading data from specific chunk offset
    - [ ] Write test for chunk header format
    - [ ] Write test for tracking chunks in SQLite
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
  - [ ] Write encryption tests
    - [ ] Write test for encrypting chunk
    - [ ] Write test for decrypting chunk
    - [ ] Write test for key storage
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
  - [ ] Write S3 storage tests
    - [ ] Write test for S3 client configuration
    - [ ] Write test for uploading chunk
    - [ ] Write test for downloading chunk
    - [ ] Write test for deleting chunk
    - [ ] Write test using mock S3 service
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
  - [ ] Write storage controller tests
    - [ ] Write test for writing chunk (local + upload to S3)
    - [ ] Write test for reading chunk (check local first, download if needed)
    - [ ] Write test for chunk eviction (when local storage limit reached)
    - [ ] Write test for parallel chunk operations
    - [ ] Write test using mock S3 backend
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
  - [ ] Write library manager tests
    - [ ] Write test for importing album from directory
    - [ ] Write test for importing single track
    - [ ] Write test for reading track data
    - [ ] Write test for album metadata operations
    - [ ] Write test using mock storage controller
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
    - [ ] Add test_s3_connection(settings: S3Settings) -> Result<()>
  - [ ] Implement settings persistence
    - [ ] Add settings table to SQLite
    - [ ] Add secure credential storage using keyring crate or OS keychain
  - [ ] Write tests
    - [ ] Write Dioxus component tests
      - [ ] Test form validation
        - [ ] Test required fields (access key, secret key, bucket name)
        - [ ] Test region selector has valid options
        - [ ] Test local storage limit is a positive number
        - [ ] Test local storage limit has reasonable max value
      - [ ] Test form submission
        - [ ] Test successful form submission calls correct backend function
        - [ ] Test form values are correctly passed to command
        - [ ] Test form is disabled during submission
        - [ ] Test loading state during S3 connection test
      - [ ] Test error display
        - [ ] Test invalid credentials error message
        - [ ] Test connection timeout error message
        - [ ] Test bucket not found error message
        - [ ] Test form remains editable after error
    - [ ] Write backend function tests
      - [ ] Test settings serialization
        - [ ] Test StorageSettings struct serializes to/from JSON
        - [ ] Test S3Settings validation (required fields, formats)
        - [ ] Test local storage limit validation
      - [ ] Test SQLite operations
        - [ ] Test saving settings to database
        - [ ] Test loading settings from database
        - [ ] Test updating existing settings
        - [ ] Test default settings when none exist
      - [ ] Test credential storage
        - [ ] Test storing S3 credentials in OS secure storage
        - [ ] Test retrieving credentials
        - [ ] Test updating credentials
        - [ ] Test credential encryption/decryption
    - [ ] Write integration tests
      - [ ] Set up test environment with mock S3
        - [ ] Add MinIO or LocalStack as mock S3 service
        - [ ] Create test bucket and credentials
        - [ ] Add helper functions to reset mock S3 state
      - [ ] Test full settings save flow
        - [ ] Test saving valid S3 settings updates database
        - [ ] Test saving valid local storage limit updates database
        - [ ] Test credentials are stored in OS secure storage
        - [ ] Test UI reflects saved settings on reload
      - [ ] Test connection validation
        - [ ] Test successful connection to mock S3
        - [ ] Test invalid credentials error handling
        - [ ] Test invalid bucket error handling
        - [ ] Test network timeout handling
      - [ ] Verify settings persistence
        - [ ] Test settings survive app restart
        - [ ] Test secure credentials survive app restart
        - [ ] Test default settings on first run

### Album Management

- [x] Implement Discogs API client
  - [x] Build custom Discogs HTTP client using reqwest
    - [ ] Compare available Rust Discogs clients
    - [ ] Document selection rationale
  - [ ] Write Discogs client tests
    - [ ] Write test for searching releases
    - [ ] Write test for getting release details
    - [ ] Write test for rate limiting handling
    - [ ] Write test using mock Discogs API
  - [x] Create DiscogsClient struct
    - [x] Implement new() with API key
    - [x] Add search_releases(query: &str) -> Result<Vec<Release>>
    - [x] Add get_release(id: &str) -> Result<Release>
  - [ ] Add API key management
    - [ ] Store API key in secure storage
    - [ ] Add key validation
- [x] Create album metadata model
  - [x] Write model tests
    - [x] Test album struct serialization
    - [x] Test DiscogsTrack duration parsing
    - [ ] Test track struct serialization
    - [ ] Test artist struct serialization
    - [ ] Test validation rules
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
    - [x] Add search input (no debouncing)
      - [ ] Add debouncing to search input
      - [ ] Add results list with pagination
      - [x] Add basic release result cards
    - [ ] Create AlbumDetails.rs
      - [ ] Display release information
      - [ ] Show track list
      - [ ] Add cover art display
  - [x] Add backend functions
    - [x] Add search_albums(query: &str) -> Result<Vec<Album>>
    - [x] Add get_album_details(id: &str) -> Result<Album>
  - [ ] Write tests
    - [ ] Write Dioxus component tests
      - [ ] Test search input behavior
      - [ ] Test results display
      - [ ] Test pagination
      - [ ] Test error states
    - [ ] Write backend function tests
      - [ ] Test function serialization
      - [ ] Test error handling
- [ ] Build album import UI
  - [ ] Create Dioxus components
    - [ ] Create AlbumImport.rs
      - [ ] Show selected release details
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
  - [ ] Write tests
    - [ ] Write Dioxus component tests
      - [ ] Test source selection
      - [ ] Test progress display
      - [ ] Test error handling
    - [ ] Write backend function tests
      - [ ] Test folder selection
      - [ ] Test import process
- [ ] Implement album browser
  - [ ] Write browser logic tests
    - [ ] Test artist grouping
      - [ ] Test artists are correctly grouped and counted
      - [ ] Test artist metadata (name, album count)
    - [ ] Test album sorting
      - [ ] Test sorting by year (ascending/descending)
      - [ ] Test sorting by title
      - [ ] Test sorting by artist
    - [ ] Test search functionality
      - [ ] Test searching by album title
      - [ ] Test searching by artist name
      - [ ] Test partial matches
      - [ ] Test case insensitivity
    - [ ] Test pagination
      - [ ] Test first page
      - [ ] Test middle pages
      - [ ] Test last page
      - [ ] Test empty results
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
  - [ ] Write UI component tests
    - [ ] Test ArtistView component
      - [ ] Test artist info display
      - [ ] Test album grid layout
      - [ ] Test sorting controls
      - [ ] Test responsive layout
    - [ ] Test AlbumGrid component
      - [ ] Test grid layout
      - [ ] Test hover effects
      - [ ] Test album selection
      - [ ] Test empty state
    - [ ] Test user interactions
      - [ ] Test clicking album opens details
      - [ ] Test sorting changes update display
      - [ ] Test search input updates results
      - [ ] Test pagination controls

### Track Mapping

- [ ] Implement track mapping core

  - [ ] Create TrackMapper trait
    - [ ] Define map_tracks(files: Vec<PathBuf>, metadata: AlbumMetadata) -> Result<TrackMapping>
    - [ ] Define verify_mapping(mapping: &TrackMapping) -> Result<()>
  - [ ] Write track mapper tests
    - [ ] Test mapping validation
    - [ ] Test error cases (missing files)
    - [ ] Test track duration calculation

- [ ] Implement one-file-per-track mapper

  - [ ] Write tests
    - [ ] Test AI matching of files to Discogs tracks
    - [ ] Test duration validation
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
  - [ ] Write CUE parser tests
    - [ ] Test parsing basic CUE sheets
    - [ ] Test error cases (malformed CUE)
    - [ ] Test track index parsing
  - [ ] Create CueSheet struct
    - [ ] Add methods to parse CUE file
    - [ ] Add track index calculation
    - [ ] Add duration calculation

- [ ] Implement FLAC + CUE mapper

  - [ ] Write tests
    - [ ] Test CUE track number matching with Discogs tracks
    - [ ] Test track boundary calculation
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
  - [ ] Write torrent client tests
    - [ ] Test magnet link parsing
    - [ ] Test torrent file parsing
    - [ ] Test file listing
    - [ ] Test download control
  - [ ] Create TorrentClient struct
    - [ ] Add new() with config
    - [ ] Add add_torrent() for magnet/file
    - [ ] Add get_files() -> Vec<TorrentFile>
    - [ ] Add start/stop/pause controls

- [ ] Implement custom storage backend

  - [ ] Write storage tests
    - [ ] Test piece storage
    - [ ] Test piece retrieval
    - [ ] Test integration with chunk system
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

  - [ ] Write download tests
    - [ ] Test download initiation
    - [ ] Test progress tracking
    - [ ] Test pause/resume
    - [ ] Test error handling
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
  - [ ] Test basic transcoding capabilities
    - [ ] Test format conversion (FLAC -> MP3/AAC)
    - [ ] Test bitrate control
    - [ ] Test seeking within files
    - [ ] Test frame-level operations
      - [ ] Test AVFrame creation and management
      - [ ] Test direct frame input/output
      - [ ] Test frame timestamp handling
      - [ ] Test sample format conversion

- [ ] Implement TranscodingService

  - [ ] Write transcoding tests
    - [ ] Test format detection
    - [ ] Test output format selection
    - [ ] Test bitrate/quality settings
    - [ ] Test error handling (corrupt files, unsupported formats)
    - [ ] Test frame processing pipeline
      - [ ] Test decoder frame output
      - [ ] Test resampler frame handling
      - [ ] Test encoder frame input
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

  - [ ] Write buffer tests
    - [ ] Test chunk reading
    - [ ] Test transcoding pipeline
    - [ ] Test seek operations
    - [ ] Test concurrent access
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

  - [ ] Write CUE streaming tests
    - [ ] Test track boundary detection
    - [ ] Test seeking within tracks
    - [ ] Test track transitions
    - [ ] Test metadata handling
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
  - [ ] Write streaming endpoint tests
    - [ ] Test format negotiation (mp3, aac, raw)
    - [ ] Test bitrate limiting
    - [ ] Test seeking via time offset
    - [ ] Test estimated content length
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

  - [ ] Write endpoint tests
    - [ ] Test ping endpoint
    - [ ] Test getLicense endpoint
    - [ ] Test version negotiation
    - [ ] Test format handling (XML/JSON)
  - [ ] Create system endpoints
    - [ ] Add ping() -> Result<Response>
    - [ ] Add getLicense() -> Result<Response>
    - [ ] Add error response handling

- [ ] Implement browsing endpoints

  - [ ] Write browsing tests
    - [ ] Test getMusicFolders (single root folder)
    - [ ] Test getIndexes (artist-based hierarchy)
    - [ ] Test getArtists/getArtist
    - [ ] Test getAlbum/getAlbumList2
    - [ ] Test getSong
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

  - [ ] Write media tests
    - [ ] Test stream endpoint with transcoding
    - [ ] Test download endpoint
    - [ ] Test getCoverArt endpoint
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

  - [ ] Write playlist tests
    - [ ] Test getPlaylists/getPlaylist
    - [ ] Test createPlaylist
    - [ ] Test updatePlaylist
    - [ ] Test deletePlaylist
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

  - [ ] Write auth tests
    - [ ] Test token generation
    - [ ] Test token validation
    - [ ] Test user authentication
    - [ ] Test password hashing
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
  - [ ] Write middleware tests
    - [ ] Test response envelope
    - [ ] Test error handling
    - [ ] Test format selection
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
    - [x] Keep Blog component for now (minimal product)
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
    - [ ] Build album selection and preview components
    - [ ] Add data source selection (local folder, torrent)
    - [ ] Implement import progress tracking UI
  - [ ] Create settings management interface
    - [ ] Build storage configuration panel (S3 settings, local cache)
    - [ ] Add AI provider configuration interface
    - [ ] Create preferences and options screens
  - [ ] Add state management and data flow
    - [ ] Set up Dioxus signals for application state
    - [ ] Connect UI components to backend storage functions
    - [ ] Implement error handling and user feedback
    - [ ] Add loading states for async operations

## Deployment & Distribution

- [ ] Create installer for multiple platforms
- [ ] Implement auto-update functionality
- [ ] Create user documentation
- [ ] Build backup and restore functionality
