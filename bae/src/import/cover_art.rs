use crate::db::ImageSource;
use crate::discogs::client::DiscogsClient;
use crate::musicbrainz::{ExternalUrls, MbRelease};
use crate::network::upgrade_to_https;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Fetch cover art URL from Cover Art Archive for a MusicBrainz release
pub async fn fetch_cover_art_from_archive(release_id: &str) -> Option<String> {
    // Try JSON endpoint first to get image metadata
    let json_url = format!("https://coverartarchive.org/release/{}", release_id);

    debug!("Fetching cover art from Cover Art Archive: {}", json_url);

    let client = match reqwest::Client::builder()
        .user_agent("bae/1.0 +https://github.com/hideselfview/bae")
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            warn!("Failed to create HTTP client for Cover Art Archive: {}", e);
            return None;
        }
    };

    match client.get(&json_url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                if let Ok(json) = response.json::<serde_json::Value>().await {
                    if let Some(images) = json.get("images").and_then(|i| i.as_array()) {
                        // Find front cover image
                        for image in images {
                            if let Some(front) = image.get("front").and_then(|f| f.as_bool()) {
                                if front {
                                    if let Some(image_url) =
                                        image.get("image").and_then(|i| i.as_str())
                                    {
                                        debug!("Found front cover art: {}", image_url);
                                        let secure_url = upgrade_to_https(image_url);
                                        return Some(secure_url);
                                    }
                                    // Try thumbnails if full image not available
                                    if let Some(thumb_url) =
                                        image.get("thumb").and_then(|t| t.as_str())
                                    {
                                        debug!("Using thumbnail: {}", thumb_url);
                                        return Some(upgrade_to_https(thumb_url));
                                    }
                                    if let Some(small_url) =
                                        image.get("small").and_then(|s| s.as_str())
                                    {
                                        debug!("Using small image: {}", small_url);
                                        return Some(upgrade_to_https(small_url));
                                    }
                                }
                            }
                        }
                        // If no front cover found, use first image
                        if let Some(first_image) = images.first() {
                            if let Some(image_url) =
                                first_image.get("image").and_then(|i| i.as_str())
                            {
                                debug!("Using first available cover art: {}", image_url);
                                return Some(upgrade_to_https(image_url));
                            }
                            if let Some(thumb_url) =
                                first_image.get("thumb").and_then(|t| t.as_str())
                            {
                                debug!("Using first thumbnail: {}", thumb_url);
                                return Some(upgrade_to_https(thumb_url));
                            }
                        }
                    }
                }
            } else if response.status() == 404 {
                debug!(
                    "No cover art found in Cover Art Archive for release {}",
                    release_id
                );
            } else {
                debug!(
                    "Cover Art Archive returned status {} for release {}",
                    response.status(),
                    release_id
                );
            }
        }
        Err(e) => {
            debug!("Failed to fetch cover art from Cover Art Archive: {}", e);
        }
    }

    None
}

/// Fetch cover art URL from Discogs release (fallback)
pub async fn fetch_cover_art_from_discogs(
    discogs_client: &DiscogsClient,
    external_urls: &ExternalUrls,
) -> Option<String> {
    // Try discogs_release_url first, then discogs_master_url
    let discogs_url = external_urls
        .discogs_release_url
        .as_ref()
        .or_else(|| external_urls.discogs_master_url.as_ref())?;

    // Extract release ID from URL (format: https://www.discogs.com/release/123456 or https://www.discogs.com/master/123456)
    let release_id = discogs_url.split('/').last()?;

    debug!("Fetching cover art from Discogs release ID: {}", release_id);

    match discogs_client.get_release(release_id).await {
        Ok(release) => release
            .cover_image
            .or_else(|| release.thumb)
            .map(|url| upgrade_to_https(&url)),
        Err(e) => {
            debug!("Failed to fetch Discogs release {}: {}", release_id, e);
            None
        }
    }
}

/// Fetch cover art for a MusicBrainz release with fallback to Discogs
pub async fn fetch_cover_art_for_mb_release(
    mb_release: &MbRelease,
    external_urls: &ExternalUrls,
    discogs_client: Option<&DiscogsClient>,
) -> Option<String> {
    // First try Cover Art Archive
    if let Some(url) = fetch_cover_art_from_archive(&mb_release.release_id).await {
        return Some(url);
    }

    // Fallback to Discogs if we have a client and URLs
    if let Some(client) = discogs_client {
        if external_urls.discogs_release_url.is_some() || external_urls.discogs_master_url.is_some()
        {
            if let Some(url) = fetch_cover_art_from_discogs(client, external_urls).await {
                return Some(url);
            }
        }
    }

    None
}

/// Result of downloading cover art to local storage
#[derive(Debug, Clone)]
pub struct DownloadedCoverArt {
    /// Path to the downloaded file (e.g., "/path/to/album/.bae/cover-mb.jpg")
    pub path: PathBuf,
    /// Source of the cover art
    pub source: ImageSource,
}

/// Download cover art from a URL to the .bae/ folder in the release directory.
///
/// Creates the .bae/ directory if it doesn't exist.
/// Returns the path to the downloaded file and its source.
pub async fn download_cover_art_to_bae_folder(
    cover_art_url: &str,
    release_folder: &Path,
    source: ImageSource,
) -> Result<DownloadedCoverArt, String> {
    // Create .bae directory
    let bae_dir = release_folder.join(".bae");
    tokio::fs::create_dir_all(&bae_dir)
        .await
        .map_err(|e| format!("Failed to create .bae directory: {}", e))?;

    // Determine filename based on source and URL extension
    let extension = cover_art_url
        .split('.')
        .last()
        .and_then(|ext| {
            let ext_lower = ext.to_lowercase();
            // Only accept common image extensions
            if ["jpg", "jpeg", "png", "gif", "webp"].contains(&ext_lower.as_str()) {
                Some(ext_lower)
            } else {
                None
            }
        })
        .unwrap_or_else(|| "jpg".to_string());

    let filename = match source {
        ImageSource::MusicBrainz => format!("cover-mb.{}", extension),
        ImageSource::Discogs => format!("cover-discogs.{}", extension),
        ImageSource::Local => format!("cover.{}", extension),
    };

    let file_path = bae_dir.join(&filename);

    // Download the image
    info!(
        "Downloading cover art from {} to {:?}",
        cover_art_url, file_path
    );

    let client = reqwest::Client::builder()
        .user_agent("bae/1.0 +https://github.com/hideselfview/bae")
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(cover_art_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch cover art: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Cover art download failed with status {}",
            response.status()
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read cover art response: {}", e))?;

    // Validate it's actually an image (basic check)
    if bytes.len() < 100 {
        return Err("Downloaded file too small to be a valid image".to_string());
    }

    tokio::fs::write(&file_path, &bytes)
        .await
        .map_err(|e| format!("Failed to write cover art file: {}", e))?;

    info!(
        "Downloaded cover art ({} bytes) to {:?}",
        bytes.len(),
        file_path
    );

    Ok(DownloadedCoverArt {
        path: file_path,
        source,
    })
}
