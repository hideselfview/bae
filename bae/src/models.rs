use serde::{Deserialize, Serialize};

/// Represents a Discogs master (full data from master detail API)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscogsMaster {
    pub id: String,
    pub title: String,
    pub year: Option<u32>,
    pub thumb: Option<String>,
    pub label: Vec<String>,
    pub country: Option<String>,
    pub tracklist: Vec<DiscogsTrack>,
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
    pub tracklist: Vec<DiscogsTrack>,
    pub master_id: Option<String>, // Reference to the master release
}

/// Represents an item that can be imported (either a master or specific release)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DiscogsAlbum {
    Master(DiscogsMaster),
    Release(DiscogsRelease),
}

impl DiscogsAlbum {
    pub fn title(&self) -> &str {
        match self {
            DiscogsAlbum::Master(master) => &master.title,
            DiscogsAlbum::Release(release) => &release.title,
        }
    }

    pub fn year(&self) -> Option<u32> {
        match self {
            DiscogsAlbum::Master(master) => master.year,
            DiscogsAlbum::Release(release) => release.year,
        }
    }

    pub fn thumb(&self) -> Option<&String> {
        match self {
            DiscogsAlbum::Master(master) => master.thumb.as_ref(),
            DiscogsAlbum::Release(release) => release.thumb.as_ref(),
        }
    }

    pub fn label(&self) -> &[String] {
        match self {
            DiscogsAlbum::Master(master) => &master.label,
            DiscogsAlbum::Release(release) => &release.label,
        }
    }

    pub fn format(&self) -> &[String] {
        match self {
            DiscogsAlbum::Master(_) => &[],
            DiscogsAlbum::Release(release) => &release.format,
        }
    }

    pub fn is_master(&self) -> bool {
        matches!(self, DiscogsAlbum::Master(_))
    }

    /// Get the tracklist for AI matching
    pub fn tracklist(&self) -> &[DiscogsTrack] {
        match self {
            DiscogsAlbum::Master(master) => &master.tracklist,
            DiscogsAlbum::Release(release) => &release.tracklist,
        }
    }

    /// Extract artist name from album title.
    ///
    /// Discogs album titles often follow "Artist - Album" format.
    /// Splits on " - " to extract the artist. Falls back to "Unknown Artist".
    pub fn extract_artist_name(&self) -> String {
        let title = self.title();
        if let Some(dash_pos) = title.find(" - ") {
            title[..dash_pos].to_string()
        } else {
            "Unknown Artist".to_string()
        }
    }
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
