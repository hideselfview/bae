# bae Library Configuration Specification

This document specifies how bae manages library configuration, initialization, and multi-device access.

## Problem Statement

**Design Requirement:** bae needs a system for:
- Configuring S3 storage and Discogs credentials
- Detecting existing libraries vs. initializing new ones
- Supporting multiple libraries (future)
- Syncing library state across devices
- Maintaining secure credential storage

## Architecture Overview

bae uses a two-tier configuration system:

1. **Local config** (`~/.bae/config.yaml`) - Connection information for known libraries
2. **Remote manifest** (`s3://bucket/bae-library.json`) - Library metadata and settings stored in S3

This separation enables:
- Accessing the S3 bucket (credentials must be local)
- Managing multiple libraries (local list of all known libraries)
- Sharing library settings across devices (manifest in S3)
- Secure credential storage (system keyring)

## Local Configuration

### File Location
```
~/.bae/config.yaml
```

### Structure
```yaml
version: "1.0"
current_library: "550e8400-e29b-41d4-a716-446655440000"
libraries:
  - id: "550e8400-e29b-41d4-a716-446655440000"
    name: "My Music Library"
    s3_bucket: "my-music-bucket"
    s3_region: "us-east-1"
    s3_endpoint: null  # optional, for S3-compatible services
    # S3 credentials stored in system keyring:
    # - key: "bae.library.{id}.s3_access_key"
    # - key: "bae.library.{id}.s3_secret_key"
    created_at: "2025-10-09T14:30:00Z"
    last_accessed: "2025-10-09T14:30:00Z"
```

### Credential Storage

**System Keyring Keys:**
- `bae.library.{library_id}.s3_access_key` → S3 access key
- `bae.library.{library_id}.s3_secret_key` → S3 secret key
- `bae.library.{library_id}.encryption_key` → AES-256 master key
- `bae.discogs.api_key` → Discogs API key (shared across libraries)

**Why keyring?**
- Platform-secure storage (macOS Keychain, Windows Credential Manager, Linux Secret Service)
- Keeps credentials out of plain text files
- Standard practice for desktop applications

## S3 Library Manifest

### File Location
```
s3://bucket/bae-library.json
```

### Structure
```json
{
  "version": "1.0",
  "library_id": "550e8400-e29b-41d4-a716-446655440000",
  "created_at": "2025-10-09T14:30:00Z",
  "last_modified": "2025-10-09T15:45:00Z",
  "settings": {
    "cache_size_mb": 1024,
    "cache_max_chunks": 10000,
    "database_file": "bae-library.db",
    "chunk_size_bytes": 1048576
  },
  "statistics": {
    "total_albums": 150,
    "total_tracks": 2340,
    "total_chunks": 45000,
    "storage_bytes": 47244640256
  }
}
```

### Purpose
- **Library detection**: Check if library exists at S3 bucket
- **Validation**: Verify library ID matches configuration
- **Settings sync**: Share library settings across devices
- **Statistics**: Display library info in UI

## Database Storage

### Local Database Location
```
~/.bae/libraries/{library_id}/library.db
```

Each library gets its own SQLite database in a dedicated directory.

### S3 Database Backup
```
s3://bucket/bae-library.db
```

The database syncs to S3:
- **Immediately** after each successful album import (critical data)
- **On shutdown** to capture any pending changes
- **Manifest statistics** update every 5-10 minutes in background

### Database Sync Process

**Steps:**
1. Upload database file to S3 at `bae-library.db`
2. Download current manifest from S3
3. Update manifest timestamp and statistics
4. Upload updated manifest back to S3

## First Launch Flow

### Setup Wizard

**Screen 1: S3 Configuration** (required)
```
┌─────────────────────────────────────┐
│ Configure S3 Storage                │
├─────────────────────────────────────┤
│ Bucket Name:    [______________]    │
│ Region:         [us-east-1    ▼]    │
│ Access Key:     [______________]    │
│ Secret Key:     [______________]    │
│                                     │
│ Optional:                           │
│ Custom Endpoint: [______________]   │
│                                     │
│        [Cancel]  [Continue →]       │
└─────────────────────────────────────┘
```

