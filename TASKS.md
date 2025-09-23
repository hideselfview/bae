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

- [x] Set up local Rust development environment with Dioxus tooling
- [x] Set up Dioxus desktop project structure

## Core Features

### Album Management

- [x] Implement Discogs API integration
  - [x] Build Discogs HTTP client
  - [x] Add release search and retrieval capabilities
  - [x] Implement secure API key management
- [x] Create album metadata models
  - [x] Design and test album, track, and artist data structures
  - [x] Implement metadata validation and parsing
- [x] Build album import interface
  - [x] Create album search and browsing UI
  - [x] Build album selection and preview components
  - [ ] Add data source selection (local folders)
  - [ ] Implement import progress tracking and feedback
  - [ ] Add album import workflow orchestration
- [ ] Implement library browsing
  - [ ] Build artist and album browsing system
  - [ ] Create library browsing UI components
  - [ ] Add search and filtering capabilities
  - [ ] Implement sorting and organization features

### Storage Strategy

- [ ] Implement local library storage
  - [ ] Set up local storage directory structure
  - [ ] Initialize SQLite database with proper schema
  - [ ] Design database tables for albums, tracks, and chunks
- [ ] Create file chunking system
  - [ ] Design chunk format and structure
  - [ ] Implement chunk creation and management
  - [ ] Add chunk tracking and indexing
  - [ ] Build chunk reading and assembly operations
- [ ] Integrate encryption for chunk security
  - [ ] Select and integrate encryption library
  - [ ] Implement chunk encryption and decryption
  - [ ] Add encryption key management
- [ ] Implement S3 storage integration
  - [ ] Select and configure S3 client library
  - [ ] Build S3 upload/download operations
  - [ ] Implement remote chunk tracking
  - [ ] Add local cache management with size limits
- [ ] Build unified storage controller
  - [ ] Create storage abstraction layer
  - [ ] Implement hybrid local/remote storage operations
  - [ ] Add intelligent chunk caching and eviction
  - [ ] Build streaming-optimized chunk access
- [ ] Build library manager
  - [ ] Create library management system
  - [ ] Implement album and track import workflows
  - [ ] Build track-to-chunk mapping and playback system
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

- [ ] Implement track mapping system
  - [ ] Design track mapping abstraction and interfaces

- [ ] Implement simple track mapping for individual files
  - [ ] Add audio format detection and validation
  - [ ] Integrate AI for intelligent track matching
  - [ ] Build track mapping persistence system

- [ ] Implement CUE sheet support
  - [ ] Select and integrate CUE sheet parser
  - [ ] Build CUE sheet parsing and track extraction

- [ ] Implement FLAC + CUE integration
  - [ ] Build FLAC with CUE sheet processing
  - [ ] Add CUE-to-Discogs track mapping
  - [ ] Extend persistence for CUE-based tracks

- [ ] Design AI-powered track matching
  - [ ] Create AI prompts for intelligent file-to-track matching
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
  - [ ] Add comprehensive error response handling

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

## Desktop Application UI

- [x] Build desktop application interface
  - [x] Create main application layout and navigation
  - [x] Set up routing system for core application views
  - [ ] Build library browsing interface with search and filtering
  - [x] Implement album import workflow UI
    - [x] Create album search and selection interface
    - [ ] Add data source selection and import progress tracking
  - [ ] Create comprehensive settings management interface
  - [x] Implement application state management and error handling

## Deployment & Distribution

- [ ] Create installer for multiple platforms
- [ ] Implement auto-update functionality
- [ ] Create user documentation
- [ ] Build backup and restore functionality
