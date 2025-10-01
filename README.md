# bae

**bae** is an album-oriented music library application that starts with metadata as the source of truth. Instead of beginning with music files and trying to organize them, bae uses the [Discogs database](https://www.discogs.com/developers) to establish verified album information first, then matches your music data to create a library with accurate track listings, artist information, and release details.

## What bae can do

bae works at two levels depending on your storage needs:

**Local-only music library**: Import albums through Discogs search, automatically map your audio files to verified track metadata, and stream everything through a Subsonic-compatible API. bae handles individual audio files as well as CUE/FLAC albums (single file containing entire album with track boundaries) seamlessly. Any Subsonic client can connect for browsing and playback.

**Cloud-backed music library**: Add S3-compatible cloud storage to automatically backup your music collection. Your albums are encrypted, chunked, and stored in the cloud while maintaining a local cache for streaming. This eliminates local storage constraints and enables access from multiple devices while keeping your music data private through encryption.

## How it works

bae uses a metadata-first approach that reverses the typical music library workflow. You search for albums in the Discogs database, select the specific release that matches your files, then provide the source folder containing your music data. bae automatically maps your files to the verified Discogs tracklist and imports everything into a unified library.

The application handles both traditional file-per-track albums and audiophile CUE/FLAC releases where a single FLAC file contains the entire album with a CUE sheet defining track boundaries. For CUE/FLAC albums, bae parses the timing information and can stream individual tracks without extracting separate files.

When cloud storage is configured, bae splits your music data into encrypted chunks and uploads them to S3-compatible storage. The original files remain untouched in their source folders, making bae torrent-friendly since you can continue seeding without any file modifications or moves. You can optionally remove source folders after import to save space, as bae can recreate them from cloud chunks when needed for seeding. A local cache keeps recently accessed chunks available for fast streaming while the complete library remains safely stored in the cloud.

## Streaming and compatibility

bae runs a Subsonic 1.16.1 compatible API server that works with existing Subsonic clients. This means you can use mobile apps like DSub or play:Sub, desktop players like Clementine, or web interfaces like Jamstash to browse and stream your library. The streaming system reassembles encrypted chunks in real-time and handles format conversion as needed.

For detailed technical information, see [BAE_STREAMING_ARCHITECTURE.md](BAE_STREAMING_ARCHITECTURE.md) which covers the chunk storage system, encryption model, and streaming pipeline.

## Album import process

The import workflow adapts to what you know about your music. If you know the specific pressing (original UK release, 180g remaster, etc.), you can import that exact release. If you only know the album title, you can import the master release with its canonical tracklist.

bae scans your source folder to identify audio files and matches them to the Discogs tracklist. For CUE/FLAC albums, it parses the CUE sheet to understand track boundaries within the single audio file. The complete import process including file detection and metadata mapping is documented in [BAE_IMPORT_WORKFLOW.md](BAE_IMPORT_WORKFLOW.md).

CUE/FLAC support includes parsing various CUE sheet formats, extracting FLAC headers for streaming, and calculating precise track positions for efficient chunk-based streaming. Technical details about CUE/FLAC handling are covered in [BAE_CUE_FLAC_SPEC.md](BAE_CUE_FLAC_SPEC.md).

## Technology stack

bae is built with Rust using Dioxus for the desktop interface and Axum for the Subsonic API server. Music processing uses the Symphonia audio framework with nom for CUE sheet parsing. Encryption uses AES-256-GCM with keys stored in the system keyring. Cloud storage integrates with any S3-compatible service through the AWS SDK.

The application uses SQLite for local metadata storage, tracking albums, tracks, files, and chunk locations. The database schema supports both individual audio files and CUE/FLAC albums with their associated timing and chunk mapping information.

## Development setup

This project uses rustup for Rust toolchain management with the exact version specified in `rust-toolchain.toml`. Install Rust via the official installer, then install the Dioxus CLI globally and run the development server:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install dioxus-cli --locked
git clone <repository-url>
cd bae/bae
dx serve
```

The application also uses Tailwind CSS for styling with automatic compilation during builds. Install Node.js and run `npm install` in the `bae/` directory to set up the CSS build system.

Implementation progress and detailed task breakdown are tracked in [TASKS.md](TASKS.md).