**Screen 2: Discogs API** (optional)
```
┌─────────────────────────────────────┐
│ Discogs Integration (Optional)      │
├─────────────────────────────────────┤
│ Add your Discogs API key to enable  │
│ album search and metadata import.   │
│                                     │
│ API Key: [____________________]     │
│                                     │
│ Get your API key at:                │
│ https://www.discogs.com/settings/   │
│ developers                          │
│                                     │
│        [Skip]    [Continue →]       │
└─────────────────────────────────────┘
```

### Initialization Process

**Main flow:**
1. Validate S3 connection with provided credentials
2. Check for existing library manifest at `s3://bucket/bae-library.json`
3. If manifest exists → load existing library
4. If manifest missing → create new library

**Creating a new library:**
1. Generate new library ID (UUID)
2. Create local database directory at `~/.bae/libraries/{library_id}/`
3. Initialize empty SQLite database
4. Create manifest with default settings and zero statistics
5. Upload manifest to S3
6. Upload empty database to S3
7. Save library entry to local config
8. Store credentials in system keyring

**Loading an existing library:**
1. Read library ID from downloaded manifest
2. Check if local database exists at `~/.bae/libraries/{library_id}/library.db`
3. If missing, download database from S3
4. Open local database
5. Save library entry to local config if not already present
6. Store credentials in system keyring

## Multi-Library Support (Future)

### Switching Libraries

When the user switches to a different library:

1. **Close current library**: Sync database to S3, close connections
2. **Load new library**: Initialize CloudStorageManager with new credentials, open new database
3. **Update UI**: Reload all library data from new database
4. **Update config**: Set `current_library` to new library ID

### Adding a Library

User can add additional libraries through settings:
1. Provide S3 credentials for new bucket
2. System detects/initializes library (same flow as first launch)
3. Library added to `libraries` array in config
4. User can switch between libraries at any time

### Config with Multiple Libraries

```yaml
version: "1.0"
current_library: "550e8400-e29b-41d4-a716-446655440000"
libraries:
  - id: "550e8400-e29b-41d4-a716-446655440000"
    name: "My Music Library"
    s3_bucket: "my-music-bucket"
    s3_region: "us-east-1"
    created_at: "2025-10-09T14:30:00Z"
    last_accessed: "2025-10-09T14:30:00Z"
  
  - id: "7c9e6679-7425-40de-944b-e07fc1f90ae7"
    name: "Work Library"
    s3_bucket: "work-music-library"
    s3_region: "eu-west-1"
    created_at: "2025-10-10T09:15:00Z"
    last_accessed: "2025-10-08T16:20:00Z"
```

## Security Considerations

### Credential Storage
- Never store credentials in plain text files
- Use system keyring for all secrets
- Config file only contains non-sensitive connection info

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

### Encryption Keys
- Master encryption key generated on library creation
- Stored in system keyring
- Used for AES-256-GCM chunk encryption
- Never transmitted to S3

## Local Development Mode

**⚠️ WARNING: This mode is INSECURE and should ONLY be used for local development. Never use in production.**

### Overview

For local development, bae supports loading configuration from a `.env` file instead of `config.yaml` and system keyring. This simplifies the development workflow by avoiding S3 setup and keyring configuration.

### Activation

Dev mode activates when **both** conditions are met:
1. **Debug build**: Application compiled with `cargo build` (not `--release`)
2. **`.env` file exists**: Present in the repository root

**Detection flow:**
```
Application startup
    ↓
Is this a debug build?
    ↓ No → Use production config (config.yaml + keyring)
    ↓ Yes
    ↓
Does .env file exist?
    ↓ No → Use production config (config.yaml + keyring)
    ↓ Yes → Load .env and use dev mode
```

