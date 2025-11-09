use crate::discogs::client::DiscogsSearchResult;
use crate::import::folder_metadata_detector::FolderMetadata;
use crate::musicbrainz::MbRelease;

#[derive(Debug, Clone, PartialEq)]
pub enum MatchSource {
    Discogs(DiscogsSearchResult),
    MusicBrainz(MbRelease),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchCandidate {
    pub source: MatchSource,
    pub confidence: f32, // 0-100%
    pub match_reasons: Vec<String>,
}

impl MatchCandidate {
    pub fn title(&self) -> String {
        match &self.source {
            MatchSource::Discogs(result) => result.title.clone(),
            MatchSource::MusicBrainz(release) => format!("{} - {}", release.artist, release.title),
        }
    }

    pub fn year(&self) -> Option<String> {
        match &self.source {
            MatchSource::Discogs(result) => result.year.clone(),
            MatchSource::MusicBrainz(release) => release.date.clone(),
        }
    }

    pub fn cover_art_url(&self) -> Option<String> {
        match &self.source {
            MatchSource::Discogs(result) => {
                result.cover_image.clone().or_else(|| result.thumb.clone())
            }
            MatchSource::MusicBrainz(_) => {
                // MusicBrainz doesn't provide cover art URLs directly
                // Could fetch from Cover Art Archive API, but for now return None
                None
            }
        }
    }
}

/// Normalize a string for comparison (lowercase, remove punctuation)
fn normalize_string(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Check if two normalized strings match exactly
fn exact_match(a: &str, b: &str) -> bool {
    normalize_string(a) == normalize_string(b)
}

/// Check if one normalized string contains the other
fn contains_match(a: &str, b: &str) -> bool {
    let norm_a = normalize_string(a);
    let norm_b = normalize_string(b);
    norm_a.contains(&norm_b) || norm_b.contains(&norm_a)
}

/// Extract artist name from Discogs title (format: "Artist - Album" or just "Album")
fn extract_artist_from_title(title: &str) -> Option<String> {
    if let Some((artist, _)) = title.split_once(" - ") {
        Some(artist.trim().to_string())
    } else {
        None
    }
}

/// Extract album name from Discogs title
fn extract_album_from_title(title: &str) -> String {
    if let Some((_, album)) = title.split_once(" - ") {
        album.trim().to_string()
    } else {
        title.trim().to_string()
    }
}

/// Rank MusicBrainz search results against folder metadata
pub fn rank_mb_matches(
    folder_metadata: &FolderMetadata,
    mb_results: Vec<MbRelease>,
) -> Vec<MatchCandidate> {
    use tracing::{debug, info};

    info!(
        "🎯 Ranking {} MusicBrainz result(s) against folder metadata",
        mb_results.len()
    );
    debug!(
        "   Folder: artist={:?}, album={:?}, year={:?}",
        folder_metadata.artist, folder_metadata.album, folder_metadata.year
    );

    let mut candidates: Vec<MatchCandidate> = mb_results
        .into_iter()
        .enumerate()
        .map(|(idx, result)| {
            debug!(
                "Evaluating MB result {}: '{}' by '{}'",
                idx + 1,
                result.title,
                result.artist
            );

            let mut confidence = 0.0;
            let mut match_reasons = Vec::new();

            // Check artist match
            if let Some(ref folder_artist) = folder_metadata.artist {
                if exact_match(folder_artist, &result.artist) {
                    confidence += 50.0;
                    match_reasons.push("Artist exact match".to_string());
                } else if contains_match(folder_artist, &result.artist) {
                    confidence += 30.0;
                    match_reasons.push("Artist partial match".to_string());
                }
            }

            // Check album match
            if let Some(ref folder_album) = folder_metadata.album {
                if exact_match(folder_album, &result.title) {
                    confidence += 40.0;
                    match_reasons.push("Album exact match".to_string());
                } else if contains_match(folder_album, &result.title) {
                    confidence += 30.0;
                    match_reasons.push("Album partial match".to_string());
                }
            }

            // Check year match
            if let Some(folder_year) = folder_metadata.year {
                if let Some(ref result_date) = result.date {
                    // Try to extract year from date string (format: "YYYY" or "YYYY-MM-DD")
                    if let Some(year_str) = result_date.split('-').next() {
                        if let Ok(result_year) = year_str.parse::<u32>() {
                            if folder_year == result_year {
                                confidence += 10.0;
                                match_reasons.push("Year match".to_string());
                            } else if (folder_year as i32 - result_year as i32).abs() <= 1 {
                                confidence += 5.0;
                                match_reasons.push("Year close match".to_string());
                            }
                        }
                    }
                }
            }

            // Bonus for MusicBrainz DiscID match
            if folder_metadata.mb_discid.is_some() {
                // If we have a MusicBrainz DiscID, we could check it here
                // For now, just note that MB results are generally more reliable
                confidence += 5.0;
                match_reasons.push("MusicBrainz source".to_string());
            }

            debug!(
                "   → Confidence: {:.1}%, reasons: {:?}",
                confidence, match_reasons
            );

            MatchCandidate {
                source: MatchSource::MusicBrainz(result),
                confidence,
                match_reasons,
            }
        })
        .collect();

    // Sort by confidence (highest first)
    candidates.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    info!("✓ Ranked {} MB candidate(s)", candidates.len());
    for (i, candidate) in candidates.iter().enumerate().take(3) {
        info!(
            "   {}. {} (confidence: {:.1}%, reasons: {})",
            i + 1,
            candidate.title(),
            candidate.confidence,
            candidate.match_reasons.join(", ")
        );
    }

    candidates
}

/// Rank Discogs search results against folder metadata
pub fn rank_discogs_matches(
    folder_metadata: &FolderMetadata,
    discogs_results: Vec<DiscogsSearchResult>,
) -> Vec<MatchCandidate> {
    use tracing::{debug, info};

    info!(
        "🎯 Ranking {} Discogs result(s) against folder metadata",
        discogs_results.len()
    );
    debug!(
        "   Folder: artist={:?}, album={:?}, year={:?}",
        folder_metadata.artist, folder_metadata.album, folder_metadata.year
    );

    let mut candidates: Vec<MatchCandidate> = discogs_results
        .into_iter()
        .enumerate()
        .map(|(idx, result)| {
            debug!("Evaluating result {}: '{}'", idx + 1, result.title);

            let mut confidence = 0.0;
            let mut match_reasons = Vec::new();

            // Extract artist and album from Discogs title
            let discogs_artist = extract_artist_from_title(&result.title);
            let discogs_album = extract_album_from_title(&result.title);

            debug!(
                "   → Extracted: artist={:?}, album={:?}",
                discogs_artist, discogs_album
            );

            // Check artist match
            if let Some(ref folder_artist) = folder_metadata.artist {
                if let Some(ref d_artist) = discogs_artist {
                    if exact_match(folder_artist, d_artist) {
                        confidence += 50.0;
                        match_reasons.push("Artist exact match".to_string());
                    } else if contains_match(folder_artist, d_artist) {
                        confidence += 30.0;
                        match_reasons.push("Artist partial match".to_string());
                    }
                }
            }

            // Check album match
            if let Some(ref folder_album) = folder_metadata.album {
                if exact_match(folder_album, &discogs_album) {
                    confidence += 40.0;
                    match_reasons.push("Album exact match".to_string());
                } else if contains_match(folder_album, &discogs_album) {
                    confidence += 30.0;
                    match_reasons.push("Album partial match".to_string());
                }
            }

            // Check year match
            if let Some(folder_year) = folder_metadata.year {
                if let Some(ref result_year_str) = result.year {
                    if let Ok(result_year) = result_year_str.parse::<u32>() {
                        if folder_year == result_year {
                            confidence += 10.0;
                            match_reasons.push("Year match".to_string());
                        } else if (folder_year as i32 - result_year as i32).abs() <= 1 {
                            confidence += 5.0;
                            match_reasons.push("Year close match".to_string());
                        }
                    }
                }
            }

            debug!(
                "   → Confidence: {:.1}%, reasons: {:?}",
                confidence, match_reasons
            );

            MatchCandidate {
                source: MatchSource::Discogs(result),
                confidence,
                match_reasons,
            }
        })
        .collect();

    // Sort by confidence (highest first)
    candidates.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    info!("✓ Ranked {} Discogs candidate(s)", candidates.len());
    for (i, candidate) in candidates.iter().enumerate().take(3) {
        info!(
            "   {}. {} (confidence: {:.1}%, reasons: {})",
            i + 1,
            candidate.title(),
            candidate.confidence,
            candidate.match_reasons.join(", ")
        );
    }

    candidates
}
