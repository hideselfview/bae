use super::match_list::MatchList;
use super::source_selector::{SearchSource, SearchSourceSelector};
use crate::import::MatchCandidate;
use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;
use std::rc::Rc;

#[component]
pub fn ManualSearchPanel(
    detected_metadata: Signal<Option<crate::import::FolderMetadata>>,
    on_match_select: EventHandler<usize>,
    on_confirm: EventHandler<MatchCandidate>,
    selected_index: Signal<Option<usize>>,
) -> Element {
    let import_context = use_context::<Rc<ImportContext>>();
    let mut search_source = use_signal(|| SearchSource::MusicBrainz);
    let mut search_query = use_signal(String::new);
    let mut match_candidates = use_signal(Vec::<MatchCandidate>::new);
    let mut is_searching = use_signal(|| false);
    let mut error_message = use_signal(|| None::<String>);

    // Pre-fill search query from detected metadata
    use_effect(move || {
        if let Some(metadata) = detected_metadata.read().as_ref() {
            let mut query_parts = Vec::new();
            if let Some(ref artist) = metadata.artist {
                query_parts.push(artist.clone());
            }
            if let Some(ref album) = metadata.album {
                query_parts.push(album.clone());
            }
            if !query_parts.is_empty() {
                search_query.set(query_parts.join(" "));
            }
        }
    });

    let on_search_click = {
        let import_context = import_context.clone();
        move |_| {
            if search_query.read().trim().is_empty() {
                error_message.set(Some("Please enter a search query".to_string()));
                return;
            }

            is_searching.set(true);
            error_message.set(None);
            match_candidates.set(Vec::new());

            let import_context_clone = import_context.clone();
            let query = search_query.read().clone();
            let source = search_source.read().clone();
            let metadata = detected_metadata.read().clone();
            let mut is_searching_clone = is_searching;
            let mut error_message_clone = error_message;
            let mut match_candidates_clone = match_candidates;

            spawn(async move {
                use tracing::info;

                match source {
                    SearchSource::MusicBrainz => {
                        info!("Searching MusicBrainz with query: {}", query);
                        if let Some(ref meta) = metadata {
                            match import_context_clone
                                .search_musicbrainz_by_metadata(meta)
                                .await
                            {
                                Ok(results) => {
                                    use crate::import::rank_mb_matches;
                                    let ranked = rank_mb_matches(meta, results);
                                    match_candidates_clone.set(ranked);
                                }
                                Err(e) => {
                                    error_message_clone
                                        .set(Some(format!("MusicBrainz search failed: {}", e)));
                                }
                            }
                        } else {
                            error_message_clone
                                .set(Some("No metadata available for search".to_string()));
                        }
                    }
                    SearchSource::Discogs => {
                        info!("Searching Discogs with query: {}", query);
                        if let Some(ref meta) = metadata {
                            match import_context_clone.search_discogs_by_metadata(meta).await {
                                Ok(results) => {
                                    use crate::import::rank_discogs_matches;
                                    let ranked = rank_discogs_matches(meta, results);
                                    match_candidates_clone.set(ranked);
                                }
                                Err(e) => {
                                    error_message_clone
                                        .set(Some(format!("Discogs search failed: {}", e)));
                                }
                            }
                        } else {
                            error_message_clone
                                .set(Some("No metadata available for search".to_string()));
                        }
                    }
                }

                is_searching_clone.set(false);
            });
        }
    };

    let on_search_keydown = {
        let import_context = import_context.clone();
        move |evt: dioxus::html::KeyboardEvent| {
            if evt.key() == dioxus::html::Key::Enter {
                if search_query.read().trim().is_empty() {
                    error_message.set(Some("Please enter a search query".to_string()));
                    return;
                }

                is_searching.set(true);
                error_message.set(None);
                match_candidates.set(Vec::new());

                let import_context_clone = import_context.clone();
                let query = search_query.read().clone();
                let source = search_source.read().clone();
                let metadata = detected_metadata.read().clone();
                let mut is_searching_clone = is_searching;
                let mut error_message_clone = error_message;
                let mut match_candidates_clone = match_candidates;

                spawn(async move {
                    use tracing::info;

                    match source {
                        SearchSource::MusicBrainz => {
                            info!("Searching MusicBrainz with query: {}", query);
                            if let Some(ref meta) = metadata {
                                match import_context_clone
                                    .search_musicbrainz_by_metadata(meta)
                                    .await
                                {
                                    Ok(results) => {
                                        use crate::import::rank_mb_matches;
                                        let ranked = rank_mb_matches(meta, results);
                                        match_candidates_clone.set(ranked);
                                    }
                                    Err(e) => {
                                        error_message_clone
                                            .set(Some(format!("MusicBrainz search failed: {}", e)));
                                    }
                                }
                            } else {
                                error_message_clone
                                    .set(Some("No metadata available for search".to_string()));
                            }
                        }
                        SearchSource::Discogs => {
                            info!("Searching Discogs with query: {}", query);
                            if let Some(ref meta) = metadata {
                                match import_context_clone.search_discogs_by_metadata(meta).await {
                                    Ok(results) => {
                                        use crate::import::rank_discogs_matches;
                                        let ranked = rank_discogs_matches(meta, results);
                                        match_candidates_clone.set(ranked);
                                    }
                                    Err(e) => {
                                        error_message_clone
                                            .set(Some(format!("Discogs search failed: {}", e)));
                                    }
                                }
                            } else {
                                error_message_clone
                                    .set(Some("No metadata available for search".to_string()));
                            }
                        }
                    }

                    is_searching_clone.set(false);
                });
            }
        }
    };

    rsx! {
        div { class: "bg-white rounded-lg shadow p-6 space-y-4",
            h3 { class: "text-lg font-semibold text-gray-900 mb-4", "Search for Release" }

            SearchSourceSelector {
                selected_source: search_source,
                on_select: move |source| {
                    search_source.set(source);
                    match_candidates.set(Vec::new());
                    error_message.set(None);
                }
            }

            div { class: "flex gap-2",
                input {
                    r#type: "text",
                    class: "flex-1 px-4 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500",
                    placeholder: "Enter artist and album name...",
                    value: "{search_query.read()}",
                    oninput: move |e| search_query.set(e.value()),
                    onkeydown: on_search_keydown,
                }
                button {
                    class: "px-6 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50",
                    disabled: *is_searching.read() || search_query.read().trim().is_empty(),
                    onclick: on_search_click,
                    if *is_searching.read() {
                        "Searching..."
                    } else {
                        "Search"
                    }
                }
            }

            if let Some(ref error) = error_message.read().as_ref() {
                div { class: "bg-red-50 border border-red-200 rounded-lg p-4",
                    p { class: "text-sm text-red-700", "Error: {error}" }
                }
            }

            if *is_searching.read() {
                div { class: "text-center py-8",
                    p { class: "text-gray-600", "Searching..." }
                }
            } else if !match_candidates.read().is_empty() {
                div { class: "space-y-4",
                    MatchList {
                        candidates: match_candidates.read().clone(),
                        selected_index: selected_index.read().as_ref().copied(),
                        on_select: move |index| {
                            selected_index.set(Some(index));
                            on_match_select.call(index);
                        }
                    }
                    if selected_index.read().is_some() {
                        div { class: "flex justify-end",
                            button {
                                class: "px-6 py-2 bg-green-600 text-white rounded-lg hover:bg-green-700",
                                onclick: move |_| {
                                    if let Some(index) = selected_index.read().as_ref().copied() {
                                        if let Some(candidate) = match_candidates.read().get(index) {
                                            on_confirm.call(candidate.clone());
                                        }
                                    }
                                },
                                "Confirm Selection"
                            }
                        }
                    }
                }
            }
        }
    }
}
