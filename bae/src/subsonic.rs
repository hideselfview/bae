use crate::library::LibraryError;
use crate::library_context::SharedLibraryManager;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tower_http::cors::CorsLayer;

/// Subsonic API server state
#[derive(Clone)]
pub struct SubsonicState {
    pub library_manager: SharedLibraryManager,
    pub cache_manager: crate::cache::CacheManager,
    pub encryption_service: crate::encryption::EncryptionService,
    pub cloud_storage: Option<crate::cloud_storage::CloudStorageManager>,
}

/// Common query parameters for Subsonic API
#[derive(Debug, Deserialize)]
pub struct SubsonicQuery {}

/// Standard Subsonic API response envelope
#[derive(Debug, Serialize)]
pub struct SubsonicResponse<T> {
    #[serde(rename = "subsonic-response")]
    pub subsonic_response: SubsonicResponseInner<T>,
}

#[derive(Debug, Serialize)]
pub struct SubsonicResponseInner<T> {
    pub status: String,
    pub version: String,
    #[serde(flatten)]
    pub data: T,
}

/// Error response for Subsonic API
#[derive(Debug, Serialize)]
pub struct SubsonicError {
    pub code: u32,
    pub message: String,
}

/// License info (always valid for open source)
#[derive(Debug, Serialize)]
pub struct License {
    pub valid: bool,
    pub email: String,
    pub key: String,
}

/// Artist info for browsing
#[derive(Debug, Serialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    #[serde(rename = "albumCount")]
    pub album_count: u32,
}

/// Album info for browsing
#[derive(Debug, Serialize)]
pub struct Album {
    pub id: String,
    pub name: String,
    pub artist: String,
    #[serde(rename = "artistId")]
    pub artist_id: String,
    #[serde(rename = "songCount")]
    pub song_count: u32,
    pub duration: u32,
    pub year: Option<i32>,
    pub genre: Option<String>,
    #[serde(rename = "coverArt")]
    pub cover_art: Option<String>,
}

/// Song/track info for browsing
#[derive(Debug, Serialize)]
pub struct Song {
    pub id: String,
    pub title: String,
    pub album: String,
    pub artist: String,
    #[serde(rename = "albumId")]
    pub album_id: String,
    #[serde(rename = "artistId")]
    pub artist_id: String,
    pub track: Option<i32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    #[serde(rename = "coverArt")]
    pub cover_art: Option<String>,
    pub size: Option<i64>,
    #[serde(rename = "contentType")]
    pub content_type: String,
    pub suffix: String,
    pub duration: Option<i32>,
    #[serde(rename = "bitRate")]
    pub bit_rate: Option<i32>,
    pub path: String,
}

/// Artists index response
#[derive(Debug, Serialize)]
pub struct ArtistsResponse {
    pub artists: ArtistsIndex,
}

#[derive(Debug, Serialize)]
pub struct ArtistsIndex {
    pub index: Vec<ArtistIndex>,
}

#[derive(Debug, Serialize)]
pub struct ArtistIndex {
    pub name: String,
    pub artist: Vec<Artist>,
}

/// Albums response
#[derive(Debug, Serialize)]
pub struct AlbumListResponse {
    #[serde(rename = "albumList")]
    pub album_list: AlbumList,
}

#[derive(Debug, Serialize)]
pub struct AlbumList {
    pub album: Vec<Album>,
}

