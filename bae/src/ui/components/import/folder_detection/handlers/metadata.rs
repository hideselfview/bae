use crate::import::{FolderMetadata, MatchCandidate, MatchSource};
use crate::musicbrainz::lookup_by_discid;
use crate::ui::import_context::ImportPhase;
use dioxus::prelude::*;
use tracing::info;

/// Initialize search query from metadata
fn init_search_query_from_metadata(
    metadata: &FolderMetadata,
    mut search_query: Signal<String>,
) {
    let mut query_parts = Vec::new();
    if let Some(ref artist) = metadata.artist {
        query_parts.push(artist.clone());
    }
    if let Some(ref album) = metadata.album {
        query_parts.push(album.clone());
    }
    search_query.set(query_parts.join(" "));
}

/// Handle metadata detection result and MusicBrainz lookup
///
/// This function processes the result of metadata detection from CUE/log files,
/// performs MusicBrainz DiscID lookup if available, and updates all relevant
/// signals based on the results.
pub async fn handle_metadata_detection(
    metadata: Option<FolderMetadata>,
    fallback_query: String,
    mut detected_metadata: Signal<Option<FolderMetadata>>,
    mut is_looking_up: Signal<bool>,
    mut exact_match_candidates: Signal<Vec<MatchCandidate>>,
    mut search_query: Signal<String>,
    mut import_phase: Signal<ImportPhase>,
    mut confirmed_candidate: Signal<Option<MatchCandidate>>,
) {
    match metadata {
        Some(metadata) => {
            info!("Detected metadata: {:?}", metadata);
            detected_metadata.set(Some(metadata.clone()));

            // Try exact lookup if MB DiscID available
            if let Some(ref mb_discid) = metadata.mb_discid {
                is_looking_up.set(true);
                info!("🎵 Found MB DiscID: {}, performing exact lookup", mb_discid);

                match lookup_by_discid(mb_discid).await {
                    Ok((releases, _external_urls)) => {
                        if releases.is_empty() {
                            info!("No exact matches found, proceeding to manual search");
                            init_search_query_from_metadata(&metadata, search_query);
                            import_phase.set(ImportPhase::ManualSearch);
                        } else if releases.len() == 1 {
                            // Single exact match - auto-proceed to confirmation
                            info!("✅ Single exact match found, auto-proceeding");
                            let mb_release = releases[0].clone();
                            let candidate = MatchCandidate {
                                source: MatchSource::MusicBrainz(mb_release),
                                confidence: 100.0,
                                match_reasons: vec!["Exact DiscID match".to_string()],
                            };
                            confirmed_candidate.set(Some(candidate));
                            import_phase.set(ImportPhase::Confirmation);
                        } else {
                            // Multiple exact matches - show for selection
                            info!(
                                "Found {} exact matches, showing for selection",
                                releases.len()
                            );
                            let candidates: Vec<MatchCandidate> = releases
                                .into_iter()
                                .map(|mb_release| MatchCandidate {
                                    source: MatchSource::MusicBrainz(mb_release),
                                    confidence: 100.0,
                                    match_reasons: vec!["Exact DiscID match".to_string()],
                                })
                                .collect();
                            exact_match_candidates.set(candidates);
                            import_phase.set(ImportPhase::ExactLookup);
                        }
                        is_looking_up.set(false);
                    }
                    Err(e) => {
                        info!(
                            "MB DiscID lookup failed: {}, proceeding to manual search",
                            e
                        );
                        is_looking_up.set(false);
                        init_search_query_from_metadata(&metadata, search_query);
                        import_phase.set(ImportPhase::ManualSearch);
                    }
                }
            } else {
                // No MB DiscID, proceed to manual search with detected metadata
                info!("No MB DiscID found, proceeding to manual search");
                init_search_query_from_metadata(&metadata, search_query);
                import_phase.set(ImportPhase::ManualSearch);
            }
        }
        None => {
            // No metadata detected, proceed with fallback query
            info!("No metadata detected, using fallback query: {}", fallback_query);
            search_query.set(fallback_query);
            import_phase.set(ImportPhase::ManualSearch);
        }
    }
}

