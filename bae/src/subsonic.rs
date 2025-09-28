use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use crate::library::{LibraryManager, LibraryError};
use crate::database::{DbAlbum, DbTrack};

/// Subsonic API server state
#[derive(Clone)]
pub struct SubsonicState {
    pub library_manager: Arc<LibraryManager>,
}

/// Common query parameters for Subsonic API
#[derive(Debug, Deserialize)]
pub struct SubsonicQuery {
    pub u: Option<String>,        // username
    pub p: Option<String>,        // password (deprecated)
    pub t: Option<String>,        // token
    pub s: Option<String>,        // salt
    pub v: Option<String>,        // client version
    pub c: Option<String>,        // client name
    pub f: Option<String>,        // format (json/xml)
}

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

/// System info response
#[derive(Debug, Serialize)]
pub struct SystemInfo {
    #[serde(rename = "type")]
    pub server_type: String,
    pub version: String,
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

impl SubsonicState {
    pub fn new(library_manager: LibraryManager) -> Self {
        Self {
            library_manager: Arc::new(library_manager),
        }
    }
}

/// Create the Subsonic API router
pub fn create_router(state: SubsonicState) -> Router {
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
async fn ping(Query(params): Query<SubsonicQuery>) -> impl IntoResponse {
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
async fn get_license(Query(params): Query<SubsonicQuery>) -> impl IntoResponse {
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
async fn get_artists(
    Query(params): Query<SubsonicQuery>,
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
async fn get_album_list(
    Query(params): Query<SubsonicQuery>,
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
    Query(mut params): Query<HashMap<String, String>>,
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

/// Stream a song (placeholder - will implement chunk reassembly)
async fn stream_song(
    Query(mut params): Query<HashMap<String, String>>,
    State(state): State<SubsonicState>,
) -> impl IntoResponse {
    let song_id = match params.get("id") {
        Some(id) => id.clone(),
        None => {
            return (StatusCode::BAD_REQUEST, "Missing song ID").into_response();
        }
    };

    // TODO: Implement actual streaming with chunk reassembly
    // For now, return a placeholder response
    (StatusCode::NOT_IMPLEMENTED, "Streaming not yet implemented").into_response()
}

/// Load artists from database and group by first letter
async fn load_artists(library_manager: &LibraryManager) -> Result<ArtistsResponse, LibraryError> {
    let albums = library_manager.get_albums().await?;
    
    // Group artists by first letter
    let mut artist_map: HashMap<String, HashMap<String, u32>> = HashMap::new();
    
    for album in albums {
        let first_letter = album.artist_name.chars().next()
            .unwrap_or('A')
            .to_uppercase()
            .to_string();
        
        let artists = artist_map.entry(first_letter).or_insert_with(HashMap::new);
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
async fn load_albums(library_manager: &LibraryManager) -> Result<AlbumListResponse, LibraryError> {
    let db_albums = library_manager.get_albums().await?;
    
    let mut albums = Vec::new();
    for db_album in db_albums {
        let tracks = library_manager.get_tracks(&db_album.id).await?;
        
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
    library_manager: &LibraryManager,
    album_id: &str,
) -> Result<serde_json::Value, LibraryError> {
    let albums = library_manager.get_albums().await?;
    let db_album = albums
        .into_iter()
        .find(|a| a.id == album_id)
        .ok_or_else(|| LibraryError::Import("Album not found".to_string()))?;
    
    let tracks = library_manager.get_tracks(album_id).await?;
    
    let songs: Vec<Song> = tracks
        .into_iter()
        .map(|track| Song {
            id: track.id,
            title: track.title,
            album: db_album.title.clone(),
            artist: track.artist_name.as_ref().unwrap_or(&db_album.artist_name).clone(),
            album_id: db_album.id.clone(),
            artist_id: format!("artist_{}", db_album.artist_name.replace(" ", "_")),
            track: track.track_number,
            year: db_album.year,
            genre: None,
            cover_art: db_album.cover_art_url.clone(),
            size: None, // TODO: Calculate from chunks
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
