use serde::{Deserialize, Serialize};

#[cfg(test)]
use std::time::Duration;

/// Represents an artist in the music library
#[cfg(test)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub bio: Option<String>,
    pub image_url: Option<String>,
}

/// Represents a single track
#[cfg(test)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub duration: Duration,
    pub track_number: Option<u32>,
    pub artist: Option<String>, // Artist name, can differ from album artist
}

/// Represents an album in the music library
#[cfg(test)]
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

impl DiscogsTrack {
    /// Parse track number from Discogs position string.
    ///
    /// Discogs uses inconsistent position formats ("1", "A1", "1-1", etc).
    /// We extract numeric characters and parse them.
    /// Returns error if no valid number can be extracted.
    pub fn parse_track_number(&self) -> Result<i32, String> {
        // Try to extract number from position string
        let numbers: String = self.position.chars().filter(|c| c.is_numeric()).collect();

        if numbers.is_empty() {
            return Err(format!(
                "No numeric characters in track position: '{}'",
                self.position
            ));
        }

        numbers.parse::<i32>().map_err(|e| {
            format!(
                "Failed to parse track number from '{}': {}",
                self.position, e
            )
        })
    }
}

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
