# bae Library Configuration Specification

This document specifies how bae manages library configuration and credentials.

## Overview

bae uses different configuration approaches for development and production:
- **Development mode**: `.env` file contains all configuration (debug builds only)
- **Production mode**: System keyring stores credentials (release builds only)

## Development Mode

### Activation

Dev mode activates when **both** conditions are met:
1. **Debug build**: Application compiled with `cargo build` (not `--release`)
2. **`.env` file exists**: Present in the repository root

In release builds, `.env` file loading code does not exist and is completely ignored.

### Configuration File

**File location:** `.env` (repository root, git-ignored)

**Structure:**
```bash
# S3 Configuration
BAE_S3_BUCKET=bae-dev
BAE_S3_REGION=us-east-1
BAE_S3_ACCESS_KEY=minioadmin
BAE_S3_SECRET_KEY=minioadmin
BAE_S3_ENDPOINT=http://localhost:9000  # Optional, for S3-compatible services

# Encryption key (hex-encoded 32-byte key for AES-256)
BAE_ENCRYPTION_KEY=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef

# Discogs API key
BAE_DISCOGS_API_KEY=your-discogs-key-here

# Library ID (optional, will be auto-generated if missing)
BAE_LIBRARY_ID=550e8400-e29b-41d4-a716-446655440000
```

### Encryption Key Management

Store the encryption key directly in the `.env` file:

```bash
BAE_ENCRYPTION_KEY=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
```

**Generating a key:**
```bash
# Generate random 256-bit key
openssl rand -hex 32
```

Copy the output and paste it into your `.env` file.

### Git Ignore

Add to `.gitignore`:
```gitignore
# Development environment
.env

# But keep example file
!.env.example
```

### Example Configuration File

**`.env.example`** (committed to repo):
```bash
# Copy this to .env and fill in your values

# S3 Configuration (use MinIO locally - see Development Workflow below)
BAE_S3_BUCKET=bae-dev
BAE_S3_REGION=us-east-1
BAE_S3_ENDPOINT=http://localhost:9000
BAE_S3_ACCESS_KEY=minioadmin
BAE_S3_SECRET_KEY=minioadmin

# Encryption key - generate with: openssl rand -hex 32
BAE_ENCRYPTION_KEY=generate-a-new-key-here

# Discogs API key - get from https://www.discogs.com/settings/developers
BAE_DISCOGS_API_KEY=your-discogs-key-here

# Optional: Will be auto-generated if missing
# BAE_LIBRARY_ID=550e8400-e29b-41d4-a716-446655440000
```

### Development Workflow

**First-time setup:**
1. Start MinIO locally:
   ```bash
   docker run -p 9000:9000 -p 9001:9001 \
     -e MINIO_ROOT_USER=minioadmin \
     -e MINIO_ROOT_PASSWORD=minioadmin \
     quay.io/minio/minio server /data --console-address ":9001"
   ```
2. Copy `.env.example` to `.env`
3. Generate encryption key: `openssl rand -hex 32`
4. Add encryption key to `.env`
5. Get Discogs API key from https://www.discogs.com/settings/developers
6. Add Discogs API key to `.env`
7. Run in debug mode: `cargo run` or `dx serve`
8. App detects `.env` file and activates dev mode automatically

**Switching to production:**
- Production build (`cargo build --release`) ignores `.env` completely
- Uses system keyring instead
- No code changes needed, just different build profile

### Security Considerations

**Why dev mode is insecure:**
- Credentials stored in plain text files
- Encryption key stored in plain text in `.env`
- No OS-level security (keyring)
- Easy to accidentally commit secrets

**Mitigations:**
- Only activates in debug builds (compile-time enforcement)
- `.env` loading code doesn't exist in release builds
- Clear documentation about security risks
- Example file committed to repo with placeholder values

**Production safeguards:**
```rust
// .env file loading is only compiled in debug builds
#[cfg(debug_assertions)]
fn load_config() {
    // Try to load .env file if it exists
    if dotenvy::dotenv().is_ok() {
        // Dev mode: use environment variables
        return load_from_env();
    }
    // No .env: use production config
    load_from_keyring()
}

#[cfg(not(debug_assertions))]
fn load_config() {
    // Release builds always use keyring
    // .env loading code doesn't exist here
    load_from_keyring()
}
```

## Production Mode

### Credential Storage

**System Keyring Keys:**
- `bae.library.{library_id}.s3_access_key` → S3 access key
- `bae.library.{library_id}.s3_secret_key` → S3 secret key
- `bae.library.{library_id}.encryption_key` → AES-256 master key
- `bae.discogs.api_key` → Discogs API key

**Why keyring?**
- Platform-secure storage (macOS Keychain, Windows Credential Manager, Linux Secret Service)
- Keeps credentials out of plain text files
- Standard practice for desktop applications

### Configuration Loading

Production builds load credentials from the system keyring. If credentials are missing, the application expects them to have been previously stored in the keyring.

## Storage Locations

### Local Storage (`~/.bae/`)

- Database: `~/.bae/library.db` (SQLite)
- Cache directory: `~/.bae/cache/` (encrypted chunks, LRU eviction)

### Primary Storage (S3)

- Encrypted chunks: `s3://bucket/chunks/{shard1}/{shard2}/{chunk_id}.enc`
- Hash-partitioned using first 4 UUID characters for S3 prefix distribution

### Database

**Location:** `~/.bae/library.db`

SQLite database containing:
- Album and track metadata
- File and chunk mappings
- CUE sheet data (for CUE/FLAC albums)
- Track positions (for seeking within CUE/FLAC files)

The database is stored locally only. There is no automatic sync to S3.

## Security

### Encryption Keys

- Master encryption key generated once
- Stored in system keyring (production) or `.env` file (dev mode)
- Used for AES-256-GCM chunk encryption
- Never transmitted to S3 (only encrypted data is uploaded)

### S3 Permissions

The S3 user/role needs these permissions:
```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "s3:GetObject",
        "s3:PutObject",
        "s3:ListBucket",
        "s3:DeleteObject"
      ],
      "Resource": [
        "arn:aws:s3:::my-music-bucket/*",
        "arn:aws:s3:::my-music-bucket"
      ]
    }
  ]
}
```

## Configuration Parameters

### Chunk Size

Configurable via environment variable (dev mode) or keyring (production):
- Variable: `BAE_CHUNK_SIZE_BYTES`
- Default: `1048576` (1MB)
- Affects memory usage during import and streaming

### Worker Concurrency

**Encryption workers** (CPU-bound):
- Variable: `BAE_MAX_ENCRYPT_WORKERS`
- Default: `2 × CPU cores`

**Upload workers** (I/O-bound):
- Variable: `BAE_MAX_UPLOAD_WORKERS`
- Default: `20`

These settings control parallelism during the import pipeline.
