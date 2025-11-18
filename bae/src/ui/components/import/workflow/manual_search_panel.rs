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

    let mut search_query = import_context.search_query();
    let search_source = import_context.search_source();
    let match_candidates = import_context.manual_match_candidates();
    let mut is_searching = use_signal(|| false);
    let error_message = import_context.error_message();

    let on_search_click = {
        let import_context = import_context.clone();

        move |_| {
            if search_query.read().trim().is_empty() {
                import_context.set_error_message(Some("Please enter a search query".to_string()));
                return;
            }

            is_searching.set(true);
            import_context.set_error_message(None);
            import_context.set_manual_match_candidates(Vec::new());

            let import_context_clone = import_context.clone();
            let query = search_query.read().clone();
            let source = search_source.read().clone();
            let mut is_searching_clone = is_searching;

            spawn(async move {
                use tracing::info;

                info!(
                    "Searching {} with query: {}",
                    match source {
                        SearchSource::MusicBrainz => "MusicBrainz",
                        SearchSource::Discogs => "Discogs",
                    },
                    query
                );

                match import_context_clone.search_for_matches(query, source).await {
                    Ok(candidates) => {
                        import_context_clone.set_manual_match_candidates(candidates);
                    }
                    Err(e) => {
                        import_context_clone
                            .set_error_message(Some(format!("Search failed: {}", e)));
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
                    import_context
                        .set_error_message(Some("Please enter a search query".to_string()));
                    return;
                }

                is_searching.set(true);
                import_context.set_error_message(None);
                import_context.set_manual_match_candidates(Vec::new());

                let import_context_clone = import_context.clone();
                let query = search_query.read().clone();
                let source = search_source.read().clone();
                let mut is_searching_clone = is_searching;

                spawn(async move {
                    use tracing::info;

                    info!(
                        "Searching {} with query: {}",
                        match source {
                            SearchSource::MusicBrainz => "MusicBrainz",
                            SearchSource::Discogs => "Discogs",
                        },
                        query
                    );

                    match import_context_clone.search_for_matches(query, source).await {
                        Ok(candidates) => {
                            import_context_clone.set_manual_match_candidates(candidates);
                        }
                        Err(e) => {
                            import_context_clone
                                .set_error_message(Some(format!("Search failed: {}", e)));
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
                    import_context.set_search_source(source);
                    import_context.set_manual_match_candidates(Vec::new());
                    import_context.set_error_message(None);
                }
            }

            div { class: "flex gap-2",
                input {
                    r#type: "text",
                    id: "manual-search-input",
                    class: "flex-1 px-4 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 text-gray-900 placeholder-gray-500",
                    placeholder: "Enter artist and album name...",
                    value: "{search_query}",
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
                    p { class: "text-sm text-red-700 select-text", "Error: {error}" }
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
