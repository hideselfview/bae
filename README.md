# bae

**bae** is an **album-oriented**, **data-driven** music library app. Here's how it works:

## Add albums

- **Choose an album**: Use the Discogs API to search for and select a release.
- **Provide a data source**: Locate data on the filesystem, and/or specify a
  "remote" where the data can be fetched. In case local data and a remote are both
  specified, the remote will be used to verify the local data and refetch it if
  needed.

  - **Remote types**:

    - **Torrent**: A magnet link or .torrent file is used to verify/retrieve data.
      - Files to retreive from the torrent are identified with AI using the
        contents of the torrent and the release information.
      - The torrent is seeded when complete.
    - **Custom**: Provided by plugins.

- **Storage**:
  - Library metadata is persisted in SQLite
  - Music data is stored locally as opaque fixed sized blocks indexed into
    by information in the library metadata.
  - bae can be configured to offload music data that is not frequently accessed to the cloud:
    - Infrequently accessed chunks are encrypted and persisted to an S3-compatible store, then deleted from the
      local store.
    - A configurable GB of chunks are cached locally.
    - Playback/torrent seeding causes offloaded required chunks to be fetched.
    - Fetched chunks can evict stale chunks from the local cache.

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
