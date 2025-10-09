# bae Implementation Progress

> [!NOTE]  
> bae has moved beyond **pre-implementation** and now has a working core system!
> We have a functional album import workflow with encrypted chunk storage and basic Subsonic streaming.
> **Current Architecture**: Cloud-first storage with S3 as primary, local cache for streaming, database sync to S3.
> **Current Focus**: Building first-launch setup wizard and library initialization system.
> Our current focus is completing the simplified cloud-first storage model while keeping these things out of scope:
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

- [x] Set up local Rust development environment with Dioxus tooling
- [x] Set up Dioxus desktop project structure
- [x] Add local development mode support (see `BAE_LIBRARY_CONFIGURATION.md`)
  - [x] Create `.env.example` file with all configuration options
  - [x] Update `.gitignore` to exclude `.env` file
  - [x] Hardwire dev storage path to `/tmp/bae-dev-storage`
  - [ ] Implement `.env` file loading in debug builds
  - [ ] Add local filesystem storage mode (alternative to S3)
  - [ ] Add dev mode warning banner in UI
  - [ ] Add compile-time check to prevent dev mode in release builds
- [ ] Build first-launch setup wizard (see `BAE_LIBRARY_CONFIGURATION.md`)
  - [ ] Create setup wizard UI (S3 configuration + optional Discogs)
  - [ ] Implement S3 connection validation
  - [ ] Add library manifest detection (`s3://bucket/bae-library.json`)
  - [ ] Build library initialization for new libraries
  - [ ] Implement config persistence to `~/.bae/config.yaml`
  - [ ] Add credential storage via system keyring
  - [ ] Add startup check to redirect to setup if unconfigured

## Core Features

### Album Management & Desktop Interface

- [x] Implement Discogs API integration
  - [x] Build Discogs HTTP client
  - [x] Add release search and retrieval capabilities
  - [x] Implement secure API key management
- [x] Create album metadata models
  - [x] Design and test album, track, and artist data structures
  - [x] Implement metadata validation and parsing
- [x] Build desktop application foundation
  - [x] Create main application layout and navigation
  - [x] Set up routing system for core application views
  - [x] Add album detail route (/album/:id) with navigation
  - [x] Implement application state management and error handling
- [x] Build album import workflow
  - [x] Create album search and selection interface
  - [x] Build album selection and preview components
  - [x] Add data source selection (local folders)
  - [x] Implement import progress tracking and feedback
  - [x] Add album import workflow orchestration
- [x] Implement library browsing system
  - [x] Build artist and album browsing system
  - [x] Create library browsing UI components with search and filtering
  - [x] Add album detail view with tracklist and metadata
  - [x] Implement click-through navigation from library to album details
  - [ ] Implement sorting and organization features (pending)
- [ ] Create settings management interface
- [ ] Implement multi-library support (future)
  - [ ] Add library switching UI in settings
  - [ ] Support multiple libraries in `config.yaml`
  - [ ] Implement library switching with database reload
  - [ ] Add per-library database storage in `~/.bae/libraries/{id}/`

### Storage Strategy

- [x] Implement local library storage
  - [x] Set up local storage directory structure
  - [x] Initialize SQLite database with proper schema
  - [x] Design database tables for albums, tracks, and chunks
- [x] Create file chunking system
  - [x] Design chunk format and structure
  - [x] Implement chunk creation and management
  - [x] Add chunk tracking and indexing
  - [x] Build chunk reading and assembly operations
- [x] Integrate encryption for chunk security
  - [x] Select and integrate encryption library (AES-256-GCM)
  - [x] Implement chunk encryption and decryption
  - [x] Add encryption key management (system keyring + in-memory for testing)
- [x] Implement S3 storage integration
  - [x] Select and configure S3 client library (AWS SDK)
  - [x] Build S3 upload/download operations
  - [x] Implement remote chunk tracking
  - [x] Add hash-based chunk partitioning for scalability
  - [x] Complete cloud chunk download in streaming pipeline
- [x] Build cloud-first storage system
  - [x] Update import flow to be cloud-first (no local ~/Music storage)
  - [x] Implement CacheManager for local chunk caching with LRU eviction
  - [x] Integrate cache layer into streaming pipeline
  - [ ] Implement database sync to S3 (after imports and periodic)
  - [ ] Add library manifest creation/detection in S3
- [x] Build library manager
  - [x] Create library management system
  - [x] Implement album and track import workflows
  - [x] Build track-to-chunk mapping system (playback system pending)
- [ ] Create storage settings interface
  - [ ] Build storage configuration UI components
  - [ ] Add storage usage monitoring and display
  - [ ] Implement settings persistence and credential management

### AI Setup (used in track mapping)

- [ ] Select and integrate AI provider for track matching
- [ ] Implement AI provider configuration and credential management
- [ ] Build AI service abstraction layer
- [ ] Create AI settings interface

### Track Mapping

- [x] Implement track mapping system
  - [x] Design track mapping abstraction and interfaces

- [x] Implement simple track mapping for individual files
  - [x] Add audio format detection and validation
  - [ ] Integrate AI for track matching (using simple filename-based mapping for now)
  - [x] Build track mapping persistence system

