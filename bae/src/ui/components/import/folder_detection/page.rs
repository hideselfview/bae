use super::{folder_selector::FolderSelector, match_list::MatchList};
use crate::import::{
    rank_discogs_matches, rank_mb_matches, should_auto_select, FolderMetadata, ImportRequestParams,
};
use crate::library::use_import_service;
use crate::ui::import_context::ImportContext;
use crate::ui::Route;
use dioxus::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;
use tracing::info;

#[component]
pub fn FolderDetectionPage() -> Element {
    let import_context = use_context::<Rc<ImportContext>>();
    let mut folder_path = use_signal(String::new);
    let mut detected_metadata = use_signal(|| None::<FolderMetadata>);
    let mut match_candidates = use_signal(Vec::<crate::import::MatchCandidate>::new);
    let mut selected_match_index = use_signal(|| None::<usize>);
    let mut locked_in_candidate = use_signal(|| None::<crate::import::MatchCandidate>);
    let mut is_detecting = use_signal(|| false);
    let mut is_searching = use_signal(|| false);
    let mut error_message = use_signal(|| None::<String>);

    // Auto-select and lock-in if exactly 1 MB DiscID result
    use_effect(move || {
        let candidates_vec = match_candidates.read();
        let metadata_opt = detected_metadata.read();

        // Check if we have exactly 1 MB result from DiscID search
        if let Some(meta) = metadata_opt.as_ref() {
            if meta.mb_discid.is_some() && candidates_vec.len() == 1 {
                if let Some(candidate) = candidates_vec.first() {
                    // Check if it's a MusicBrainz result
                    if matches!(candidate.source, crate::import::MatchSource::MusicBrainz(_)) {
                        info!("Auto-locking in single MB DiscID result");
                        locked_in_candidate.set(Some(candidate.clone()));
                        return;
                    }
                }
            }
        }

        // Otherwise, use normal auto-select logic for high confidence
        if !candidates_vec.is_empty() {
            if let Some(index) = should_auto_select(&candidates_vec) {
                info!("Auto-selecting candidate at index {}", index);
                // Don't auto-lock-in, just select
            }
        }
    });

    let on_folder_select = {
        let import_context_for_select = import_context.clone();
        move |path: String| {
            let import_context = import_context_for_select.clone();
            folder_path.set(path.clone());
            detected_metadata.set(None);
            match_candidates.set(Vec::new());
            selected_match_index.set(None);
            locked_in_candidate.set(None);
            error_message.set(None);
            is_detecting.set(true);

            spawn(async move {
                // Step 1: Detect metadata
                match import_context.detect_folder_metadata(path.clone()).await {
                    Ok(metadata) => {
                        detected_metadata.set(Some(metadata.clone()));
                        is_detecting.set(false);

                        // Step 2: Search both Discogs and MusicBrainz in parallel
                        is_searching.set(true);

                        let import_context = import_context.clone();
                        spawn(async move {
                            use tracing::{info, warn};

                            // Search both sources in parallel
                            info!("🔍 Starting parallel search: Discogs + MusicBrainz");
                            let (discogs_result, mb_result) = tokio::join!(
                                import_context.search_discogs_by_metadata(&metadata),
                                import_context.search_musicbrainz_by_metadata(&metadata)
                            );

                            let mut all_candidates = Vec::new();
                            let mut search_errors = Vec::new();

                            // Process Discogs results
                            match discogs_result {
                                Ok(results) => {
                                    info!("✓ Discogs returned {} result(s)", results.len());
                                    let ranked = rank_discogs_matches(&metadata, results);
                                    info!("✓ Ranked {} Discogs candidate(s)", ranked.len());
                                    all_candidates.extend(ranked);
                                }
                                Err(e) => {
                                    warn!("✗ Discogs search failed: {}", e);
                                    search_errors.push(format!("Discogs: {}", e));
                                    // Don't fail the whole import if Discogs fails
                                }
                            }

                            // Process MusicBrainz results
                            match mb_result {
                                Ok(results) => {
                                    info!("✓ MusicBrainz returned {} result(s)", results.len());
                                    if results.is_empty() {
                                        warn!("⚠ MusicBrainz search returned 0 results");
                                        search_errors
                                            .push("MusicBrainz: No results found".to_string());
                                    } else {
                                        let ranked = rank_mb_matches(&metadata, results);
                                        info!("✓ Ranked {} MusicBrainz candidate(s)", ranked.len());
                                        all_candidates.extend(ranked);
                                    }
                                }
                                Err(e) => {
                                    warn!("✗ MusicBrainz search failed: {}", e);
                                    search_errors.push(format!("MusicBrainz: {}", e));
                                    // Don't fail the whole import if MusicBrainz fails
                                }
                            }

                            // Sort all candidates by confidence (highest first)
                            all_candidates.sort_by(|a, b| {
                                b.confidence
                                    .partial_cmp(&a.confidence)
                                    .unwrap_or(std::cmp::Ordering::Equal)
                            });

                            info!(
                                "📊 Total candidates after ranking: {}",
                                all_candidates.len()
                            );

                            // Set error message if we have errors but no candidates
                            if all_candidates.is_empty() && !search_errors.is_empty() {
                                let error_msg = format!(
                                    "Search completed but no matches found. {}",
                                    search_errors.join("; ")
                                );
                                warn!("{}", error_msg);
                                error_message.set(Some(error_msg));
                            } else if !search_errors.is_empty() {
                                // Some searches failed but we have results
                                let error_msg = format!(
                                    "Some searches had issues: {}",
                                    search_errors.join("; ")
                                );
                                warn!("{}", error_msg);
                                error_message.set(Some(error_msg));
                            }

                            if all_candidates.is_empty() {
                                warn!("⚠ No candidates found after processing all search results");
                            } else {
                                info!(
                                    "✅ Setting {} candidate(s) for display",
                                    all_candidates.len()
                                );
                            }

                            match_candidates.set(all_candidates);
                            is_searching.set(false);
                            info!("✓ Search completed, is_searching set to false");
                        });
                    }
                    Err(e) => {
                        error_message.set(Some(e));
                        is_detecting.set(false);
                    }
                }
            });
        }
    };

    let on_match_select = move |index: usize| {
        selected_match_index.set(Some(index));
        // Lock in the selected candidate
        if let Some(candidate) = match_candidates.read().get(index) {
            locked_in_candidate.set(Some(candidate.clone()));
        }
    };

    let on_edit = move |_| {
        locked_in_candidate.set(None);
        selected_match_index.set(None);
    };

    let on_confirm = move |_| {
        if let Some(candidate) = locked_in_candidate.read().as_ref().cloned() {
            let folder = folder_path.read().clone();
            let metadata = detected_metadata.read().clone();
            let import_service = use_import_service();
            let navigator = use_navigator();
            let import_context = import_context.clone();
            spawn(async move {
                // Extract master_year from metadata or release date
                let master_year = metadata.as_ref().and_then(|m| m.year).unwrap_or(1970);

                match candidate.source.clone() {
                    crate::import::MatchSource::Discogs(discogs_result) => {
                        // Fetch full Discogs release
                        let master_id = match discogs_result.master_id {
                            Some(id) => id.to_string(),
                            None => {
                                error_message
                                    .set(Some("Discogs result has no master_id".to_string()));
                                return;
                            }
                        };
                        let release_id = discogs_result.id.to_string();

                        match import_context.import_release(release_id, master_id).await {
                            Ok(discogs_release) => {
                                info!(
                                    "Starting import for Discogs release: {}",
                                    discogs_release.title
                                );

                                let request = ImportRequestParams::FromFolder {
                                    discogs_release: Some(discogs_release),
                                    mb_release: None,
                                    folder: PathBuf::from(folder),
                                    master_year,
                                };

                                match import_service.send_request(request).await {
                                    Ok((album_id, _release_id)) => {
                                        info!("Import started, navigating to album: {}", album_id);
                                        navigator.push(Route::AlbumDetail {
                                            album_id,
                                            release_id: String::new(),
                                        });
                                    }
                                    Err(e) => {
                                        error_message
                                            .set(Some(format!("Failed to start import: {}", e)));
                                    }
                                }
                            }
                            Err(e) => {
                                error_message
                                    .set(Some(format!("Failed to fetch Discogs release: {}", e)));
                            }
                        }
                    }
                    crate::import::MatchSource::MusicBrainz(mb_release) => {
                        info!(
                            "Starting import for MusicBrainz release: {}",
                            mb_release.title
                        );

                        let request = ImportRequestParams::FromFolder {
                            discogs_release: None,
                            mb_release: Some(mb_release.clone()),
                            folder: PathBuf::from(folder),
                            master_year,
                        };

                        match import_service.send_request(request).await {
                            Ok((album_id, _release_id)) => {
                                info!("Import started, navigating to album: {}", album_id);
                                navigator.push(Route::AlbumDetail {
                                    album_id,
                                    release_id: String::new(),
                                });
                            }
                            Err(e) => {
                                error_message.set(Some(format!("Failed to start import: {}", e)));
                            }
                        }
                    }
                }
            });
        }
    };

    rsx! {
        div { class: "max-w-4xl mx-auto p-6",
            div { class: "mb-6",
                h1 { class: "text-2xl font-bold text-white", "Import" }
            }

            if folder_path.read().is_empty() {
                div { class: "bg-white rounded-lg shadow p-6",
                        FolderSelector {
                        on_select: on_folder_select,
                        on_error: move |e: String| {
                            error_message.set(Some(e));
                        }
                    }
                }
            } else {
                div { class: "space-y-6",
                    div { class: "bg-white rounded-lg shadow p-6",
                        div { class: "mb-6 pb-4 border-b border-gray-200",
                            h3 { class: "text-sm font-semibold text-gray-700 uppercase tracking-wide mb-2", "Selected Folder" }
                            p { class: "text-sm text-gray-900 font-mono break-all", "{folder_path.read()}" }
                        }

                        if *is_detecting.read() {
                            div { class: "text-center py-8",
                                p { class: "text-gray-600", "Detecting metadata..." }
                            }
                        }
                    }

                    if *is_searching.read() {
                        div { class: "bg-white rounded-lg shadow p-6 text-center",
                            p { class: "text-gray-600", "Searching Discogs and MusicBrainz..." }
                        }
                    } else if let Some(locked_candidate) = locked_in_candidate.read().as_ref() {
                        // Locked-in confirmation view
                        div { class: "space-y-4",
                            div { class: "bg-blue-50 border-2 border-blue-500 rounded-lg p-6",
                                div { class: "flex items-start justify-between mb-4",
                                    div { class: "flex-1",
                                        h3 { class: "text-lg font-semibold text-gray-900 mb-2",
                                            "Selected Release"
                                        }
                                        div { class: "text-sm text-gray-600 space-y-1",
                                            p { class: "text-lg font-medium text-gray-900", "{locked_candidate.title()}" }
                                            if let Some(ref year) = locked_candidate.year() {
                                                p { "Year: {year}" }
                                            }
                                            {
                                                let (format_text, country_text, label_text) = match &locked_candidate.source {
                                                    crate::import::MatchSource::MusicBrainz(release) => (
                                                        release.format.as_ref().map(|f| format!("Format: {}", f)),
                                                        release.country.as_ref().map(|c| format!("Country: {}", c)),
                                                        release.label.as_ref().map(|l| format!("Label: {}", l)),
                                                    ),
                                                    crate::import::MatchSource::Discogs(_) => (None, None, None),
                                                };
                                                rsx! {
                                                    if let Some(ref fmt) = format_text {
                                                        p { "{fmt}" }
                                                    }
                                                    if let Some(ref country) = country_text {
                                                        p { "{country}" }
                                                    }
                                                    if let Some(ref label) = label_text {
                                                        p { "{label}" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    span { class: "text-xs bg-purple-100 text-purple-700 px-2 py-1 rounded",
                                        "{locked_candidate.source_name()}"
                                    }
                                }
                            }
                            div { class: "flex justify-end gap-3",
                                button {
                                    class: "px-6 py-2 bg-gray-600 text-white rounded hover:bg-gray-700",
                                    onclick: on_edit,
                                    "Edit"
                                }
                                button {
                                    class: "px-6 py-2 bg-green-600 text-white rounded hover:bg-green-700",
                                    onclick: on_confirm,
                                    "Import"
                                }
                            }
                        }
                    } else if !match_candidates.read().is_empty() {
                        // Results view (incomplete state - no import button)
                        div { class: "space-y-4",
                            MatchList {
                                candidates: match_candidates.read().clone(),
                                selected_index: selected_match_index.read().as_ref().copied(),
                                on_select: on_match_select,
                            }
                            p { class: "text-sm text-gray-500 text-center",
                                "Select a release above to continue"
                            }
                        }
                    }

                    if let Some(ref error) = error_message.read().as_ref() {
                        div { class: "bg-red-50 border border-red-200 rounded-lg p-4",
                            p { class: "text-sm text-red-700", "Error: {error}" }
                        }
                    }
                }
            }
        }
    }
}