/// Create the Subsonic API router
pub fn create_router(
    library_manager: SharedLibraryManager,
    cache_manager: crate::cache::CacheManager,
    encryption_service: crate::encryption::EncryptionService,
    cloud_storage: Option<crate::cloud_storage::CloudStorageManager>,
) -> Router {
    let state = SubsonicState {
        library_manager,
        cache_manager,
        encryption_service,
        cloud_storage,
    };
    Router::new()
        .route("/rest/ping", get(ping))
        .route("/rest/getLicense", get(get_license))
        .route("/rest/getArtists", get(get_artists))
        .route("/rest/getAlbumList", get(get_album_list))
        .route("/rest/getAlbum", get(get_album))
        .route("/rest/stream", get(stream_song))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Ping endpoint - basic connectivity test
/// Ping endpoint - params required by Subsonic API spec but not used for simple health check
async fn ping(Query(_params): Query<SubsonicQuery>) -> impl IntoResponse {
    let response = SubsonicResponse {
        subsonic_response: SubsonicResponseInner {
            status: "ok".to_string(),
            version: "1.16.1".to_string(),
            data: serde_json::json!({}),
        },
    };

    Json(response)
}

/// Get license info - always return valid for open source
/// params required by Subsonic API spec but not used in this endpoint
async fn get_license(Query(_params): Query<SubsonicQuery>) -> impl IntoResponse {
    let license = License {
        valid: true,
        email: "opensource@bae.music".to_string(),
        key: "bae-open-source".to_string(),
    };

    let response = SubsonicResponse {
        subsonic_response: SubsonicResponseInner {
            status: "ok".to_string(),
            version: "1.16.1".to_string(),
            data: serde_json::json!({ "license": license }),
        },
    };

    Json(response)
}

/// Get artists index
/// params required by Subsonic API spec but not currently validated
async fn get_artists(
    Query(_params): Query<SubsonicQuery>,
    State(state): State<SubsonicState>,
) -> impl IntoResponse {
    match load_artists(&state.library_manager).await {
        Ok(artists_response) => {
            let response = SubsonicResponse {
                subsonic_response: SubsonicResponseInner {
                    status: "ok".to_string(),
                    version: "1.16.1".to_string(),
                    data: serde_json::json!(artists_response),
                },
            };
            Json(response).into_response()
        }
        Err(e) => {
            let error = SubsonicError {
                code: 0,
                message: format!("Failed to load artists: {}", e),
            };
            let response = SubsonicResponse {
                subsonic_response: SubsonicResponseInner {
                    status: "failed".to_string(),
                    version: "1.16.1".to_string(),
                    data: serde_json::json!({ "error": error }),
                },
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response)).into_response()
        }
    }
}

/// Get album list
/// params required by Subsonic API spec but not currently validated
async fn get_album_list(
    Query(_params): Query<SubsonicQuery>,
    State(state): State<SubsonicState>,
) -> impl IntoResponse {
    match load_albums(&state.library_manager).await {
        Ok(album_response) => {
            let response = SubsonicResponse {
                subsonic_response: SubsonicResponseInner {
                    status: "ok".to_string(),
                    version: "1.16.1".to_string(),
                    data: serde_json::json!(album_response),
                },
            };
            Json(response).into_response()
        }
        Err(e) => {
            let error = SubsonicError {
                code: 0,
                message: format!("Failed to load albums: {}", e),
            };
            let response = SubsonicResponse {
                subsonic_response: SubsonicResponseInner {
                    status: "failed".to_string(),
                    version: "1.16.1".to_string(),
                    data: serde_json::json!({ "error": error }),
                },
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response)).into_response()
        }
    }
}

/// Get album with tracks
async fn get_album(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<SubsonicState>,
) -> impl IntoResponse {
    let album_id = match params.get("id") {
        Some(id) => id.clone(),
        None => {
            let error = SubsonicError {
                code: 10,
                message: "Required parameter 'id' missing".to_string(),
            };
            let response = SubsonicResponse {
                subsonic_response: SubsonicResponseInner {
                    status: "failed".to_string(),
                    version: "1.16.1".to_string(),
                    data: serde_json::json!({ "error": error }),
                },
            };
            return (StatusCode::BAD_REQUEST, Json(response)).into_response();
        }
    };

    match load_album_with_songs(&state.library_manager, &album_id).await {
        Ok(album_response) => {
            let response = SubsonicResponse {
                subsonic_response: SubsonicResponseInner {
                    status: "ok".to_string(),
                    version: "1.16.1".to_string(),
                    data: album_response,
                },
            };
            Json(response).into_response()
        }
        Err(e) => {
            let error = SubsonicError {
                code: 70,
                message: format!("Album not found: {}", e),
            };
            let response = SubsonicResponse {
                subsonic_response: SubsonicResponseInner {
                    status: "failed".to_string(),
                    version: "1.16.1".to_string(),
                    data: serde_json::json!({ "error": error }),
                },
            };
            (StatusCode::NOT_FOUND, Json(response)).into_response()
        }
    }
}

