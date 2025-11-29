use super::state::ImportContext;
use crate::discogs::client::DiscogsSearchResult;
use crate::import::{FolderMetadata, MatchCandidate};
use crate::musicbrainz::{
    search_releases, search_releases_with_params, MbRelease, ReleaseSearchParams,
};
use crate::ui::components::import::SearchSource;
use dioxus::prelude::*;
use tracing::{info, warn};

pub async fn search_discogs_by_metadata(
    ctx: &ImportContext,
    metadata: &FolderMetadata,
) -> Result<Vec<DiscogsSearchResult>, String> {
    info!("🔍 Starting Discogs search with metadata:");
    info!(
        "   Artist: {:?}, Album: {:?}, Year: {:?}, DISCID: {:?}",
        metadata.artist, metadata.album, metadata.year, metadata.discid
    );

    // Try DISCID search first if available
    if let Some(ref discid) = metadata.discid {
        info!("🎯 Attempting DISCID search: {}", discid);
        match ctx.discogs_client.search_by_discid(discid).await {
            Ok(results) if !results.is_empty() => {
                info!("✓ DISCID search returned {} result(s)", results.len());
                return Ok(results);
            }
            Ok(_) => {
                warn!("✗ DISCID search returned 0 results, falling back to text search");
            }
            Err(e) => {
                warn!("✗ DISCID search failed: {}, falling back to text search", e);
            }
        }
    } else {
        info!("No DISCID available, using text search");
    }

    // Fall back to metadata search
    if let (Some(ref artist), Some(ref album)) = (&metadata.artist, &metadata.album) {
        info!(
            "🔎 Searching Discogs by text: artist='{}', album='{}', year={:?}",
            artist, album, metadata.year
        );

        match ctx
            .discogs_client
            .search_by_metadata(artist, album, metadata.year)
            .await
        {
            Ok(results) => {
                info!("✓ Text search returned {} result(s)", results.len());
                for (i, result) in results.iter().enumerate().take(5) {
                    info!(
                        "   {}. {} (master_id: {:?}, year: {:?})",
                        i + 1,
                        result.title,
                        result.master_id,
                        result.year
                    );
                }
                Ok(results)
            }
            Err(e) => {
                warn!("✗ Text search failed: {}", e);
                Err(format!("Discogs search failed: {}", e))
            }
        }
    } else {
        warn!("✗ Insufficient metadata for search (missing artist or album)");
        Err("Insufficient metadata for search".to_string())
    }
}

pub async fn search_musicbrainz_by_metadata(
    _ctx: &ImportContext, // ctx not used but kept for consistency if needed later
    metadata: &FolderMetadata,
) -> Result<Vec<MbRelease>, String> {
    info!("🎵 Starting MusicBrainz search with metadata:");
    info!(
        "   Artist: {:?}, Album: {:?}, Year: {:?}",
        metadata.artist, metadata.album, metadata.year
    );

    if let (Some(ref artist), Some(ref album)) = (&metadata.artist, &metadata.album) {
        info!(
            "🔎 Searching MusicBrainz by text: artist='{}', album='{}', year={:?}",
            artist, album, metadata.year
        );

        match search_releases(artist, album, metadata.year).await {
            Ok(releases) => {
                info!(
                    "✓ MusicBrainz text search returned {} result(s)",
                    releases.len()
                );
                for (i, release) in releases.iter().enumerate().take(5) {
                    info!(
                        "   {}. {} - {} (release_id: {}, release_group_id: {})",
                        i + 1,
                        release.artist,
                        release.title,
                        release.release_id,
                        release.release_group_id
                    );
                }
                Ok(releases)
            }
            Err(e) => {
                warn!("✗ MusicBrainz text search failed: {}", e);
                Err(format!("MusicBrainz search failed: {}", e))
            }
        }
    } else {
        warn!("✗ Insufficient metadata for search (missing artist or album)");
        Err("Insufficient metadata for search".to_string())
    }
}

pub async fn search_for_matches(
    ctx: &ImportContext,
    source: SearchSource,
) -> Result<Vec<MatchCandidate>, String> {
    let metadata = ctx.detected_metadata().read().clone();

    match source {
        SearchSource::MusicBrainz => {
            // Build search params from UI fields
            let artist = ctx.search_artist().read().clone();
            let album = ctx.search_album().read().clone();
            let year = ctx.search_year().read().clone();
            let catalog_number = ctx.search_catalog_number().read().clone();
            let barcode = ctx.search_barcode().read().clone();
            let format = ctx.search_format().read().clone();
            let country = ctx.search_country().read().clone();

            let params = ReleaseSearchParams {
                artist: if artist.trim().is_empty() {
                    None
                } else {
                    Some(artist)
                },
                album: if album.trim().is_empty() {
                    None
                } else {
                    Some(album)
                },
                year: if year.trim().is_empty() {
                    None
                } else {
                    Some(year)
                },
                catalog_number: if catalog_number.trim().is_empty() {
                    None
                } else {
                    Some(catalog_number)
                },
                barcode: if barcode.trim().is_empty() {
                    None
                } else {
                    Some(barcode)
                },
                format: if format.trim().is_empty() {
                    None
                } else {
                    Some(format)
                },
                country: if country.trim().is_empty() {
                    None
                } else {
                    Some(country)
                },
            };

            if !params.has_any_field() {
                return Err("Please fill in at least one search field".to_string());
            }

            info!("🎵 Starting MusicBrainz search with params: {:?}", params);

            match search_releases_with_params(&params).await {
                Ok(releases) => {
                    info!("✓ MusicBrainz search returned {} result(s)", releases.len());
                    for (i, release) in releases.iter().enumerate().take(5) {
                        info!(
                            "   {}. {} - {} (release_id: {}, release_group_id: {})",
                            i + 1,
                            release.artist,
                            release.title,
                            release.release_id,
                            release.release_group_id
                        );
                    }

                    // Use detected metadata for ranking if available
                    if let Some(ref meta) = metadata {
                        use crate::import::rank_mb_matches;
                        Ok(rank_mb_matches(meta, releases))
                    } else {
                        // No metadata for ranking, return unranked results
                        Ok(releases
                            .into_iter()
                            .map(|release| MatchCandidate {
                                source: crate::import::MatchSource::MusicBrainz(release),
                                confidence: 50.0,
                                match_reasons: vec!["Manual search result".to_string()],
                                cover_art_url: None,
                            })
                            .collect())
                    }
                }
                Err(e) => {
                    warn!("✗ MusicBrainz search failed: {}", e);
                    Err(format!("MusicBrainz search failed: {}", e))
                }
            }
        }
        SearchSource::Discogs => {
            if let Some(ref meta) = metadata {
                let results = search_discogs_by_metadata(ctx, meta).await?;
                use crate::import::rank_discogs_matches;
                Ok(rank_discogs_matches(meta, results))
            } else {
                Err("No metadata available for search".to_string())
            }
        }
    }
}
