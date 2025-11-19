use crate::discogs::client::DiscogsClient;
use crate::musicbrainz::{ExternalUrls, MbRelease};
use tracing::{debug, warn};

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
                                        return Some(image_url.to_string());
                                    }
                                    // Try thumbnails if full image not available
                                    if let Some(thumb_url) =
                                        image.get("thumb").and_then(|t| t.as_str())
                                    {
                                        debug!("Using thumbnail: {}", thumb_url);
                                        return Some(thumb_url.to_string());
                                    }
                                    if let Some(small_url) =
                                        image.get("small").and_then(|s| s.as_str())
                                    {
                                        debug!("Using small image: {}", small_url);
                                        return Some(small_url.to_string());
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
                                return Some(image_url.to_string());
                            }
                            if let Some(thumb_url) =
                                first_image.get("thumb").and_then(|t| t.as_str())
                            {
                                debug!("Using first thumbnail: {}", thumb_url);
                                return Some(thumb_url.to_string());
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
        Ok(release) => release.cover_image.or_else(|| release.thumb),
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