/// Stream a song - reassemble encrypted chunks into audio stream
async fn stream_song(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<SubsonicState>,
) -> impl IntoResponse {
    let song_id = match params.get("id") {
        Some(id) => id.clone(),
        None => {
            return (StatusCode::BAD_REQUEST, "Missing song ID").into_response();
        }
    };

    println!("Streaming request for song ID: {}", song_id);

    match stream_track_chunks(&state, &song_id).await {
        Ok(audio_data) => {
            // Return the reassembled audio with proper headers
            let headers = [
                ("Content-Type", "audio/flac"), // TODO: Detect actual format
                ("Content-Length", &audio_data.len().to_string()),
                ("Accept-Ranges", "bytes"),
            ];

            (StatusCode::OK, headers, audio_data).into_response()
        }
        Err(e) => {
            println!("Streaming error for song {}: {}", song_id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Streaming error: {}", e),
            )
                .into_response()
        }
    }
}

/// Load artists from database and group by first letter
async fn load_artists(
    library_manager: &SharedLibraryManager,
) -> Result<ArtistsResponse, LibraryError> {
    let albums = library_manager.get().get_albums().await?;

    // Group artists by first letter
    let mut artist_map: HashMap<String, HashMap<String, u32>> = HashMap::new();

    for album in albums {
        let first_letter = album
            .artist_name
            .chars()
            .next()
            .unwrap_or('A')
            .to_uppercase()
            .to_string();

        let artists = artist_map.entry(first_letter).or_default();
        *artists.entry(album.artist_name).or_insert(0) += 1;
    }

    let mut indices = Vec::new();
    for (letter, artists) in artist_map {
        let artist_list: Vec<Artist> = artists
            .into_iter()
            .map(|(name, count)| Artist {
                id: format!("artist_{}", name.replace(" ", "_")),
                name,
                album_count: count,
            })
            .collect();

        if !artist_list.is_empty() {
            indices.push(ArtistIndex {
                name: letter,
                artist: artist_list,
            });
        }
    }

    // Sort indices by letter
    indices.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(ArtistsResponse {
        artists: ArtistsIndex { index: indices },
    })
}

/// Load albums from database
async fn load_albums(
    library_manager: &SharedLibraryManager,
) -> Result<AlbumListResponse, LibraryError> {
    let db_albums = library_manager.get().get_albums().await?;

    let mut albums = Vec::new();
    for db_album in db_albums {
        let tracks = library_manager.get().get_tracks(&db_album.id).await?;

        albums.push(Album {
            id: db_album.id.clone(),
            name: db_album.title,
            artist: db_album.artist_name.clone(),
            artist_id: format!("artist_{}", db_album.artist_name.replace(" ", "_")),
            song_count: tracks.len() as u32,
            duration: 0, // TODO: Calculate from tracks
            year: db_album.year,
            genre: None, // TODO: Add genre support
            cover_art: db_album.cover_art_url,
        });
    }

    Ok(AlbumListResponse {
        album_list: AlbumList { album: albums },
    })
}

/// Load album with its songs
async fn load_album_with_songs(
    library_manager: &SharedLibraryManager,
    album_id: &str,
) -> Result<serde_json::Value, LibraryError> {
    let albums = library_manager.get().get_albums().await?;
    let db_album = albums
        .into_iter()
        .find(|a| a.id == album_id)
        .ok_or_else(|| LibraryError::Import("Album not found".to_string()))?;

    let tracks = library_manager.get().get_tracks(album_id).await?;

    let songs: Vec<Song> = tracks
        .into_iter()
        .map(|track| Song {
            id: track.id,
            title: track.title,
            album: db_album.title.clone(),
            artist: track
                .artist_name
                .as_ref()
                .unwrap_or(&db_album.artist_name)
                .clone(),
            album_id: db_album.id.clone(),
            artist_id: format!("artist_{}", db_album.artist_name.replace(" ", "_")),
            track: track.track_number,
            year: db_album.year,
            genre: None,
            cover_art: db_album.cover_art_url.clone(),
            size: None,                             // TODO: Calculate from chunks
            content_type: "audio/flac".to_string(), // TODO: Detect from files
            suffix: "flac".to_string(),
            duration: track.duration_ms.map(|ms| (ms / 1000) as i32),
            bit_rate: None,
            path: format!("{}/{}", db_album.artist_name, db_album.title),
        })
        .collect();

    let album = Album {
        id: db_album.id.clone(),
        name: db_album.title,
        artist: db_album.artist_name.clone(),
        artist_id: format!("artist_{}", db_album.artist_name.replace(" ", "_")),
        song_count: songs.len() as u32,
        duration: songs.iter().map(|s| s.duration.unwrap_or(0) as u32).sum(),
        year: db_album.year,
        genre: None,
        cover_art: db_album.cover_art_url,
    };

    Ok(serde_json::json!({
        "album": {
            "id": album.id,
            "name": album.name,
            "artist": album.artist,
            "artistId": album.artist_id,
            "songCount": album.song_count,
            "duration": album.duration,
            "year": album.year,
            "coverArt": album.cover_art,
            "song": songs
        }
    }))
}

