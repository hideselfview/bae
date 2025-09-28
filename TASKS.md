# bae Implementation Progress

> [!NOTE]  
> bae has moved beyond **pre-implementation** and now has a working core system!
> We have a functional album import workflow with encrypted chunk storage and basic Subsonic streaming.
> **Current Architecture**: Cloud-first storage with S3 as primary, local cache for streaming, optional source folder checkouts.
> Our current focus is completing the cloud-first storage model while keeping these things out of scope:
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
  - [x] Implement CheckoutManager for source folder lifecycle
  - [x] Integrate cache layer into streaming pipeline
  - [x] Add source folder path tracking to database schema
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

- [ ] Implement CUE sheet support (see `BAE_CUE_FLAC_SPEC.md`)
  - [ ] Phase 1: Database schema changes
    - [ ] Add FLAC headers and CUE support columns to files table
    - [ ] Create cue_sheets table for parsed CUE data
    - [ ] Create track_positions table for track boundaries
    - [ ] Update database models and queries
  - [ ] Phase 2: Import changes
    - [ ] Implement CUE sheet parser using nom crate
    - [ ] Implement FLAC header extractor
    - [ ] Add CUE/FLAC detection to import workflow
    - [ ] Update chunking to skip headers and store them in DB
  - [ ] Phase 3: Streaming changes  
    - [ ] Update streaming to use track positions for chunk ranges
    - [ ] Implement chunk range queries for efficient downloads
    - [ ] Add header prepending logic from database
    - [ ] Integrate audio seeking library for precise track boundaries
  - [ ] Phase 4: Audio processing
    - [ ] Research audio libraries (symphonia, rodio, ffmpeg)
    - [ ] Implement precise track seeking within FLAC streams
    - [ ] Add format conversion if needed for client compatibility
    - [ ] Optimize streaming performance for CUE tracks
  - [ ] Add CUE-to-Discogs track mapping
  - [ ] Extend persistence for CUE-based tracks

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


## Deployment & Distribution

- [ ] Create installer for multiple platforms
- [ ] Implement auto-update functionality
- [ ] Create user documentation
- [ ] Build backup and restore functionality

## Current Import Process

**What happens when you import an album:**

1. **User selects source folder** containing album files
2. **File scanning** identifies audio files and matches them to Discogs tracklist  
3. **Chunking** splits files into 1MB AES-256-GCM encrypted chunks
4. **Cloud upload** stores all chunks in S3 with hash-based partitioning (`chunks/ab/cd/chunk_uuid.enc`)
5. **Source folder tracking** records original folder path for optional checkout recreation
6. **Database storage** saves album/track metadata and S3 chunk locations
7. **Import completion** - chunks exist only in S3, source folder remains for seeding/backup

**For streaming:**
- Cache-first streaming: Check local cache → Download from S3 → Cache → Decrypt → Stream
- Cache uses LRU eviction with configurable size limits (default: 1GB, 10K chunks)
- CheckoutManager can recreate original files from cloud chunks for seeding/backup
