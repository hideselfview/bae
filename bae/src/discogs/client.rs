use crate::discogs::models::{
    DiscogsArtist, DiscogsMaster, DiscogsMasterReleaseVersion, DiscogsRelease, DiscogsTrack,
    SortOrder,
};
use reqwest::{Client, Error as ReqwestError};
use serde::Deserialize;
use std::collections::HashMap;
use thiserror::Error;
use tracing::{error, warn};

#[derive(Error, Debug)]
pub enum DiscogsError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] ReqwestError),
    #[error("API rate limit exceeded")]
    RateLimit,
    #[error("Invalid API key")]
    InvalidApiKey,
    #[error("Release not found")]
    NotFound,
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Discogs search response wrapper
#[derive(Debug, Deserialize)]
struct SearchResponse {
    results: Vec<DiscogsSearchResult>,
}

/// Individual search result
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct DiscogsSearchResult {
    pub id: u64,
    pub title: String,
    pub year: Option<String>,
    pub genre: Option<Vec<String>>,
    pub style: Option<Vec<String>>,
    pub format: Option<Vec<String>>,
    pub country: Option<String>,
    pub label: Option<Vec<String>>,
    pub cover_image: Option<String>,
    pub thumb: Option<String>,
    pub master_id: Option<u64>,
    #[serde(rename = "type")]
    pub result_type: String,
}

/// Master versions response wrapper
#[derive(Debug, Deserialize)]
struct MasterVersionsResponse {
    versions: Vec<VersionResponse>,
}

/// Individual version from master versions API
#[derive(Debug, Deserialize)]
struct VersionResponse {
    id: u64,
    title: String,
    format: String,
    label: String,
    catno: String,
    country: String,
    released: Option<String>,
    thumb: Option<String>,
}

/// Artist credit in Discogs API responses
#[derive(Debug, Deserialize, Clone)]
struct ArtistCredit {
    id: u64,
    name: String,
}

/// Master detail response from Discogs
#[derive(Debug, Deserialize)]
struct MasterResponse {
    id: u64,
    title: String,
    year: Option<u32>,
    thumb: Option<String>,
    // TODO is this optional? It might be required.
    artists: Option<Vec<ArtistCredit>>,
    tracklist: Option<Vec<TrackResponse>>,
}

