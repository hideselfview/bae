# bae Implementation Progress

## Setup & Initial Infrastructure

- [ ] Set up Tauri project structure
- [ ] Configure Rust backend with TypeScript/React frontend
- [ ] Create initial database schema in SQLite

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
- [ ] Create file chunking system (required for album data import)
- [ ] Integrate encryption library for cloud storage
- [ ] Integrate S3 client library
- [ ] Implement caching system for cloud chunks
  - [ ] Implement configurable cache size for local chunks
  - [ ] Implement chunk eviction strategy for stale chunks
- [ ] Build chunk retrieval mechanism
- [ ] Create storage management UI
  - [ ] Create cache management UI

### Album Management

- [ ] Implement/integrate Discogs API client for searching and selecting releases
- [ ] Create album metadata model
- [ ] Build UI for album search and selection
- [ ] Build UI for specifying local folder for album data
- [ ] Implement album data import from local folders (using storage chunking system)
- [ ] Design and implement album browser
  - [ ] Create artist view
  - [ ] Implement search functionality

### Track Mapping

- [ ] Implement simple one-file-per-track mapping logic
- [ ] Integrate or adapt existing CUE file parser library
- [ ] Implement FLAC + CUE mapping functionality
- [ ] Create AI prompts for audio track mapping
  - [ ] Build UI for manual track mapping correction
  - [ ] Implement automatic matching algorithm

### Remote Sources

- [ ] Integrate libtorrent-rs for BitTorrent functionality
- [ ] Implement custom storage backend for libtorrent-rs
- [ ] Create torrent search and selection UI
- [ ] Design AI prompts for torrent file identification
  - [ ] Add fallback strategies for when AI torrent identification fails
- [ ] Implement torrent seeding functionality
- [ ] Implement verification of local data against remote sources
- [ ] Build download manager for remote sources
  - [ ] Create download queue UI
  - [ ] Implement error handling for failed or interrupted downloads

## Browsing & Streaming

### Transcoding

- [ ] Integrate ffmpeg with appropriate Rust bindings
- [ ] Implement on-the-fly format conversion
  - [ ] Build buffering system for smooth playback
- [ ] Implement CUE-based streaming from FLAC files

### Subsonic API Implementation

- [ ] Implement basic Subsonic API endpoints
  - [ ] Add album art and metadata endpoints
  - [ ] Create streaming endpoints
- [ ] Create authentication system for API
- [ ] Implement playlist functionality

### Browsing UI

- [ ] Build settings interface for app configuration
- [ ] Implement playlist management UI

## Deployment & Distribution

- [ ] Create installer for multiple platforms
- [ ] Implement auto-update functionality
- [ ] Create user documentation
- [ ] Build backup and restore functionality
