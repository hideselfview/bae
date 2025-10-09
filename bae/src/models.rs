use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Represents an artist in the music library
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub bio: Option<String>,
    pub image_url: Option<String>,
}

/// Represents a single track
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub duration: Duration,
    pub track_number: Option<u32>,
    pub artist: Option<String>, // Artist name, can differ from album artist
}

/// Represents an album in the music library
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Album {
    pub id: String,
    pub title: String,
    pub artist: Artist,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub cover_art_url: Option<String>,
    pub tracks: Vec<Track>,
}

/// Metadata for importing an album
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumMetadata {
    pub title: String,
    pub artist_name: String,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub discogs_id: Option<String>,
    pub cover_art_url: Option<String>,
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
    pub tracklist: Vec<DiscogsTrack>,
}

/// Represents an item that can be imported (either a master or specific release)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ImportItem {
    Master(DiscogsMaster),
    Release(DiscogsRelease),
}

impl ImportItem {
    pub fn title(&self) -> &str {
        match self {
            ImportItem::Master(master) => &master.title,
            ImportItem::Release(release) => &release.title,
        }
    }

    pub fn year(&self) -> Option<u32> {
        match self {
            ImportItem::Master(master) => master.year,
            ImportItem::Release(release) => release.year,
        }
    }

    pub fn thumb(&self) -> Option<&String> {
        match self {
            ImportItem::Master(master) => master.thumb.as_ref(),
            ImportItem::Release(release) => release.thumb.as_ref(),
        }
    }

    pub fn label(&self) -> &[String] {
        match self {
            ImportItem::Master(master) => &master.label,
            ImportItem::Release(release) => &release.label,
        }
    }

    pub fn format(&self) -> &[String] {
        match self {
            ImportItem::Master(_) => &[],
            ImportItem::Release(release) => &release.format,
        }
    }

    pub fn is_master(&self) -> bool {
        matches!(self, ImportItem::Master(_))
    }

    /// Get the tracklist for AI matching
    pub fn tracklist(&self) -> &[DiscogsTrack] {
        match self {
            ImportItem::Master(master) => &master.tracklist,
            ImportItem::Release(release) => &release.tracklist,
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

/// Represents a track from Discogs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscogsTrack {
    pub position: String,
    pub title: String,
    pub duration: Option<String>, // Duration as string from Discogs (e.g., "3:45")
}

impl DiscogsTrack {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_album_serialization() {
        let artist = Artist {
            id: "artist1".to_string(),
            name: "Test Artist".to_string(),
            bio: None,
            image_url: None,
        };

        let track = Track {
            id: "track1".to_string(),
            title: "Test Track".to_string(),
            duration: Duration::from_secs(225),
            track_number: Some(1),
            artist: None,
        };

        let album = Album {
            id: "album1".to_string(),
            title: "Test Album".to_string(),
            artist,
            year: Some(2023),
            genre: Some("Rock".to_string()),
            cover_art_url: None,
            tracks: vec![track],
        };

        // Test that serialization works
        let json = serde_json::to_string(&album).unwrap();
        let deserialized: Album = serde_json::from_str(&json).unwrap();
        assert_eq!(album, deserialized);
    }
}