/// Stream track chunks - reassemble encrypted chunks into audio data
/// Optimized for CUE/FLAC tracks with chunk range queries and header prepending
async fn stream_track_chunks(
    state: &SubsonicState,
    track_id: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let library_manager = &state.library_manager;
    println!("Starting chunk reassembly for track: {}", track_id);

    // Check if this is a CUE/FLAC track with track positions
    if let Some(track_position) = library_manager
        .get()
        .get_track_position(track_id)
        .await
        .map_err(|e| format!("Database error: {}", e))?
    {
        println!("Detected CUE/FLAC track - using efficient chunk range streaming");
        return stream_cue_track_chunks(state, track_id, &track_position).await;
    }

    // Fallback to regular file streaming for individual tracks
    println!("Using regular file streaming");

    // Get files for this track
    let files = library_manager.get().get_files_for_track(track_id).await?;
    if files.is_empty() {
        return Err("No files found for track".into());
    }

    // For now, just handle the first file (most tracks have one file)
    let file = &files[0];
    println!(
        "Processing file: {} ({} bytes)",
        file.original_filename, file.file_size
    );

    // Get chunks for this file
    let chunks = library_manager.get().get_chunks_for_file(&file.id).await?;
    if chunks.is_empty() {
        return Err("No chunks found for file".into());
    }

    println!("Found {} chunks to reassemble", chunks.len());

    // Sort chunks by index to ensure correct order
    let mut sorted_chunks = chunks;
    sorted_chunks.sort_by_key(|c| c.chunk_index);

    // Reassemble chunks into audio data
    let mut audio_data = Vec::new();

    for chunk in sorted_chunks {
        println!(
            "Processing chunk {} (index {})",
            chunk.id, chunk.chunk_index
        );

        // Download and decrypt chunk (with caching)
        let chunk_data = download_and_decrypt_chunk(state, &chunk).await?;
        audio_data.extend_from_slice(&chunk_data);
    }

    println!(
        "Successfully reassembled {} bytes of audio data",
        audio_data.len()
    );
    Ok(audio_data)
}

/// Download and decrypt a single chunk with caching
async fn download_and_decrypt_chunk(
    state: &SubsonicState,
    chunk: &crate::database::DbChunk,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let cache_manager = &state.cache_manager;

    // Check cache first (for both local and cloud chunks)
    if let Some(cached_encrypted_data) = cache_manager
        .get_chunk(&chunk.id)
        .await
        .map_err(|e| format!("Cache error: {}", e))?
    {
        // Cache hit - decrypt and return (using injected encryption service)
        let decrypted_data = state
            .encryption_service
            .decrypt_chunk(&cached_encrypted_data)
            .map_err(|e| format!("Failed to decrypt cached chunk: {}", e))?;

        return Ok(decrypted_data);
    }

    // Cache miss - need to download
    let encrypted_data = if chunk.is_local {
        // Read from local storage (legacy support)
        let local_path = chunk
            .storage_location
            .strip_prefix("local:")
            .ok_or("Invalid local storage location")?;

        println!("Reading chunk from local path: {}", local_path);
        tokio::fs::read(local_path).await?
    } else {
        // Download from cloud storage (using injected cloud storage manager)
        println!("Downloading chunk from cloud: {}", chunk.storage_location);

        let cloud_storage = state.cloud_storage.as_ref().ok_or_else(|| {
            "Cloud storage not configured. Please configure S3 settings in the app.".to_string()
        })?;

        // Download encrypted chunk data
        cloud_storage
            .download_chunk(&chunk.storage_location)
            .await
            .map_err(|e| format!("Failed to download chunk: {}", e))?
    };

    // Store in cache for future requests
    if let Err(e) = cache_manager.put_chunk(&chunk.id, &encrypted_data).await {
        println!("Warning: Failed to cache chunk {}: {}", chunk.id, e);
    }

    // Decrypt and return (using injected encryption service)
    let decrypted_data = state
        .encryption_service
        .decrypt_chunk(&encrypted_data)
        .map_err(|e| format!("Failed to decrypt chunk: {}", e))?;

    Ok(decrypted_data)
}

