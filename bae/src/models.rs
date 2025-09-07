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
    pub cover_image: Option<String>,
    pub thumb: Option<String>,
    pub tracklist: Vec<DiscogsTrack>,
}

/// Represents a track from Discogs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscogsTrack {
    pub position: String,
    pub title: String,
    pub duration: Option<String>, // Duration as string from Discogs (e.g., "3:45")
}

impl DiscogsTrack {
    /// Parse duration string from Discogs format (e.g., "3:45") to Duration
    pub fn parse_duration(&self) -> Option<Duration> {
        self.duration.as_ref().and_then(|d| {
            let parts: Vec<&str> = d.split(':').collect();
            match parts.len() {
                2 => {
                    let minutes = parts[0].parse::<u64>().ok()?;
                    let seconds = parts[1].parse::<u64>().ok()?;
                    Some(Duration::from_secs(minutes * 60 + seconds))
                }
                3 => {
                    let hours = parts[0].parse::<u64>().ok()?;
                    let minutes = parts[1].parse::<u64>().ok()?;
                    let seconds = parts[2].parse::<u64>().ok()?;
                    Some(Duration::from_secs(hours * 3600 + minutes * 60 + seconds))
                }
                _ => None,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discogs_track_duration_parsing() {
        let track = DiscogsTrack {
            position: "1".to_string(),
            title: "Test Track".to_string(),
            duration: Some("3:45".to_string()),
        };
        
        let duration = track.parse_duration().unwrap();
        assert_eq!(duration.as_secs(), 3 * 60 + 45);
    }

    #[test]
    fn test_discogs_track_duration_with_hours() {
        let track = DiscogsTrack {
            position: "1".to_string(),
            title: "Long Track".to_string(),
            duration: Some("1:02:30".to_string()),
        };
        
        let duration = track.parse_duration().unwrap();
        assert_eq!(duration.as_secs(), 1 * 3600 + 2 * 60 + 30);
    }

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
