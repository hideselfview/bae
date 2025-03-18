# bae

_**bae**_ is an album-oriented music library application that uses a metadata-first approach to music library management. Traditional music library applications typically begin with music files and provide tools to manage associated metadata. In contrast, bae starts with album metadata from Discogs as the source of truth, and then matches it with music data. This approach results in a library with verified track listings, artist information, and release details, while storing the underlying music data in the cloud without disk constraints.

See [TASKS.md](TASKS.md) for implementation progress and detailed task breakdown.

## Add albums

- **Choose an album**: Use the [Discogs API](https://www.discogs.com/developers)
  to search for and select an album.
- **Provide a data source**: Locate existing data on the filesystem, and/or
  specify a "remote" where the data can be fetched.

  - **Remote sources**:

    - **Torrent**: A magnet link or .torrent file is used to verify/retrieve data.
      - Files to retreive from the torrent are identified with AI using the
        contents of the torrent and the release information.
      - The torrent is seeded when complete.
    - **Custom**: Provided by plugins.

- **Storage**:

  - Library metadata (albums, artists, tracks) is persisted in SQLite
  - When albums are imported:
    - Music data is split into chunks
    - Each chunk is encrypted
    - Chunks are uploaded to user-configurable cloud storage
    - SQLite tracks which chunks make up which files
  - Local chunk management:
    - Configure how many GB of chunks to keep locally
    - Recently used chunks stay local for faster access
    - When over the limit, least recently used chunks are removed (files remain in cloud)
  - During playback/seeding:
    - Required chunks are fetched from cloud if not available locally
    - Chunks are decrypted when retrieved, stored decrypted locally

## Browse and stream

- Served via a Subsonic-compatible API. Use a Subsonic client to browse and stream.
- Source data is transcoded out of storage chunks on-the-fly. Album tracks are
  mapped to data using AI. bae can handle:
  - **File-per-track**: A file for every track.
  - **CUE/FLAC**: A cue file that maps into a single FLAC file CD image.

## Stack

- **Backend/Core**:

  - Rust for core functionality (audio processing, file operations, database management)
  - ffmpeg via Rust bindings for audio transcoding and manipulation
  - SQLite for metadata persistence
  - libtorrent-rs for BitTorrent functionality with custom storage backend integration

- **Frontend**:

  - Tauri as the application framework
  - TypeScript + React for the user interface

## Development Approach

This project explores _README-driven development_ as a potential approach for agentic LLM development. The hypothesis is that curating context for LLMs in the form of README and TASKS directly in the codebase, along with the process involved in doing this, will be a good fit for LLM-driven development.

The process we're exploring:

1. Features are documented in this README
2. Implementation tasks are broken down in [TASKS.md](TASKS.md) with specific, actionable steps
3. Code is written by LLMs based on these descriptions and tasks
4. Results are reviewed and tested by humans
5. Documentation is updated based on implementation learnings
6. If implementation fails, the documentation and task breakdown are improved until they're clear enough for LLM implementation

### Motivation

We're exploring this approach to:

- Preserve valuable prompts and LLM interactions as part of the codebase
- Retain design context that would otherwise be lost after coding sessions
- Maintain technical documentation that evolves alongside the implementation
- Create self-documenting code and capture thought process
- Facilitate collaboration between contributors across time
