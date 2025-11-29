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
    pub first_release_date: Option<String>,
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

                // Extract first release date from release-group
                let first_release_date = release_json
                    .get("release-group")
                    .and_then(|rg| rg.get("first-release-date"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
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
                    first_release_date,
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
/// Returns the full JSON response for reuse by callers
pub async fn lookup_release_by_id(
    release_id: &str,
) -> Result<(MbRelease, ExternalUrls, serde_json::Value), MusicBrainzError> {
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

    // Extract first release date from release-group
    let first_release_date = json
        .get("release-group")
        .and_then(|rg| rg.get("first-release-date"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
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
        first_release_date,
        format,
        country,
        label,
        catalog_number,
        barcode,
    };

    Ok((release, external_urls, json))
}

/// Parameters for searching MusicBrainz releases
#[derive(Debug, Clone, Default)]
pub struct ReleaseSearchParams {
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<String>,
    pub catalog_number: Option<String>,
    pub barcode: Option<String>,
    pub format: Option<String>,
    pub country: Option<String>,
}

impl ReleaseSearchParams {
    /// Check if at least one field is filled
    pub fn has_any_field(&self) -> bool {
        self.artist.is_some()
            || self.album.is_some()
            || self.year.is_some()
            || self.catalog_number.is_some()
            || self.barcode.is_some()
            || self.format.is_some()
            || self.country.is_some()
    }

    /// Build Lucene query string from filled fields
    fn build_query(&self) -> String {
        let mut parts = Vec::new();

        if let Some(ref artist) = self.artist {
            if !artist.trim().is_empty() {
                parts.push(format!("artist:\"{}\"", artist.trim()));
            }
        }
        if let Some(ref album) = self.album {
            if !album.trim().is_empty() {
                parts.push(format!("release:\"{}\"", album.trim()));
            }
        }
        if let Some(ref year) = self.year {
            if !year.trim().is_empty() {
                parts.push(format!("date:{}", year.trim()));
            }
        }
        if let Some(ref catno) = self.catalog_number {
            if !catno.trim().is_empty() {
                parts.push(format!("catno:\"{}\"", catno.trim()));
            }
        }
        if let Some(ref barcode) = self.barcode {
            if !barcode.trim().is_empty() {
                parts.push(format!("barcode:{}", barcode.trim()));
            }
        }
        if let Some(ref format) = self.format {
            if !format.trim().is_empty() {
                parts.push(format!("format:\"{}\"", format.trim()));
            }
        }
        if let Some(ref country) = self.country {
            if !country.trim().is_empty() {
                parts.push(format!("country:\"{}\"", country.trim()));
            }
        }

        parts.join(" AND ")
    }
}

/// Clean album name for search by removing common metadata patterns
pub fn clean_album_name_for_search(album: &str) -> String {
    use regex::Regex;

    let mut cleaned = album.to_string();

    // Remove catalog numbers in brackets: [Label 123-456, Year]
    let bracket_pattern = Regex::new(r"\s*\[([^\]]+)\]\s*").unwrap();
    cleaned = bracket_pattern.replace_all(&cleaned, " ").to_string();

    // Remove year in parentheses at the end: (1968), (2024)
    let year_pattern = Regex::new(r"\s*\((\d{4})\)\s*$").unwrap();
    cleaned = year_pattern.replace_all(&cleaned, "").to_string();

    // Remove disc indicators: (Disc 2), (CD1), (CD 2)
    let disc_pattern = Regex::new(r"(?i)\s*\((Disc|CD)\s*\d+\)\s*$").unwrap();
    cleaned = disc_pattern.replace_all(&cleaned, "").to_string();

    // Remove edition markers: (Remastered), (Deluxe Edition), etc.
    let edition_pattern =
        Regex::new(r"(?i)\s*\((Remaster(ed)?|Deluxe|Limited|Special|Expanded)(\s+Edition)?\)\s*$")
            .unwrap();
    cleaned = edition_pattern.replace_all(&cleaned, "").to_string();

    // Trim and collapse multiple spaces
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract catalog number from album or folder name
pub fn extract_catalog_number(text: &str) -> Option<String> {
    use regex::Regex;

    // Match patterns like [Label 123-456] or [123-456, Year]
    let bracket_pattern = Regex::new(r"\[([^\]]+)\]").unwrap();

    if let Some(caps) = bracket_pattern.captures(text) {
        if let Some(content) = caps.get(1) {
            let content_str = content.as_str();

            // Try to extract catalog number pattern: alphanumeric with dashes/spaces
            let catno_pattern = Regex::new(r"([A-Z0-9][\w\s\-]+\d+)").unwrap();

            if let Some(catno_caps) = catno_pattern.captures(content_str) {
                if let Some(catno) = catno_caps.get(1) {
                    let catno_str = catno.as_str().trim();
                    // Filter out years (4 digits only)
                    if !Regex::new(r"^\d{4}$").unwrap().is_match(catno_str) {
                        return Some(catno_str.to_string());
                    }
                }
            }
        }
    }

    None
}

/// Search MusicBrainz for releases using structured parameters
pub async fn search_releases_with_params(
    params: &ReleaseSearchParams,
) -> Result<Vec<MbRelease>, MusicBrainzError> {
    if !params.has_any_field() {
        return Err(MusicBrainzError::Api(
            "At least one search field must be provided".to_string(),
        ));
    }

    let query = params.build_query();

    info!("🎵 MusicBrainz: Searching with params: {:?}", params);
    info!("   Query: {}", query);

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
            return Ok(Vec::new());
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

    if let Some(error_msg) = json.get("error").and_then(|e| e.as_str()) {
        warn!("MusicBrainz API returned error: {}", error_msg);
        return Err(MusicBrainzError::Api(format!(
            "MusicBrainz error: {}",
            error_msg
        )));
    }

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

                let artist = release_json
                    .get("artist-credit")
                    .and_then(|ac| ac.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|first| first.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown Artist")
                    .to_string();

                let date = release_json
                    .get("date")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let country = release_json
                    .get("country")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let barcode = release_json
                    .get("barcode")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let label = release_json
                    .get("label-info")
                    .and_then(|li| li.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|first| first.get("label"))
                    .and_then(|label| label.get("name"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let catalog_number = release_json
                    .get("label-info")
                    .and_then(|li| li.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|first| first.get("catalog-number"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                releases.push(MbRelease {
                    release_id: id.to_string(),
                    release_group_id,
                    title: title.to_string(),
                    artist,
                    date,
                    first_release_date: None,
                    format: None,
                    country,
                    label,
                    catalog_number,
                    barcode,
                });
            }
        }
    }

    info!("✓ Found {} release(s)", releases.len());
    Ok(releases)
}

/// Search MusicBrainz for releases by artist, album, and optional year
/// This is a convenience wrapper around search_releases_with_params
pub async fn search_releases(
    artist: &str,
    album: &str,
    year: Option<u32>,
) -> Result<Vec<MbRelease>, MusicBrainzError> {
    info!(
        "🎵 MusicBrainz: Searching for artist='{}', album='{}', year={:?}",
        artist, album, year
    );

    let params = ReleaseSearchParams {
        artist: Some(artist.to_string()),
        album: Some(album.to_string()),
        year: year.map(|y| y.to_string()),
        ..Default::default()
    };

    search_releases_with_params(&params).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_album_name() {
        assert_eq!(
            clean_album_name_for_search("Electric Ladyland (1968) [Polydor 823 359-2, 1984]"),
            "Electric Ladyland"
        );
        assert_eq!(
            clean_album_name_for_search("Back In Black (Disc 2)"),
            "Back In Black"
        );
        assert_eq!(
            clean_album_name_for_search("Abbey Road (Remastered)"),
            "Abbey Road"
        );
        assert_eq!(
            clean_album_name_for_search("The Wall (Deluxe Edition)"),
            "The Wall"
        );
    }

    #[test]
    fn test_extract_catalog_number() {
        assert_eq!(
            extract_catalog_number("Electric Ladyland (1968) [Polydor 823 359-2, 1984]"),
            Some("Polydor 823 359-2".to_string())
        );
        assert_eq!(
            extract_catalog_number("Back In Black [Atlantic A2 16018]"),
            Some("Atlantic A2 16018".to_string())
        );
        assert_eq!(extract_catalog_number("No catalog here"), None);
    }

    #[test]
    fn test_release_search_params_build_query() {
        let params = ReleaseSearchParams {
            artist: Some("Hendrix".to_string()),
            album: Some("Electric Ladyland".to_string()),
            year: Some("1968".to_string()),
            ..Default::default()
        };
        assert_eq!(
            params.build_query(),
            "artist:\"Hendrix\" AND release:\"Electric Ladyland\" AND date:1968"
        );

        let params2 = ReleaseSearchParams {
            artist: Some("ACDC".to_string()),
            catalog_number: Some("A2 16018".to_string()),
            ..Default::default()
        };
        assert_eq!(
            params2.build_query(),
            "artist:\"ACDC\" AND catno:\"A2 16018\""
        );
    }
}

/// Fetch the first release date for a release group
/// Returns the earliest release date from all releases in the group
pub async fn fetch_release_group_first_date(
    release_group_id: &str,
) -> Result<Option<String>, MusicBrainzError> {
    info!(
        "🎵 MusicBrainz: Fetching first release date for release group '{}'",
        release_group_id
    );

    let url = format!(
        "https://musicbrainz.org/ws/2/release-group/{}",
        release_group_id
    );
    let url_with_params = format!("{}?inc=releases", url);

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
        return Err(MusicBrainzError::Api(format!(
            "MusicBrainz API returned status: {}",
            response.status()
        )));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| MusicBrainzError::Api(format!("Failed to parse JSON: {}", e)))?;

    debug!("MusicBrainz release-group response: {:#}", json);

    // Get the first-release-date field if available
    if let Some(first_date) = json.get("first-release-date").and_then(|v| v.as_str()) {
        if !first_date.is_empty() {
            info!("✓ Found first release date: {}", first_date);
            return Ok(Some(first_date.to_string()));
        }
    }

    info!("No first release date found for release group");
    Ok(None)
}
