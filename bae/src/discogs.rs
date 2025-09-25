use crate::models::{DiscogsRelease, DiscogsTrack};
use reqwest::{Client, Error as ReqwestError};
use serde::Deserialize;
use std::collections::HashMap;
use thiserror::Error;

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
    results: Vec<SearchResult>,
    pagination: Pagination,
}

/// Individual search result
#[derive(Debug, Deserialize)]
struct SearchResult {
    id: u64,
    title: String,
    year: Option<String>,
    genre: Option<Vec<String>>,
    style: Option<Vec<String>>,
    format: Option<Vec<String>>,
    country: Option<String>,
    label: Option<Vec<String>>,
    cover_image: Option<String>,
    thumb: Option<String>,
    master_id: Option<u64>,
    #[serde(rename = "type")]
    result_type: String,
}

/// Discogs API pagination info
#[derive(Debug, Deserialize)]
struct Pagination {
    pages: u32,
    page: u32,
    per_page: u32,
    items: u32,
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
    tracklist: Option<Vec<TrackResponse>>,
    artists: Option<Vec<ArtistResponse>>,
    master_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Format {
    name: String,
    qty: String,
    descriptions: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct Image {
    #[serde(rename = "type")]
    image_type: String,
    uri: String,
    uri150: String,
}

#[derive(Debug, Deserialize)]
struct TrackResponse {
    position: String,
    title: String,
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArtistResponse {
    name: String,
    id: u64,
}

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
    pub async fn search_masters(&self, query: &str, format: &str) -> Result<Vec<DiscogsRelease>, DiscogsError> {
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
                .map(|r| DiscogsRelease {
                    id: r.id.to_string(),
                    title: r.title,
                    year: r.year.and_then(|y| y.parse().ok()),
                    genre: r.genre.unwrap_or_default(),
                    style: r.style.unwrap_or_default(),
                    format: r.format.unwrap_or_default(),
                    country: r.country,
                    label: r.label.unwrap_or_default(),
                    cover_image: r.cover_image,
                    thumb: r.thumb,
                    tracklist: Vec::new(), // Will be populated when getting release details
                    master_id: None, // This is a master, so no master_id
                })
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

    /// Search for releases by master ID
    pub async fn search_releases_for_master(&self, master_id: &str, format: &str) -> Result<Vec<DiscogsRelease>, DiscogsError> {
        let url = format!("{}/database/search", self.base_url);
        
        let mut params = HashMap::new();
        params.insert("master_id", master_id);
        params.insert("type", "release");
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
                .filter(|r| r.result_type == "release")
                .map(|r| DiscogsRelease {
                    id: r.id.to_string(),
                    title: r.title,
                    year: r.year.and_then(|y| y.parse().ok()),
                    genre: r.genre.unwrap_or_default(),
                    style: r.style.unwrap_or_default(),
                    format: r.format.unwrap_or_default(),
                    country: r.country,
                    label: r.label.unwrap_or_default(),
                    cover_image: r.cover_image,
                    thumb: r.thumb,
                    tracklist: Vec::new(), // Will be populated when getting release details
                    master_id: r.master_id.map(|id| id.to_string()), // Use master_id from search result
                })
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

            let cover_image = release
                .images
                .as_ref()
                .and_then(|images| {
                    images
                        .iter()
                        .find(|img| img.image_type == "primary")
                        .or_else(|| images.first())
                        .map(|img| img.uri.clone())
                });

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
                thumb: None, // Not available in detailed release
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_discogs_client_creation() {
        let client = DiscogsClient::new("test_key".to_string());
        assert_eq!(client.api_key, "test_key");
        assert_eq!(client.base_url, "https://api.discogs.com");
    }

    // Note: These tests would require a real API key and network access
    // In a real implementation, you'd want to use mocked responses
}
