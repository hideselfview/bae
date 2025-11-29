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

    let mut search_artist = import_context.search_artist();
    let mut search_album = import_context.search_album();
    let mut search_year = import_context.search_year();
    let mut search_catalog_number = import_context.search_catalog_number();
    let mut search_barcode = import_context.search_barcode();
    let mut search_format = import_context.search_format();
    let mut search_country = import_context.search_country();

    let search_source = import_context.search_source();
    let match_candidates = import_context.manual_match_candidates();
    let mut is_searching = use_signal(|| false);
    let error_message = import_context.error_message();

    // Check if any field has content
    let has_any_field = move || {
        !search_artist.read().trim().is_empty()
            || !search_album.read().trim().is_empty()
            || !search_year.read().trim().is_empty()
            || !search_catalog_number.read().trim().is_empty()
            || !search_barcode.read().trim().is_empty()
            || !search_format.read().trim().is_empty()
            || !search_country.read().trim().is_empty()
    };

    let mut perform_search = {
        let import_context = import_context.clone();
        move || {
            is_searching.set(true);
            import_context.set_error_message(None);
            import_context.set_manual_match_candidates(Vec::new());

            let import_context_clone = import_context.clone();
            let source = search_source.read().clone();
            let mut is_searching_clone = is_searching;

            spawn(async move {
                use tracing::info;

                info!(
                    "Searching {}",
                    match source {
                        SearchSource::MusicBrainz => "MusicBrainz",
                        SearchSource::Discogs => "Discogs",
                    }
                );

                match import_context_clone.search_for_matches(source).await {
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

    let on_search_click = move |_| {
        perform_search();
    };

    rsx! {
        div { class: "bg-gray-800 rounded-lg shadow p-6 space-y-4",
            h3 { class: "text-lg font-semibold text-white mb-4", "Search for Release" }

            // Top row: Source selector and Clear button
            div { class: "flex justify-between items-center",
                SearchSourceSelector {
                    selected_source: search_source,
                    on_select: move |source| {
                        import_context.set_search_source(source);
                        import_context.set_manual_match_candidates(Vec::new());
                        import_context.set_error_message(None);
                    }
                }
                button {
                    class: "px-4 py-2 bg-gray-600 text-white rounded-lg hover:bg-gray-700 disabled:opacity-50 disabled:cursor-not-allowed",
                    disabled: !has_any_field(),
                    onclick: move |_| {
                        search_artist.set(String::new());
                        search_album.set(String::new());
                        search_year.set(String::new());
                        search_catalog_number.set(String::new());
                        search_barcode.set(String::new());
                        search_format.set(String::new());
                        search_country.set(String::new());
                    },
                    "Clear"
                }
            }

            // Structured search form
            div { class: "space-y-3",
                // Artist field
                div {
                    label { class: "block text-sm font-medium text-gray-300 mb-1", "Artist" }
                    input {
                        r#type: "text",
                        class: "w-full px-4 py-2 bg-gray-700 border border-gray-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 text-white placeholder-gray-400",
                        placeholder: "Artist name...",
                        value: "{search_artist}",
                        oninput: move |e| search_artist.set(e.value()),
                    }
                }

                // Album field
                div {
                    label { class: "block text-sm font-medium text-gray-300 mb-1", "Album" }
                    input {
                        r#type: "text",
                        class: "w-full px-4 py-2 bg-gray-700 border border-gray-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 text-white placeholder-gray-400",
                        placeholder: "Album title...",
                        value: "{search_album}",
                        oninput: move |e| search_album.set(e.value()),
                    }
                }

                // Two-column layout for remaining fields
                div { class: "grid grid-cols-2 gap-3",
                    // Year field
                    div {
                        label { class: "block text-sm font-medium text-gray-300 mb-1", "Year" }
                        input {
                            r#type: "text",
                            class: "w-full px-4 py-2 bg-gray-700 border border-gray-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 text-white placeholder-gray-400",
                            placeholder: "YYYY",
                            value: "{search_year}",
                            oninput: move |e| search_year.set(e.value()),
                        }
                    }

                    // Catalog Number field
                    div {
                        label { class: "block text-sm font-medium text-gray-300 mb-1", "Catalog Number" }
                        input {
                            r#type: "text",
                            class: "w-full px-4 py-2 bg-gray-700 border border-gray-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 text-white placeholder-gray-400",
                            placeholder: "e.g. 823 359-2",
                            value: "{search_catalog_number}",
                            oninput: move |e| search_catalog_number.set(e.value()),
                        }
                    }

                    // Barcode field
                    div {
                        label { class: "block text-sm font-medium text-gray-300 mb-1", "Barcode" }
                        input {
                            r#type: "text",
                            class: "w-full px-4 py-2 bg-gray-700 border border-gray-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 text-white placeholder-gray-400",
                            placeholder: "UPC/EAN...",
                            value: "{search_barcode}",
                            oninput: move |e| search_barcode.set(e.value()),
                        }
                    }

                    // Format field
                    div {
                        label { class: "block text-sm font-medium text-gray-300 mb-1", "Format" }
                        input {
                            r#type: "text",
                            class: "w-full px-4 py-2 bg-gray-700 border border-gray-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 text-white placeholder-gray-400",
                            placeholder: "CD, Vinyl...",
                            value: "{search_format}",
                            oninput: move |e| search_format.set(e.value()),
                        }
                    }

                    // Country field
                    div {
                        label { class: "block text-sm font-medium text-gray-300 mb-1", "Country" }
                        input {
                            r#type: "text",
                            class: "w-full px-4 py-2 bg-gray-700 border border-gray-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 text-white placeholder-gray-400",
                            placeholder: "US, UK, JP...",
                            value: "{search_country}",
                            oninput: move |e| search_country.set(e.value()),
                        }
                    }
                }

                // Search button
                div { class: "flex justify-end pt-2",
                    button {
                        class: "px-6 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed",
                        disabled: *is_searching.read() || !has_any_field(),
                        onclick: on_search_click,
                        if *is_searching.read() {
                            "Searching..."
                        } else {
                            "Search"
                        }
                    }
                }
            }

            if let Some(ref error) = error_message.read().as_ref() {
                div { class: "bg-red-900/30 border border-red-700 rounded-lg p-4",
                    p { class: "text-sm text-red-300 select-text", "Error: {error}" }
                }
            }

            if *is_searching.read() {
                div { class: "text-center py-8",
                    p { class: "text-gray-400", "Searching..." }
                }
            } else if !match_candidates.read().is_empty() {
                div { class: "space-y-4 mt-4",
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