- [x] Implement CUE sheet support (see `BAE_CUE_FLAC_SPEC.md`)
  - [x] Phase 1: Database schema changes
    - [x] Add FLAC headers and CUE support columns to files table
    - [x] Create cue_sheets table for parsed CUE data
    - [x] Create track_positions table for track boundaries
    - [x] Update database models and queries
  - [x] Phase 2: Import changes
    - [x] Implement CUE sheet parser using nom crate
    - [x] Implement FLAC header extractor
    - [x] Add CUE/FLAC detection to import workflow
    - [x] Update chunking to store headers in DB and chunk entire files
  - [x] Phase 3: Streaming changes  
    - [x] Update streaming to use track positions for chunk ranges
    - [x] Implement chunk range queries for efficient downloads
    - [x] Add header prepending logic from database
    - [x] Integrate audio seeking library for precise track boundaries
  - [x] Phase 4: Audio processing
    - [x] Research audio libraries (symphonia, rodio, ffmpeg)
    - [x] Implement precise track seeking within FLAC streams
    - [x] Add format conversion if needed for client compatibility
    - [x] Optimize streaming performance for CUE tracks
  - [ ] Add AI-powered CUE-to-Discogs track mapping (currently uses simple filename matching)

- [ ] Design AI-powered track matching
  - [ ] Create AI prompts for file-to-track matching
  - [ ] Build AI validation for CUE sheet mapping
  - [ ] Add manual mapping fallback system


## Browsing & Streaming

### Transcoding

- [ ] Select and integrate audio processing library
  - [ ] Choose appropriate ffmpeg bindings for transcoding

- [ ] Build audio transcoding system
  - [ ] Create transcoding service with format support
  - [ ] Implement audio format detection and conversion
  - [ ] Add quality configuration and progress tracking
  - [ ] Build streaming transcoder with seeking support

- [ ] Build audio streaming buffer system
  - [ ] Create configurable streaming buffer
  - [ ] Implement chunk-to-frame conversion
  - [ ] Add buffer management with underrun/overrun handling
  - [ ] Build efficient seeking within streams

- [ ] Implement CUE-based streaming
  - [ ] Build CUE sheet streaming handler
  - [ ] Add track boundary calculation and seeking
  - [ ] Implement seamless track transitions

- [ ] Add streaming management functions
  - [ ] Implement stream format negotiation
  - [ ] Build stream lifecycle and control operations
  - [ ] Add stream progress monitoring

- [ ] Build Subsonic streaming integration
  - [ ] Create streaming endpoint handler
  - [ ] Implement concurrent stream management
  - [ ] Add client lifecycle and disconnection handling

### Subsonic API Implementation

- [x] Implement core Subsonic system endpoints
  - [x] Build basic system status and license endpoints (ping, getLicense)
  - [x] Add error response handling with proper Subsonic envelope

- [x] Implement Subsonic browsing API
  - [x] Set up Axum web server foundation running on localhost:4533
  - [x] Build library browsing endpoints (getArtists, getAlbumList, getAlbum)
  - [x] Add JSON response formatting with Subsonic 1.16.1 envelope
  - [x] Implement Subsonic response envelope system
  - [x] Integrate with LibraryManager for database access

- [x] Implement media streaming endpoints
  - [x] Build streaming endpoint with chunk reassembly from encrypted storage
  - [x] Add chunk download from cloud storage
  - [x] Integrate cache layer for high-performance streaming
  - [ ] Add HTTP range request support for seeking
  - [ ] Implement Discogs cover art proxy with caching

- [ ] Implement playlist management API
  - [ ] Build playlist CRUD endpoints
  - [ ] Add playlist persistence to database
  - [ ] Implement playlist modification operations

- [ ] Build authentication system
  - [ ] Implement user management with secure password hashing
  - [ ] Build token-based authentication and session management
  - [ ] Create authentication middleware with role-based access
  - [ ] Add user and session persistence

- [ ] Build Subsonic response middleware
  - [ ] Create response envelope and format handling
  - [ ] Implement XML/JSON serialization with JSONP support
  - [ ] Add Subsonic-compatible error translation


## Future Features

### Torrent Integration
- [ ] Design torrent workflow integration
  - [ ] Research chunk-to-torrent mapping approach
  - [ ] Design checkout/recreation system for seeding
  - [ ] Plan BitTorrent client integration
  - [ ] Add torrent management UI

## Deployment & Distribution

- [ ] Create installer for multiple platforms
- [ ] Implement auto-update functionality
- [ ] Create user documentation
- [ ] Build backup and restore functionality

## Current Import Process

**Initial setup (first launch):**

*Production mode:*
1. **Setup wizard** prompts for S3 configuration (required) and Discogs API key (optional)
2. **Library detection** checks for `bae-library.json` manifest in S3 bucket
3. **Library initialization** creates manifest if new library, or downloads database if existing
4. **Config persistence** saves library settings to `~/.bae/config.yaml`

*Development mode (`.env` file):*
1. **Auto-initialization** loads config from `.env`, skips setup wizard
2. **Local storage** uses `/tmp/bae-dev-storage/` directory instead of S3 (if enabled)
3. **Insecure mode** credentials in plain text, only works in debug builds

**Album import workflow:**
1. **User selects source folder** containing album files
2. **File scanning** identifies audio files and matches them to Discogs tracklist  
3. **Chunking** splits files into 1MB AES-256-GCM encrypted chunks
4. **Cloud upload** stores all chunks in S3 with hash-based partitioning (`chunks/ab/cd/chunk_uuid.enc`)
5. **Database update** saves album/track metadata and S3 chunk locations locally
6. **Database sync** uploads SQLite database to S3 (`bae-library.db`)
7. **Source folder** remains untouched on disk (no tracking, no management)

**Streaming:**
- Cache-first: Check local cache → Download from S3 → Cache → Decrypt → Stream
- Cache uses LRU eviction with configurable size limits (default: 1GB, 10K chunks)
- Manifest statistics update every 5-10 minutes in background
