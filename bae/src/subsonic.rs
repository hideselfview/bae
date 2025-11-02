use crate::library::LibraryError;
use crate::library::SharedLibraryManager;
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
use tracing::{debug, error, info, warn};

/// Subsonic API server state
#[derive(Clone)]
pub struct SubsonicState {
    pub library_manager: SharedLibraryManager,
    pub cache_manager: crate::cache::CacheManager,
    pub encryption_service: crate::encryption::EncryptionService,
    pub cloud_storage: crate::cloud_storage::CloudStorageManager,
    pub chunk_size_bytes: usize,
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
    cloud_storage: crate::cloud_storage::CloudStorageManager,
    chunk_size_bytes: usize,
) -> Router {
    let state = SubsonicState {
        library_manager,
        cache_manager,
        encryption_service,
        cloud_storage,
        chunk_size_bytes,
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

    info!("Streaming request for song ID: {}", song_id);

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
            error!("Streaming error for song {}: {}", song_id, e);
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

    // Group artists by first letter, counting album appearances
    let mut artist_map: HashMap<String, HashMap<String, u32>> = HashMap::new();

    for album in &albums {
        // Get artists for this album
        let artists = library_manager
            .get()
            .get_artists_for_album(&album.id)
            .await?;

        for artist in artists {
            let first_letter = artist
                .name
                .chars()
                .next()
                .unwrap_or('A')
                .to_uppercase()
                .to_string();

            let artist_map_entry = artist_map.entry(first_letter).or_default();
            *artist_map_entry.entry(artist.name).or_insert(0) += 1;
        }
    }

    let mut indices = Vec::new();
    for (letter, artists) in artist_map {
        let artist_list: Vec<Artist> = artists
            .into_iter()
            .map(|(name, count)| Artist {
                id: format!("artist_{}", name.replace(' ', "_")),
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

        // Get artists for this album
        let artists = library_manager
            .get()
            .get_artists_for_album(&db_album.id)
            .await?;
        let artist_name = if artists.is_empty() {
            "Unknown Artist".to_string()
        } else {
            artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };

        albums.push(Album {
            id: db_album.id.clone(),
            name: db_album.title,
            artist: artist_name.clone(),
            artist_id: format!("artist_{}", artist_name.replace(' ', "_")),
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

    // Get album artists
    let album_artists = library_manager
        .get()
        .get_artists_for_album(&db_album.id)
        .await?;
    let album_artist_name = if album_artists.is_empty() {
        "Unknown Artist".to_string()
    } else {
        album_artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };

    let mut songs = Vec::new();
    for track in tracks {
        // Get track artists (for compilations/features)
        let track_artists = library_manager
            .get()
            .get_artists_for_track(&track.id)
            .await?;
        let track_artist_name = if track_artists.is_empty() {
            album_artist_name.clone()
        } else {
            track_artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };

        songs.push(Song {
            id: track.id,
            title: track.title,
            album: db_album.title.clone(),
            artist: track_artist_name.clone(),
            album_id: db_album.id.clone(),
            artist_id: format!("artist_{}", track_artist_name.replace(' ', "_")),
            track: track.track_number,
            year: db_album.year,
            genre: None,
            cover_art: db_album.cover_art_url.clone(),
            size: None,                             // TODO: Calculate from chunks
            content_type: "audio/flac".to_string(), // TODO: Detect from files
            suffix: "flac".to_string(),
            duration: track.duration_ms.map(|ms| (ms / 1000) as i32),
            bit_rate: None,
            path: format!("{}/{}", album_artist_name, db_album.title),
        });
    }

    let album = Album {
        id: db_album.id.clone(),
        name: db_album.title,
        artist: album_artist_name.clone(),
        artist_id: format!("artist_{}", album_artist_name.replace(' ', "_")),
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
    info!("Starting chunk reassembly for track: {}", track_id);

    // Get track chunk coordinates (has all location info)
    let coords = library_manager
        .get()
        .get_track_chunk_coords(track_id)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("No chunk coordinates found for track {}", track_id))?;

    // Get audio format (has FLAC headers if needed)
    let audio_format = library_manager
        .get()
        .get_audio_format_by_track_id(track_id)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("No audio format found for track {}", track_id))?;

    // Get track to find release_id
    let track = library_manager
        .get()
        .get_track(track_id)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("Track not found: {}", track_id))?;

    // Get chunks in range
    let release_chunks = library_manager
        .get()
        .get_chunks_for_release(&track.release_id)
        .await
        .map_err(|e| format!("Database error: {}", e))?;

    let mut chunks: Vec<_> = release_chunks
        .into_iter()
        .filter(|c| {
            c.chunk_index >= coords.start_chunk_index && c.chunk_index <= coords.end_chunk_index
        })
        .collect();

    if chunks.is_empty() {
        return Err("No chunks found for track".into());
    }

    debug!("Found {} chunks to reassemble", chunks.len());

    // Sort chunks by index to ensure correct order
    chunks.sort_by_key(|c| c.chunk_index);

    // Download and decrypt chunks in parallel
    let mut chunk_data_vec: Vec<Vec<u8>> = Vec::new();
    for chunk in chunks {
        debug!(
            "Processing chunk {} (index {})",
            chunk.id, chunk.chunk_index
        );

        // Download and decrypt chunk (with caching)
        let chunk_data = download_and_decrypt_chunk(state, &chunk).await?;
        chunk_data_vec.push(chunk_data);
    }

    // Extract byte ranges from chunks
    let chunk_size = state.chunk_size_bytes;
    let mut audio_data = extract_bytes_from_chunks(
        &chunk_data_vec,
        coords.start_byte_offset,
        coords.end_byte_offset,
        chunk_size,
    );

    // Prepend FLAC headers if needed (CUE/FLAC tracks)
    if audio_format.needs_headers {
        if let Some(ref headers) = audio_format.flac_headers {
            debug!("Prepending FLAC headers: {} bytes", headers.len());
            let mut complete_audio = headers.clone();
            complete_audio.extend_from_slice(&audio_data);
            audio_data = complete_audio;
        }
    }

    info!(
        "Successfully reassembled {} bytes of audio data",
        audio_data.len()
    );
    Ok(audio_data)
}

/// Download and decrypt a single chunk with caching
async fn download_and_decrypt_chunk(
    state: &SubsonicState,
    chunk: &crate::db::DbChunk,
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

    // Cache miss - download from cloud storage
    debug!("Downloading chunk from cloud: {}", chunk.storage_location);

    let encrypted_data = state
        .cloud_storage
        .download_chunk(&chunk.storage_location)
        .await
        .map_err(|e| format!("Failed to download chunk: {}", e))?;

    // Store in cache for future requests
    if let Err(e) = cache_manager.put_chunk(&chunk.id, &encrypted_data).await {
        warn!("Failed to cache chunk {}: {}", chunk.id, e);
    }

    // Decrypt and return (using injected encryption service)
    let decrypted_data = state
        .encryption_service
        .decrypt_chunk(&encrypted_data)
        .map_err(|e| format!("Failed to decrypt chunk: {}", e))?;

    Ok(decrypted_data)
}

/// Extract bytes from chunks using byte offsets
fn extract_bytes_from_chunks(
    chunks: &[Vec<u8>],
    start_byte_offset: i64,
    end_byte_offset: i64,
    _chunk_size: usize,
) -> Vec<u8> {
    if chunks.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();

    if chunks.len() == 1 {
        // Track is entirely within a single chunk
        let start = start_byte_offset as usize;
        let end = (end_byte_offset + 1) as usize; // end_byte_offset is inclusive
        result.extend_from_slice(&chunks[0][start..end]);
    } else {
        // Track spans multiple chunks
        // First chunk: from start_byte_offset to end of chunk
        let first_chunk_start = start_byte_offset as usize;
        result.extend_from_slice(&chunks[0][first_chunk_start..]);

        // Middle chunks: use entirely
        for chunk in &chunks[1..chunks.len() - 1] {
            result.extend_from_slice(chunk);
        }

        // Last chunk: from start to end_byte_offset
        let last_chunk_end = (end_byte_offset + 1) as usize; // end_byte_offset is inclusive
        result.extend_from_slice(&chunks[chunks.len() - 1][0..last_chunk_end]);
    }

    result
}
