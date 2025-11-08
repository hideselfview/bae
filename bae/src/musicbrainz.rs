use thiserror::Error;
use tracing::{debug, info, warn};

/// MusicBrainz release information
#[derive(Debug, Clone, PartialEq)]
pub struct MbRelease {
    pub release_id: String,
    pub release_group_id: String,
    pub title: String,
    pub artist: String,
    pub date: Option<String>,
    pub format: Option<String>,
    pub country: Option<String>,
    pub label: Option<String>,
    pub catalog_number: Option<String>,
    pub barcode: Option<String>,
}

/// External URLs extracted from MusicBrainz relationships
#[derive(Debug, Clone)]
pub struct ExternalUrls {
    pub discogs_master_url: Option<String>,
    pub discogs_release_url: Option<String>,
    pub bandcamp_url: Option<String>,
}

#[derive(Debug, Error)]
pub enum MusicBrainzError {
    #[error("MusicBrainz API error: {0}")]
    Api(String),
    #[error("No release found for DISCID: {0}")]
    NotFound(String),
}

/// Lookup releases by MusicBrainz DiscID
pub async fn lookup_by_discid(
    discid: &str,
) -> Result<(Vec<MbRelease>, ExternalUrls), MusicBrainzError> {
    info!("🎵 MusicBrainz: Looking up DiscID '{}'", discid);

    // Use musicbrainz_rs to lookup by discid
    // The API endpoint is /ws/2/discid/{discid}
    // Build URL properly to handle special characters in DiscID
    let base_url = reqwest::Url::parse("https://musicbrainz.org/ws/2/discid/")
        .map_err(|e| MusicBrainzError::Api(format!("Failed to parse base URL: {}", e)))?;

    let url = base_url
        .join(discid)
        .map_err(|e| MusicBrainzError::Api(format!("Failed to construct DiscID URL: {}", e)))?;

    let mut url_with_params = url.clone();
    // For discid resource, 'releases' are automatically included, and 'media' is not a valid inc parameter
    // We can get media info from the individual release lookups if needed
    url_with_params.set_query(Some(
        "inc=recordings+artist-credits+release-groups+url-rels+labels",
    ));

    debug!("MusicBrainz API request: {}", url_with_params);

    // Use reqwest to make the request since musicbrainz_rs doesn't have direct discid lookup
    let client = reqwest::Client::builder()
        .user_agent("bae/1.0 +https://github.com/hideselfview/bae")
        .build()
        .map_err(|e| MusicBrainzError::Api(format!("Failed to create HTTP client: {}", e)))?;

    let response = client
        .get(url_with_params.as_str())
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| MusicBrainzError::Api(format!("HTTP request failed: {}", e)))?;

    if !response.status().is_success() {
        // Try to get error details from response body
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        warn!(
            "MusicBrainz API error response ({}): {}",
            status, error_text
        );

        if status == 404 {
            return Err(MusicBrainzError::NotFound(discid.to_string()));
        }
        return Err(MusicBrainzError::Api(format!(
            "MusicBrainz API returned status {}: {}",
            status, error_text
        )));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| MusicBrainzError::Api(format!("Failed to parse JSON: {}", e)))?;

    debug!("MusicBrainz response: {:#}", json);

    // Parse releases from response
    let mut releases = Vec::new();
    let mut external_urls = ExternalUrls {
        discogs_master_url: None,
        discogs_release_url: None,
        bandcamp_url: None,
    };

    if let Some(releases_array) = json.get("releases").and_then(|r| r.as_array()) {
        for release_json in releases_array {
            if let (Some(id), Some(title), Some(release_group)) = (
                release_json.get("id").and_then(|v| v.as_str()),
                release_json.get("title").and_then(|v| v.as_str()),
                release_json
                    .get("release-group")
                    .and_then(|rg| rg.get("id").and_then(|v| v.as_str())),
            ) {
                // Extract artist from artist-credit
                let artist = release_json
                    .get("artist-credit")
                    .and_then(|ac| ac.as_array())
                    .and_then(|credits| credits.first())
                    .and_then(|credit| credit.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown Artist")
                    .to_string();

                // Extract date
                let date = release_json
                    .get("date")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // Extract country
                let country = release_json
                    .get("country")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // Extract barcode
                let barcode = release_json
                    .get("barcode")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());

                // Extract format from media array
                let format = release_json
                    .get("media")
                    .and_then(|m| m.as_array())
                    .and_then(|media| media.first())
                    .and_then(|m| m.get("format"))
                    .and_then(|f| f.as_str())
                    .map(|s| s.to_string());

                // Extract label and catalog number from label-info array
                let (label, catalog_number) = release_json
                    .get("label-info")
                    .and_then(|li| li.as_array())
                    .and_then(|labels| labels.first())
                    .map(|label_info| {
                        let label_name = label_info
                            .get("label")
                            .and_then(|l| l.get("name"))
                            .and_then(|n| n.as_str())
                            .map(|s| s.to_string());
                        let catalog = label_info
                            .get("catalog-number")
                            .and_then(|c| c.as_str())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string());
                        (label_name, catalog)
                    })
                    .unwrap_or((None, None));

                releases.push(MbRelease {
                    release_id: id.to_string(),
                    release_group_id: release_group.to_string(),
                    title: title.to_string(),
                    artist,
                    date,
                    format,
                    country,
                    label,
                    catalog_number,
                    barcode,
                });

                // Extract URLs from first release (they should be the same across releases)
                if external_urls.discogs_master_url.is_none() {
                    // Parse URL relationships from JSON
                    if let Some(relations) =
                        release_json.get("relations").and_then(|r| r.as_array())
                    {
                        for relation in relations {
                            if let Some(url_obj) = relation.get("url") {
                                if let Some(resource) =
                                    url_obj.get("resource").and_then(|v| v.as_str())
                                {
                                    if resource.contains("discogs.com/master/") {
                                        external_urls.discogs_master_url =
                                            Some(resource.to_string());
                                    } else if resource.contains("discogs.com/release/") {
                                        external_urls.discogs_release_url =
                                            Some(resource.to_string());
                                    } else if resource.contains("bandcamp.com") {
                                        external_urls.bandcamp_url = Some(resource.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if releases.is_empty() {
        return Err(MusicBrainzError::NotFound(discid.to_string()));
    }

    info!(
        "✓ MusicBrainz found {} release(s) for DiscID {}",
        releases.len(),
        discid
    );
    if external_urls.discogs_master_url.is_some() || external_urls.discogs_release_url.is_some() {
        info!("  → Found Discogs URL in relationships");
    }

    Ok((releases, external_urls))
}

/// Fetch a release-group with its URL relationships
async fn fetch_release_group_with_relations(
    release_group_id: &str,
) -> Result<serde_json::Value, MusicBrainzError> {
    let url = format!(
        "https://musicbrainz.org/ws/2/release-group/{}",
        release_group_id
    );
    let url_with_params = format!("{}?inc=url-rels", url);

    debug!("Fetching release-group with relations: {}", url_with_params);

    let client = reqwest::Client::builder()
        .user_agent("bae/1.0 +https://github.com/hideselfview/bae")
        .build()
        .map_err(|e| MusicBrainzError::Api(format!("Failed to create HTTP client: {}", e)))?;

    let response = client
        .get(&url_with_params)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| MusicBrainzError::Api(format!("HTTP request failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(MusicBrainzError::Api(format!(
            "MusicBrainz API returned status: {}",
            response.status()
        )));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| MusicBrainzError::Api(format!("Failed to parse JSON: {}", e)))?;

    Ok(json)
}

/// Lookup a specific release by MusicBrainz release ID and extract external URLs
pub async fn lookup_release_by_id(
    release_id: &str,
) -> Result<(MbRelease, ExternalUrls), MusicBrainzError> {
    info!("🎵 MusicBrainz: Looking up release ID '{}'", release_id);

    let url = format!("https://musicbrainz.org/ws/2/release/{}", release_id);
    let url_with_params = format!(
        "{}?inc=recordings+artist-credits+release-groups+release-group-rels+url-rels+labels+media",
        url
    );

    debug!("MusicBrainz API request: {}", url_with_params);

    let client = reqwest::Client::builder()
        .user_agent("bae/1.0 +https://github.com/hideselfview/bae")
        .build()
        .map_err(|e| MusicBrainzError::Api(format!("Failed to create HTTP client: {}", e)))?;

    let response = client
        .get(&url_with_params)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| MusicBrainzError::Api(format!("HTTP request failed: {}", e)))?;

    if !response.status().is_success() {
        if response.status() == 404 {
            return Err(MusicBrainzError::NotFound(release_id.to_string()));
        }
        return Err(MusicBrainzError::Api(format!(
            "MusicBrainz API returned status: {}",
            response.status()
        )));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| MusicBrainzError::Api(format!("Failed to parse JSON: {}", e)))?;

    debug!("MusicBrainz release response: {:#}", json);

    // Extract release information
    let release_id_str = json
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| MusicBrainzError::Api("Missing release id".to_string()))?
        .to_string();

    let title = json
        .get("title")
        .and_then(|v| v.as_str())
        .ok_or_else(|| MusicBrainzError::Api("Missing release title".to_string()))?
        .to_string();

    let release_group_id = json
        .get("release-group")
        .and_then(|rg| rg.get("id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let artist = json
        .get("artist-credit")
        .and_then(|ac| ac.as_array())
        .and_then(|credits| credits.first())
        .and_then(|credit| credit.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "Unknown Artist".to_string());

    let date = json
        .get("date")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Extract country
    let country = json
        .get("country")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Extract barcode
    let barcode = json
        .get("barcode")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // Extract format from media array
    let format = json
        .get("media")
        .and_then(|m| m.as_array())
        .and_then(|media| media.first())
        .and_then(|m| m.get("format"))
        .and_then(|f| f.as_str())
        .map(|s| s.to_string());

    // Extract label and catalog number from label-info array
    let (label, catalog_number) = json
        .get("label-info")
        .and_then(|li| li.as_array())
        .and_then(|labels| labels.first())
        .map(|label_info| {
            let label_name = label_info
                .get("label")
                .and_then(|l| l.get("name"))
                .and_then(|n| n.as_str())
                .map(|s| s.to_string());
            let catalog = label_info
                .get("catalog-number")
                .and_then(|c| c.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            (label_name, catalog)
        })
        .unwrap_or((None, None));

    // Extract external URLs from relationships
    let mut external_urls = ExternalUrls {
        discogs_master_url: None,
        discogs_release_url: None,
        bandcamp_url: None,
    };

    // Check release-level relationships
    if let Some(relations) = json.get("relations").and_then(|r| r.as_array()) {
        debug!("Found {} relation(s) on release", relations.len());
        for relation in relations {
            // Check if this is a URL relationship
            if let Some(url_obj) = relation.get("url") {
                if let Some(resource) = url_obj.get("resource").and_then(|r| r.as_str()) {
                    debug!("Found URL relation: {}", resource);
                    // Check for Discogs URLs by examining the resource URL directly
                    // This is more reliable than checking relation type
                    if resource.contains("discogs.com/master/") {
                        external_urls.discogs_master_url = Some(resource.to_string());
                        info!("Found Discogs master URL: {}", resource);
                    } else if resource.contains("discogs.com/release/") {
                        external_urls.discogs_release_url = Some(resource.to_string());
                        info!("Found Discogs release URL: {}", resource);
                    } else if resource.contains("bandcamp.com") {
                        external_urls.bandcamp_url = Some(resource.to_string());
                    }
                }
            }
        }
    }

    // Also check release-group relationships (Discogs links are often on the release-group)
    if let Some(release_group) = json.get("release-group") {
        if let Some(rg_relations) = release_group.get("relations").and_then(|r| r.as_array()) {
            debug!("Found {} relation(s) on release-group", rg_relations.len());
            for relation in rg_relations {
                if let Some(url_obj) = relation.get("url") {
                    if let Some(resource) = url_obj.get("resource").and_then(|r| r.as_str()) {
                        debug!("Found URL relation on release-group: {}", resource);
                        if resource.contains("discogs.com/master/")
                            && external_urls.discogs_master_url.is_none()
                        {
                            external_urls.discogs_master_url = Some(resource.to_string());
                            info!("Found Discogs master URL on release-group: {}", resource);
                        } else if resource.contains("discogs.com/release/")
                            && external_urls.discogs_release_url.is_none()
                        {
                            external_urls.discogs_release_url = Some(resource.to_string());
                            info!("Found Discogs release URL on release-group: {}", resource);
                        }
                    }
                }
            }
        } else {
            // Release-group relations not included in response, fetch release-group separately
            if let Some(rg_id) = release_group.get("id").and_then(|v| v.as_str()) {
                debug!(
                    "Release-group relations not found, fetching release-group {} separately",
                    rg_id
                );
                if let Ok(rg_json) = fetch_release_group_with_relations(rg_id).await {
                    if let Some(rg_relations) = rg_json.get("relations").and_then(|r| r.as_array())
                    {
                        debug!(
                            "Found {} relation(s) on release-group (from separate fetch)",
                            rg_relations.len()
                        );
                        for relation in rg_relations {
                            if let Some(url_obj) = relation.get("url") {
                                if let Some(resource) =
                                    url_obj.get("resource").and_then(|r| r.as_str())
                                {
                                    debug!("Found URL relation on release-group: {}", resource);
                                    if resource.contains("discogs.com/master/")
                                        && external_urls.discogs_master_url.is_none()
                                    {
                                        external_urls.discogs_master_url =
                                            Some(resource.to_string());
                                        info!(
                                            "Found Discogs master URL on release-group: {}",
                                            resource
                                        );
                                    } else if resource.contains("discogs.com/release/")
                                        && external_urls.discogs_release_url.is_none()
                                    {
                                        external_urls.discogs_release_url =
                                            Some(resource.to_string());
                                        info!(
                                            "Found Discogs release URL on release-group: {}",
                                            resource
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let release = MbRelease {
        release_id: release_id_str,
        release_group_id,
        title,
        artist,
        date,
        format,
        country,
        label,
        catalog_number,
        barcode,
    };

    Ok((release, external_urls))
}

/// Search MusicBrainz for releases by artist, album, and optional year
pub async fn search_releases(
    artist: &str,
    album: &str,
    year: Option<u32>,
) -> Result<Vec<MbRelease>, MusicBrainzError> {
    info!(
        "🎵 MusicBrainz: Searching for artist='{}', album='{}', year={:?}",
        artist, album, year
    );

    // Build query string - URL encode the query parameters
    let query = if let Some(y) = year {
        format!(
            "artist:\"{}\" AND release:\"{}\" AND date:{}",
            artist, album, y
        )
    } else {
        format!("artist:\"{}\" AND release:\"{}\"", artist, album)
    };

    let url = "https://musicbrainz.org/ws/2/release";

    debug!("MusicBrainz API request: {}?query={}&limit=25&inc=recordings+artist-credits+release-groups+labels+media+url-rels", url, query);

    let client = reqwest::Client::builder()
        .user_agent("bae/1.0 +https://github.com/hideselfview/bae")
        .build()
        .map_err(|e| MusicBrainzError::Api(format!("Failed to create HTTP client: {}", e)))?;

    let response = client
        .get(url)
        .query(&[
            ("query", query.as_str()),
            ("limit", "25"),
            (
                "inc",
                "recordings+artist-credits+release-groups+labels+media+url-rels",
            ),
        ])
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| MusicBrainzError::Api(format!("HTTP request failed: {}", e)))?;

    if !response.status().is_success() {
        // Try to extract error message from response
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        warn!(
            "MusicBrainz API error response ({}): {}",
            status, error_text
        );

        if status == 404 {
            return Ok(Vec::new()); // No results, not an error
        }
        return Err(MusicBrainzError::Api(format!(
            "MusicBrainz API returned status {}: {}",
            status, error_text
        )));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| MusicBrainzError::Api(format!("Failed to parse JSON: {}", e)))?;

    debug!("MusicBrainz search response: {:#}", json);

    // Check for error field in response
    if let Some(error_msg) = json.get("error").and_then(|e| e.as_str()) {
        warn!("MusicBrainz API returned error: {}", error_msg);
        return Err(MusicBrainzError::Api(format!(
            "MusicBrainz error: {}",
            error_msg
        )));
    }

    // Parse releases from response
    let mut releases = Vec::new();

    if let Some(releases_array) = json.get("releases").and_then(|r| r.as_array()) {
        for release_json in releases_array {
            if let (Some(id), Some(title)) = (
                release_json.get("id").and_then(|v| v.as_str()),
                release_json.get("title").and_then(|v| v.as_str()),
            ) {
                let release_group_id = release_json
                    .get("release-group")
                    .and_then(|rg| rg.get("id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                // Extract artist from artist-credit
                let artist = release_json
                    .get("artist-credit")
                    .and_then(|ac| ac.as_array())
                    .and_then(|credits| credits.first())
                    .and_then(|credit| credit.get("name"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "Unknown Artist".to_string());

                // Extract date
                let date = release_json
                    .get("date")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // Extract country
                let country = release_json
                    .get("country")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // Extract barcode
                let barcode = release_json
                    .get("barcode")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());

                // Extract format from media array
                let format = release_json
                    .get("media")
                    .and_then(|m| m.as_array())
                    .and_then(|media| media.first())
                    .and_then(|m| m.get("format"))
                    .and_then(|f| f.as_str())
                    .map(|s| s.to_string());

                // Extract label and catalog number from label-info array
                let (label, catalog_number) = release_json
                    .get("label-info")
                    .and_then(|li| li.as_array())
                    .and_then(|labels| labels.first())
                    .map(|label_info| {
                        let label_name = label_info
                            .get("label")
                            .and_then(|l| l.get("name"))
                            .and_then(|n| n.as_str())
                            .map(|s| s.to_string());
                        let catalog = label_info
                            .get("catalog-number")
                            .and_then(|c| c.as_str())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string());
                        (label_name, catalog)
                    })
                    .unwrap_or((None, None));

                releases.push(MbRelease {
                    release_id: id.to_string(),
                    release_group_id,
                    title: title.to_string(),
                    artist,
                    date,
                    format,
                    country,
                    label,
                    catalog_number,
                    barcode,
                });
            }
        }
    }

    info!(
        "✓ MusicBrainz search returned {} release(s)",
        releases.len()
    );
    Ok(releases)
}