/// Stream CUE/FLAC track chunks efficiently using chunk ranges and header prepending
/// This provides 85% download reduction compared to downloading entire files
async fn stream_cue_track_chunks(
    state: &SubsonicState,
    track_id: &str,
    track_position: &crate::database::DbTrackPosition,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let library_manager = &state.library_manager;
    println!(
        "Streaming CUE/FLAC track: chunks {}-{}",
        track_position.start_chunk_index, track_position.end_chunk_index
    );

    // Get the file for this track
    let files = library_manager.get().get_files_for_track(track_id).await?;
    if files.is_empty() {
        return Err("No files found for CUE track".into());
    }

    let file = &files[0];

    // Check if this file has FLAC headers stored in database
    if !file.has_cue_sheet {
        return Err("File is not marked as CUE/FLAC".into());
    }

    let flac_headers = file
        .flac_headers
        .as_ref()
        .ok_or("No FLAC headers found in database")?;

    println!("Using stored FLAC headers: {} bytes", flac_headers.len());

    // Get the album_id for this track
    let album_id = library_manager
        .get()
        .get_album_id_for_track(track_id)
        .await
        .map_err(|e| format!("Failed to get album ID: {}", e))?;

    // Get only the chunks we need for this track (efficient!)
    let chunk_range = track_position.start_chunk_index..=track_position.end_chunk_index;
    let chunks = library_manager
        .get()
        .get_chunks_in_range(&album_id, chunk_range)
        .await
        .map_err(|e| format!("Failed to get chunk range: {}", e))?;

    if chunks.is_empty() {
        return Err("No chunks found in track range".into());
    }

    println!(
        "Downloading {} chunks instead of {} total chunks ({}% reduction)",
        chunks.len(),
        file.file_size / (1024 * 1024), // Approximate total chunks
        100 - (chunks.len() * 100) / (file.file_size / (1024 * 1024)) as usize
    );

    // Sort chunks by index to ensure correct order
    let mut sorted_chunks = chunks;
    sorted_chunks.sort_by_key(|c| c.chunk_index);

    // Store chunk count before moving
    let chunk_count = sorted_chunks.len();

    // Start with FLAC headers for instant playback
    let mut audio_data = flac_headers.clone();

    // Append track chunks
    for chunk in sorted_chunks {
        println!(
            "Processing track chunk {} (index {})",
            chunk.id, chunk.chunk_index
        );

        // Download and decrypt chunk (with caching)
        let chunk_data = download_and_decrypt_chunk(state, &chunk).await?;
        audio_data.extend_from_slice(&chunk_data);
    }

    println!(
        "Successfully assembled CUE track: {} bytes (headers + {} chunks)",
        audio_data.len(),
        chunk_count
    );

    // Phase 4: Extract precise track boundaries using audio processing
    println!(
        "Extracting precise track boundaries: {}ms to {}ms",
        track_position.start_time_ms, track_position.end_time_ms
    );

    let precise_audio = crate::audio_processing::AudioProcessor::extract_track_from_flac(
        &audio_data,
        track_position.start_time_ms as u64,
        track_position.end_time_ms as u64,
    )
    .map_err(|e| format!("Precise track extraction failed: {}", e))?;

    println!(
        "Successfully extracted precise track: {} bytes",
        precise_audio.len()
    );
    Ok(precise_audio)
}
