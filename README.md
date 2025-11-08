# bae

**bae** is an album-oriented music library that starts with metadata as the source of truth:

- Search the [Discogs database](https://www.discogs.com/developers) for albums
- Pick a master or specific release
- Point bae to your music files
- bae handles the rest:
  - Chunking and encrypting your music
  - Uploading to S3
  - Making everything streamable through a Subsonic-compatible API

Your music is encrypted and stored in S3-compatible cloud storage with a local cache for fast streaming. The SQLite database also lives in S3, so your library is fully cloud-backed. Any Subsonic client (DSub, play:Sub, Clementine, Jamstash) can connect to the local API for browsing and playback.

## How it works

### Setup 

On first launch, configure S3 storage and Discogs API key. The system detects existing libraries or initializes a new one. Configuration is stored in `~/.bae/config.yaml` with credentials in the system keyring.

### Import

Search Discogs for your album, select the specific release (or master if you just know the album title), then point to your source folder. bae scans the folder, matches files to the Discogs tracklist, chunks and encrypts everything, then uploads to S3. The local SQLite database syncs to S3 after each import.

### Streaming

bae runs a Subsonic 1.16.1 API server on localhost:4533. The streaming system downloads chunks from S3 (or local cache), decrypts them, and reassembles audio in real-time. Works with any Subsonic client.

### Format support

Handles traditional file-per-track albums and CUE/FLAC releases (single FLAC file with CUE sheet for track boundaries). For CUE/FLAC albums, bae parses timing information and streams individual tracks without extraction.

## Development setup

**Prerequisites:**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install dioxus-cli --locked
# Install cmake (required for libflac-sys to build libFLAC from source)
# macOS: brew install cmake
# Linux: apt-get install cmake (or equivalent)
# Windows: Download from https://cmake.org/download/
# macOS: Install libdiscid for MusicBrainz DiscID support
# macOS: brew install libdiscid pkg-config
# Note: .cargo/config.toml is included in the repo to configure libdiscid linking on macOS
```

**Quick start:**
```bash
# Start MinIO for dev
docker run -p 9000:9000 -p 9001:9001 \
  -e MINIO_ROOT_USER=minioadmin \
  -e MINIO_ROOT_PASSWORD=minioadmin \
  quay.io/minio/minio server /data --console-address ":9001"

# Setup bae
git clone <repository-url>
cd bae
./scripts/install-hooks.sh  # Install git hooks for formatting checks
npm install  # Tailwind CSS setup
cp .env.example .env
# Edit .env: 
#   - Add encryption key from: openssl rand -hex 32
#   - Add Discogs API key from: https://www.discogs.com/settings/developers
cd bae && dx serve
```

Dev mode activates automatically in debug builds when `.env` exists. Requires MinIO running locally and a valid Discogs API key. See [BAE_LIBRARY_CONFIGURATION.md](BAE_LIBRARY_CONFIGURATION.md) for details.

## Logging

bae uses structured logging with the `tracing` crate. Log levels can be controlled via the `RUST_LOG` environment variable:

```bash
# Show general information (default)
RUST_LOG=info cargo run

# Show detailed debugging information
RUST_LOG=debug cargo run

# Show debug logs only for the bae module
RUST_LOG=bae=debug cargo run

# Show debug logs only for specific modules
RUST_LOG=bae::cache=debug,bae::import=info cargo run

# Show all logs from all dependencies
RUST_LOG=trace cargo run
```

## Documentation

- [TASKS.md](TASKS.md) - Implementation progress and task breakdown
- [BAE_LIBRARY_CONFIGURATION.md](BAE_LIBRARY_CONFIGURATION.md) - Library setup, configuration, and multi-device access
- [BAE_IMPORT_WORKFLOW.md](BAE_IMPORT_WORKFLOW.md) - Album import process and Discogs integration
- [BAE_STREAMING_ARCHITECTURE.md](BAE_STREAMING_ARCHITECTURE.md) - Streaming system and Subsonic API
- [BAE_CUE_FLAC_SPEC.md](BAE_CUE_FLAC_SPEC.md) - CUE sheet and FLAC album support