**In release builds:**
- `.env` file is completely ignored (loading code doesn't exist)
- Only `config.yaml` + keyring are used
- Setup wizard runs on first launch

### Configuration File

**File location:** `.env` (repository root, git-ignored)

**Structure:**
```bash
# Dev mode activates automatically in debug builds when this file exists

# S3 Configuration (optional - can use local filesystem instead)
BAE_S3_BUCKET=my-dev-bucket
BAE_S3_REGION=us-east-1
BAE_S3_ACCESS_KEY=AKIAIOSFODNN7EXAMPLE
BAE_S3_SECRET_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY
# BAE_S3_ENDPOINT=http://localhost:9000  # for MinIO/LocalStack

# Local filesystem mode (alternative to S3)
BAE_USE_LOCAL_STORAGE=true
BAE_LOCAL_STORAGE_PATH=/tmp/bae-dev-storage

# Encryption key (hex-encoded 32-byte key for AES-256)
BAE_ENCRYPTION_KEY=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef

# Discogs API key
BAE_DISCOGS_API_KEY=your-discogs-key-here

# Library ID (optional, will be generated if missing)
BAE_LIBRARY_ID=550e8400-e29b-41d4-a716-446655440000
```

### Local Storage Mode

When `BAE_USE_LOCAL_STORAGE=true`, bae uses local filesystem instead of S3:

**Storage structure:**
```
/tmp/bae-dev-storage/
├── bae-library.json          # Library manifest
├── bae-library.db            # SQLite database
└── chunks/
    ├── ab/
    │   └── cd/
    │       └── chunk_abcd1234....enc
    └── ...
```

This mirrors the S3 structure but uses local directories. The cache system still works the same way.

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

Note: `dev-storage/` is not needed in `.gitignore` since the hardwired path `/tmp/bae-dev-storage/` is outside the repository.

### Example Configuration File

**`.env.example`** (committed to repo):
```bash
# Copy this to .env and fill in your values
# Dev mode activates automatically in debug builds when this file exists

# Use local filesystem (recommended for dev)
BAE_USE_LOCAL_STORAGE=true
BAE_LOCAL_STORAGE_PATH=/tmp/bae-dev-storage

# OR use S3 (if you have MinIO/LocalStack running)
# BAE_S3_BUCKET=my-dev-bucket
# BAE_S3_REGION=us-east-1
# BAE_S3_ENDPOINT=http://localhost:9000
# BAE_S3_ACCESS_KEY=minioadmin
# BAE_S3_SECRET_KEY=minioadmin

# Generate with: openssl rand -hex 32
BAE_ENCRYPTION_KEY=generate-a-new-key-here

# Optional: Get from https://www.discogs.com/settings/developers
BAE_DISCOGS_API_KEY=your-discogs-key-here

# Optional: Will be auto-generated if missing
# BAE_LIBRARY_ID=550e8400-e29b-41d4-a716-446655440000
```

### Development Workflow

**First-time setup:**
1. Copy `.env.example` to `.env`
2. Generate encryption key: `openssl rand -hex 32`
3. Add encryption key to `.env`
4. Optionally add Discogs API key
5. Run in debug mode: `cargo run` or `dx serve`
6. App detects `.env` file and activates dev mode automatically
7. Local library initializes automatically at `/tmp/bae-dev-storage/`

**Switching to production:**
- Production build (`cargo build --release`) ignores `.env` completely
- Uses `config.yaml` + keyring instead
- Setup wizard runs on first launch
- No code changes needed, just different build profile

### Security Considerations

**Why this is insecure:**
- Credentials stored in plain text files
- Encryption key stored in plain text in `.env`
- No OS-level security (keyring)
- Easy to accidentally commit secrets

**Mitigations:**
- Only activates in debug builds (compile-time enforcement)
- Show warning banner in UI when dev mode active
- `.env` loading code doesn't exist in release builds
- Clear documentation about security risks

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
    load_from_config_yaml()
}

#[cfg(not(debug_assertions))]
fn load_config() {
    // Release builds always use config.yaml + keyring
    // .env loading code doesn't exist here
    load_from_config_yaml()
}
```

This ensures:
- Dev mode can only activate in debug builds (compile-time enforcement)
- `.env` file loading code doesn't even exist in release builds
- No way to accidentally use dev mode in production