/// Detailed release response from Discogs
#[derive(Debug, Deserialize)]
struct ReleaseResponse {
    id: u64,
    title: String,
    year: Option<u32>,
    genres: Option<Vec<String>>,
    styles: Option<Vec<String>>,
    formats: Option<Vec<Format>>,
    country: Option<String>,
    images: Option<Vec<Image>>,
    artists: Option<Vec<ArtistCredit>>,
    tracklist: Option<Vec<TrackResponse>>,
    master_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Format {
    name: String,
}

#[derive(Debug, Deserialize)]
struct Image {
    #[serde(rename = "type")]
    image_type: String,
    uri: String,
    uri150: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TrackResponse {
    position: String,
    title: String,
    duration: Option<String>,
}

#[derive(Clone)]
pub struct DiscogsClient {
    client: Client,
    api_key: String,
    base_url: String,
}

impl DiscogsClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: "https://api.discogs.com".to_string(),
        }
    }

    /// Search for masters by query string
    pub async fn search_masters(
        &self,
        query: &str,
        format: &str,
    ) -> Result<Vec<DiscogsSearchResult>, DiscogsError> {
        let url = format!("{}/database/search", self.base_url);

        let mut params = HashMap::new();
        params.insert("q", query);
        params.insert("type", "master");
        params.insert("token", &self.api_key);

        if !format.is_empty() {
            params.insert("format", format);
        }

        let response = self
            .client
            .get(&url)
            .query(&params)
            .header("User-Agent", "bae/1.0 +https://github.com/hideselfview/bae")
            .send()
            .await?;

        if response.status().is_success() {
            let search_response: SearchResponse = response.json().await?;

            Ok(search_response
                .results
                .into_iter()
                .filter(|r| r.result_type == "master")
                .collect())
        } else if response.status() == 429 {
            Err(DiscogsError::RateLimit)
        } else if response.status() == 401 {
            Err(DiscogsError::InvalidApiKey)
        } else {
            Err(DiscogsError::Request(
                response.error_for_status().unwrap_err(),
            ))
        }
    }

    /// Get detailed information about a master release
    pub async fn get_master(&self, master_id: &str) -> Result<DiscogsMaster, DiscogsError> {
        let url = format!("{}/masters/{}", self.base_url, master_id);

        let mut params = HashMap::new();
        params.insert("token", &self.api_key);

        let response = self
            .client
            .get(&url)
            .query(&params)
            .header("User-Agent", "bae/1.0 +https://github.com/hideselfview/bae")
            .send()
            .await?;

        if response.status().is_success() {
            let master: MasterResponse = response.json().await?;

            let tracklist = master
                .tracklist
                .unwrap_or_default()
                .into_iter()
                .map(|t| DiscogsTrack {
                    position: t.position,
                    title: t.title,
                    duration: t.duration,
                })
                .collect();

            let artists = master
                .artists
                .unwrap_or_default()
                .into_iter()
                .map(|a| DiscogsArtist {
                    id: a.id.to_string(),
                    name: a.name,
                })
                .collect();

            // Masters don't have direct label field, use empty default
            // TODO fix this
            let label = Vec::new();

            let discogs_master = DiscogsMaster {
                id: master.id.to_string(),
                title: master.title,
                year: master.year,
                thumb: master.thumb,
                label,
                country: None, // Masters don't have country info
                artists,
                tracklist,
            };

            Ok(discogs_master)
        } else if response.status() == 404 {
            Err(DiscogsError::NotFound)
        } else if response.status() == 429 {
            Err(DiscogsError::RateLimit)
        } else if response.status() == 401 {
            Err(DiscogsError::InvalidApiKey)
        } else {
            Err(DiscogsError::Request(
                response.error_for_status().unwrap_err(),
            ))
        }
    }

    /// Get versions of a master release
    pub async fn get_master_releases(
        &self,
        master_id: &str,
        sort_order: Option<SortOrder>,
    ) -> Result<Vec<DiscogsMasterReleaseVersion>, DiscogsError> {
        let url = format!("{}/masters/{}/versions", self.base_url, master_id);

        let sort_order = match sort_order.unwrap_or(SortOrder::Ascending) {
            SortOrder::Ascending => String::from("asc"),
            SortOrder::Descending => String::from("desc"),
        };

        let mut params = HashMap::<&str, String>::new();

        params.insert("token", self.api_key.clone());
        params.insert("per_page", String::from("100"));
        params.insert("sort", String::from("released"));
        params.insert("sort_order", sort_order);

        let response = self
            .client
            .get(&url)
            .query(&params)
            .header("User-Agent", "bae/1.0 +https://github.com/hideselfview/bae")
            .send()
            .await?;

        if response.status().is_success() {
            // Get the raw response text first for debugging on error
            let response_text = response.text().await?;

            let versions_response: MasterVersionsResponse = serde_json::from_str(&response_text)
                .map_err(|e| {
                    error!("JSON parsing error for master_id {}: {}", master_id, e);
                    error!("Raw response: {}", response_text);
                    e
                })?;

            Ok(versions_response
                .versions
                .into_iter()
                .map(|v| DiscogsMasterReleaseVersion {
                    id: v.id,
                    title: v.title,
                    format: v.format,
                    label: v.label,
                    catno: v.catno,
                    country: v.country,
                    released: v.released,
                    thumb: v.thumb,
                })
                .collect())
        } else if response.status() == 429 {
            warn!("Rate limit hit for master_id {}", master_id);
            Err(DiscogsError::RateLimit)
        } else if response.status() == 401 {
            error!("Invalid API key for master_id {}", master_id);
            Err(DiscogsError::InvalidApiKey)
        } else if response.status() == 404 {
            warn!("Master not found: {}", master_id);
            Err(DiscogsError::NotFound)
        } else {
            let status = response.status();
            error!("API error for master_id {}: Status {}", master_id, status);
            Err(DiscogsError::Request(
                response.error_for_status().unwrap_err(),
            ))
        }
    }

    /// Get detailed information about a specific release
    pub async fn get_release(&self, id: &str) -> Result<DiscogsRelease, DiscogsError> {
        let url = format!("{}/releases/{}", self.base_url, id);

        let mut params = HashMap::new();
        params.insert("token", &self.api_key);

        let response = self
            .client
            .get(&url)
            .query(&params)
            .header("User-Agent", "bae/1.0 +https://github.com/yourusername/bae")
            .send()
            .await?;

        if response.status().is_success() {
            let release: ReleaseResponse = response.json().await?;

            let tracklist = release
                .tracklist
                .unwrap_or_default()
                .into_iter()
                .map(|t| DiscogsTrack {
                    position: t.position,
                    title: t.title,
                    duration: t.duration,
                })
                .collect();

            let artists = release
                .artists
                .unwrap_or_default()
                .into_iter()
                .map(|a| DiscogsArtist {
                    id: a.id.to_string(),
                    name: a.name,
                })
                .collect();

            let primary_image = release.images.as_ref().and_then(|images| {
                images
                    .iter()
                    .find(|img| img.image_type == "primary")
                    .or_else(|| images.first())
            });

            let cover_image = primary_image.map(|img| img.uri.clone());
            let thumb =
                primary_image.and_then(|img| img.uri150.clone().or_else(|| Some(img.uri.clone())));

            Ok(DiscogsRelease {
                id: release.id.to_string(),
                title: release.title,
                year: release.year,
                genre: release.genres.unwrap_or_default(),
                style: release.styles.unwrap_or_default(),
                format: release
                    .formats
                    .unwrap_or_default()
                    .into_iter()
                    .map(|f| f.name)
                    .collect(),
                country: release.country,
                label: Vec::new(), // Not available in detailed release
                cover_image,
                thumb,
                artists,
                tracklist,
                master_id: release.master_id.map(|id| id.to_string()), // Use master_id from detailed release
            })
        } else if response.status() == 404 {
            Err(DiscogsError::NotFound)
        } else if response.status() == 429 {
            Err(DiscogsError::RateLimit)
        } else if response.status() == 401 {
            Err(DiscogsError::InvalidApiKey)
        } else {
            Err(DiscogsError::Request(
                response.error_for_status().unwrap_err(),
            ))
        }
    }
}
