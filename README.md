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

  - Dioxus for native desktop application with built-in UI components

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

## Development Setup

### Prerequisites

This project uses **rustup** for Rust toolchain management (similar to how `nvm` manages Node.js versions). The [`rust-toolchain.toml`](rust-toolchain.toml) file automatically ensures everyone uses the same Rust version and components.

### Installation

1. **Install Rust via rustup** (the official installer):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source ~/.cargo/env  # Only needed for current session; rustup adds itself to your PATH automatically
   ```

2. **Install Dioxus CLI** globally:
   ```bash
   cargo install dioxus-cli --locked
   ```

3. **Clone and verify** the project setup:
   ```bash
   git clone <repository-url>
   cd bae
   rustup show  # Should show: "overridden by '.../rust-toolchain.toml'"
   ```

### Running the Application

```bash
cd bae  # Navigate to the Dioxus app directory
dx serve
```

The development server provides hot reloading and will automatically rebuild when you make changes.

### How It Works

- **rustup** manages Rust versions globally (like `nvm` for Node.js)  
- **rust-toolchain.toml** specifies the exact Rust version for this project  
- When you `cd` into the project, rustup automatically switches to the specified toolchain  
- **dx CLI** is installed globally

## Development Commands

### Dioxus Commands (dx)
Use `dx` for Dioxus-specific development with hot reloading and platform targeting:

```bash
dx serve          # Start development server with hot reloading
dx build          # Build for production (creates desktop app)
dx build --release # Production build with optimizations
dx check          # Quick syntax/type check via dx
```

### Standard Rust Commands (cargo)
Use `cargo` for standard Rust development and testing:

```bash
cargo check       # Fast compile check (no executable)
cargo build       # Build the binary
cargo run         # Build and run the binary
cargo test        # Run tests
cargo clippy      # Run linter
cargo fmt         # Format code
```

### When to Use Which?

- **Use `dx`** for Dioxus app development - it handles UI assets, hot reloading, and cross-platform builds
- **Use `cargo`** for standard Rust tasks like testing, linting, and when you need direct control over compilation

Both tools respect your `rust-toolchain.toml` and will use the same Rust version automatically.

## Tailwind CSS Setup

The project uses Tailwind CSS for styling with automatic compilation during builds.

### Prerequisites

1. **Install Node.js and npm**: https://docs.npmjs.com/downloading-and-installing-node-js-and-npm
2. **Install project dependencies**: Run `npm install` in the `bae/` directory to install Tailwind CSS v4

### How It Works

- **Automatic compilation**: The `build.rs` script automatically runs `tailwindcss` during every `cargo build`
- **Source scanning**: Tailwind scans your Rust files (`src/**/*.rs`) for class names
- **Optimized output**: Only the CSS classes you actually use are included in the final `assets/tailwind.css` file
- **No manual steps**: Just run `cargo build` or `dx serve` and Tailwind CSS is automatically generated

### Manual Compilation (Optional)

If you need to manually regenerate the CSS:

```bash
cd bae
npx tailwindcss -i tailwind.css -o assets/tailwind.css
```

### Tailwind CSS v4 Features

This project uses Tailwind CSS v4 with the new CSS-based configuration:
- **CSS Configuration**: Uses `@import "tailwindcss"` and `@source` directives in `tailwind.css`
- **Local Installation**: Tailwind CSS is installed as a local dependency (version pinned in `package.json`)
- **Automatic CLI**: Uses `npx tailwindcss` to run the locally installed version