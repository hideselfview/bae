# bae Implementation Progress

> [!NOTE]  
> bae has moved beyond **pre-implementation** and now has a working core system!
> We have a functional album import workflow with encrypted chunk storage.
> Our current focus is expanding functionality while keeping these things out of scope:
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

## ✅ Currently Working Features

**Complete Album Import Pipeline:**
- Search Discogs for albums (masters and releases)
- Select local folder with audio files  
- Real-time import with progress tracking
- Automatic file-to-track mapping
- AES-256-GCM encryption of all chunks
- SQLite database storage of metadata
- Secure key management via system keyring

**What happens when you import an album:**
1. Search Discogs API for album metadata
2. Select local folder containing music files
3. Create album/track records in SQLite database
4. Split each audio file into 1MB encrypted chunks
5. Store chunk metadata with integrity checksums
6. Files are now ready for cloud storage upload

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
  - [x] Implement application state management and error handling
- [x] Build album import workflow
  - [x] Create album search and selection interface
  - [x] Build album selection and preview components
  - [x] Add data source selection (local folders)
  - [x] Implement import progress tracking and feedback
  - [x] Add album import workflow orchestration
- [ ] Implement library browsing system
  - [ ] Build artist and album browsing system
  - [ ] Create library browsing UI components with search and filtering
  - [ ] Implement sorting and organization features
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
- [ ] Implement S3 storage integration
  - [ ] Select and configure S3 client library
  - [ ] Build S3 upload/download operations
  - [ ] Implement remote chunk tracking
  - [ ] Add local cache management with size limits
- [ ] Build unified storage controller
  - [ ] Create storage abstraction layer
  - [ ] Implement hybrid local/remote storage operations
  - [ ] Add chunk caching and eviction
  - [ ] Build streaming-optimized chunk access
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

- [ ] Implement CUE sheet support
  - [ ] Select and integrate CUE sheet parser
  - [ ] Build CUE sheet parsing and track extraction

- [ ] Implement FLAC + CUE integration
  - [ ] Build FLAC with CUE sheet processing
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

- [ ] Implement core Subsonic system endpoints
  - [ ] Build basic system status and license endpoints
  - [ ] Add error response handling

- [ ] Implement Subsonic browsing API
  - [ ] Build library browsing endpoints (folders, artists, albums, songs)
  - [ ] Add XML and JSON response formatting
  - [ ] Implement Subsonic response envelope system

- [ ] Implement media streaming endpoints
  - [ ] Build streaming, download, and cover art endpoints
  - [ ] Integrate streaming endpoints with transcoding system
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
