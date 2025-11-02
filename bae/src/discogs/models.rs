use serde::{Deserialize, Serialize};

/// Sort order for release date
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

/// Artist credit from Discogs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscogsArtist {
    pub id: String,
    pub name: String,
}

/// Represents a Discogs master (full data from master detail API)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscogsMaster {
    pub id: String,
    pub title: String,
    pub year: Option<u32>,
    pub thumb: Option<String>,
    pub label: Vec<String>,
    pub country: Option<String>,
    pub artists: Vec<DiscogsArtist>,
    pub tracklist: Vec<DiscogsTrack>,
    pub main_release: String,
}

/// Represents a Discogs release search result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscogsRelease {
    pub id: String,
    pub title: String,
    pub year: Option<u32>,
    pub genre: Vec<String>,
    pub style: Vec<String>,
    pub format: Vec<String>,
    pub country: Option<String>,
    pub label: Vec<String>,
    pub cover_image: Option<String>,
    pub thumb: Option<String>,
    pub artists: Vec<DiscogsArtist>,
    pub tracklist: Vec<DiscogsTrack>,
    pub master_id: Option<String>, // Reference to the master release
}

/// Represents a release version from master versions API
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscogsMasterReleaseVersion {
    pub id: u64,
    pub title: String,
    pub format: String, // Fixed: format is a string in the API response
    pub label: String,  // Fixed: label is a string in the API response
    pub catno: String,
    pub country: String,
    pub released: Option<String>,
    pub thumb: Option<String>,
}

/// Represents a track from Discogs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscogsTrack {
    pub position: String,
    pub title: String,
    pub duration: Option<String>, // Duration as string from Discogs (e.g., "3:45")
}
