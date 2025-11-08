use super::{
    folder_selector::FolderSelector, match_list::MatchList, metadata_display::MetadataDisplay,
};
use crate::import::{
    rank_discogs_matches, rank_mb_matches, should_auto_select, FolderMetadata, MatchCandidate,
};
use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;
use std::rc::Rc;
use tracing::info;

fn extract_discogs_master_id(url: &str) -> Option<String> {
    // Extract master ID from URL like https://www.discogs.com/master/12345
    url.split("/master/")
        .nth(1)
        .and_then(|s| s.split('/').next())
        .map(|s| s.to_string())
}

fn extract_discogs_release_id(url: &str) -> Option<String> {
    // Extract release ID from URL like https://www.discogs.com/release/12345
    url.split("/release/")
        .nth(1)
        .and_then(|s| s.split('/').next())
        .map(|s| s.to_string())
}

#[component]
pub fn FolderDetectionPage() -> Element {
    let import_context = use_context::<Rc<ImportContext>>();
    let folder_path = use_signal(|| String::new());
    let detected_metadata = use_signal(|| None::<FolderMetadata>);
    let match_candidates = use_signal(|| Vec::<MatchCandidate>::new());
    let selected_match_index = use_signal(|| None::<usize>);
    let is_detecting = use_signal(|| false);
    let is_searching = use_signal(|| false);
    let error_message = use_signal(|| None::<String>);

    // Auto-select if we have high confidence match
    use_effect({
        let candidates = match_candidates.clone();
        let mut selected = selected_match_index.clone();
        move || {
            let candidates_vec = candidates.read();
            if !candidates_vec.is_empty() {
                if let Some(index) = should_auto_select(&candidates_vec) {
                    info!("Auto-selecting candidate at index {}", index);
                    selected.set(Some(index));
                }
            }
        }
    });

    let on_folder_select = {
        let import_context = import_context.clone();
        let mut folder_path_signal = folder_path.clone();
        let mut detected_metadata_signal = detected_metadata.clone();
        let mut match_candidates_signal = match_candidates.clone();
        let mut is_detecting_signal = is_detecting.clone();
        let is_searching_signal = is_searching.clone();
        let mut error_message_signal = error_message.clone();
        let mut selected_match_index_signal = selected_match_index.clone();

        move |path: String| {
            folder_path_signal.set(path.clone());
            detected_metadata_signal.set(None);
            match_candidates_signal.set(Vec::new());
            selected_match_index_signal.set(None);
            error_message_signal.set(None);
            is_detecting_signal.set(true);

            let import_context = import_context.clone();
            let mut detected_metadata_signal = detected_metadata_signal.clone();
            let mut match_candidates_signal = match_candidates_signal.clone();
            let mut is_searching_signal = is_searching_signal.clone();
            let mut error_message_signal = error_message_signal.clone();
            let mut is_detecting_signal = is_detecting_signal.clone();

            spawn(async move {
                // Step 1: Detect metadata
                match import_context.detect_folder_metadata(path.clone()).await {
                    Ok(metadata) => {
                        detected_metadata_signal.set(Some(metadata.clone()));
                        is_detecting_signal.set(false);

                        // Step 2: Search both Discogs and MusicBrainz in parallel
                        is_searching_signal.set(true);

                        let import_context_discogs = import_context.clone();
                        let import_context_mb = import_context.clone();
                        let metadata_discogs = metadata.clone();
                        let metadata_mb = metadata.clone();
                        let mut match_candidates_signal = match_candidates_signal.clone();
                        let mut is_searching_signal = is_searching_signal.clone();
                        let mut error_message_signal = error_message_signal.clone();

                        spawn(async move {
                            use tracing::{info, warn};

                            // Search both sources in parallel
                            info!("🔍 Starting parallel search: Discogs + MusicBrainz");
                            let (discogs_result, mb_result) = tokio::join!(
                                import_context_discogs
                                    .search_discogs_by_metadata(&metadata_discogs),
                                import_context_mb.search_musicbrainz_by_metadata(&metadata_mb)
                            );

                            let mut all_candidates = Vec::new();
                            let mut search_errors = Vec::new();

                            // Process Discogs results
                            match discogs_result {
                                Ok(results) => {
                                    info!("✓ Discogs returned {} result(s)", results.len());
                                    let ranked = rank_discogs_matches(&metadata_discogs, results);
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
                                        let ranked = rank_mb_matches(&metadata_mb, results);
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
                                error_message_signal.set(Some(error_msg));
                            } else if !search_errors.is_empty() {
                                // Some searches failed but we have results
                                let error_msg = format!(
                                    "Some searches had issues: {}",
                                    search_errors.join("; ")
                                );
                                warn!("{}", error_msg);
                                error_message_signal.set(Some(error_msg));
                            }

                            if all_candidates.is_empty() {
                                warn!("⚠ No candidates found after processing all search results");
                            } else {
                                info!(
                                    "✅ Setting {} candidate(s) for display",
                                    all_candidates.len()
                                );
                            }

                            match_candidates_signal.set(all_candidates);
                            is_searching_signal.set(false);
                            info!("✓ Search completed, is_searching set to false");
                        });
                    }
                    Err(e) => {
                        error_message_signal.set(Some(e));
                        is_detecting_signal.set(false);
                    }
                }
            });
        }
    };

    let mut selected_match_index_signal = selected_match_index.clone();
    let on_match_select = move |index: usize| {
        selected_match_index_signal.set(Some(index));
    };

    let on_confirm = {
        let import_context = import_context.clone();
        let match_candidates_value = match_candidates.clone();
        let selected_index = selected_match_index.clone();
        let mut error_message_signal = error_message.clone();

        move |_| {
            if let Some(index) = selected_index.read().as_ref() {
                if let Some(candidate) = match_candidates_value.read().get(*index) {
                    // Try to get Discogs master_id directly
                    if let Some(master_id) = candidate.discogs_master_id() {
                        info!(
                            "Selected match: {} (master_id: {})",
                            candidate.title(),
                            master_id
                        );
                        import_context.navigate_to_import_workflow(master_id.to_string(), None);
                    } else {
                        // For MusicBrainz results, try to find Discogs link, but proceed with MB-only if not found
                        match &candidate.source {
                            crate::import::MatchSource::MusicBrainz(release) => {
                                use crate::musicbrainz::lookup_release_by_id;
                                let release_id = release.release_id.clone();
                                let import_context = import_context.clone();
                                let mut error_message_signal = error_message_signal.clone();

                                spawn(async move {
                                    match lookup_release_by_id(&release_id).await {
                                        Ok((mb_release, external_urls)) => {
                                            // Try to extract Discogs master/release ID from URLs if available
                                            if let Some(ref discogs_url) =
                                                external_urls.discogs_master_url
                                            {
                                                if let Some(master_id) =
                                                    extract_discogs_master_id(discogs_url)
                                                {
                                                    info!("Found Discogs master_id: {} from MusicBrainz release", master_id);
                                                    import_context.navigate_to_import_workflow(
                                                        master_id, None,
                                                    );
                                                    return;
                                                }
                                            }
                                            if let Some(ref discogs_url) =
                                                external_urls.discogs_release_url
                                            {
                                                if let Some(release_id) =
                                                    extract_discogs_release_id(discogs_url)
                                                {
                                                    // For release URLs, we'd need to lookup master_id
                                                    // For now, proceed with MusicBrainz-only import
                                                    info!("Found Discogs release URL but proceeding with MusicBrainz-only import");
                                                }
                                            }
                                            // No Discogs link found - proceed with MusicBrainz-only import
                                            info!("Proceeding with MusicBrainz-only import for release: {} (no Discogs link found)", mb_release.release_id);
                                            // TODO: Implement MusicBrainz-only import workflow
                                            // For now, just log that we're proceeding without Discogs
                                            // The import workflow will need to be updated to handle MusicBrainz-only imports
                                            error_message_signal.set(Some("MusicBrainz-only imports are supported, but the import workflow needs to be updated to handle MusicBrainz data. Please select a release with a Discogs link for now.".to_string()));
                                        }
                                        Err(e) => {
                                            error_message_signal.set(Some(format!(
                                                "Failed to lookup MusicBrainz release: {}",
                                                e
                                            )));
                                        }
                                    }
                                });
                            }
                            crate::import::MatchSource::Discogs(_) => {
                                error_message_signal
                                    .set(Some("Selected match has no master_id".to_string()));
                            }
                        }
                    }
                }
            }
        }
    };

    let on_back = {
        let import_context = import_context.clone();
        move |_| {
            import_context.navigate_back();
        }
    };

    rsx! {
        div { class: "max-w-4xl mx-auto p-6",
            div { class: "mb-6",
                button {
                    class: "text-blue-600 hover:text-blue-800 mb-4",
                    onclick: on_back,
                    "← Back"
                }
                h1 { class: "text-2xl font-bold text-white", "Import from Folder" }
            }

            if folder_path.read().is_empty() {
                div { class: "bg-white rounded-lg shadow p-6",
                        FolderSelector {
                        on_select: on_folder_select,
                        on_error: {
                            let mut error_message_signal = error_message.clone();
                            move |e: String| {
                                error_message_signal.set(Some(e));
                            }
                        }
                    }
                }
            } else {
                div { class: "space-y-6",
                    div { class: "bg-white rounded-lg shadow p-6",
                        div { class: "mb-4",
                            p { class: "text-sm text-gray-600", "Selected folder: {folder_path.read()}" }
                        }

                        if *is_detecting.read() {
                            div { class: "text-center py-8",
                                p { class: "text-gray-600", "Detecting metadata..." }
                            }
                        } else if let Some(ref metadata) = detected_metadata.read().as_ref() {
                            MetadataDisplay { metadata: (*metadata).clone() }
                        }
                    }

                    if *is_searching.read() {
                        div { class: "bg-white rounded-lg shadow p-6 text-center",
                            p { class: "text-gray-600", "Searching Discogs and MusicBrainz..." }
                        }
                    } else if !match_candidates.read().is_empty() {
                        div { class: "space-y-4",
                            MatchList {
                                candidates: match_candidates.read().clone(),
                                selected_index: selected_match_index.read().as_ref().copied(),
                                on_select: on_match_select,
                            }

                            div { class: "flex justify-end",
                                button {
                                    class: "px-6 py-2 bg-green-600 text-white rounded hover:bg-green-700 disabled:opacity-50",
                                    disabled: selected_match_index.read().is_none(),
                                    onclick: on_confirm,
                                    "Continue to Import"
                                }
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
