use dioxus::prelude::*;
use crate::{models, discogs, api_keys};

#[derive(Debug, Clone, PartialEq)]
pub enum SearchView {
    SearchResults,
    ReleaseDetails { master_id: String, master_title: String },
}

#[derive(Clone)]
pub struct SearchContext {
    pub search_query: Signal<String>,
    pub search_results: Signal<Vec<models::DiscogsRelease>>,
    pub is_loading: Signal<bool>,
    pub error_message: Signal<Option<String>>,
    pub current_view: Signal<SearchView>,
}

impl SearchContext {

    pub fn search_albums(&mut self, query: String) {
        if query.trim().is_empty() {
            self.search_results.set(Vec::new());
            return;
        }

        let mut search_results = self.search_results.clone();
        let mut is_loading = self.is_loading.clone();
        let mut error_message = self.error_message.clone();

        spawn(async move {
            is_loading.set(true);
            error_message.set(None);

            // Get API key from secure storage
            match api_keys::retrieve_api_key() {
                Ok(api_key) => {
                    let client = discogs::DiscogsClient::new(api_key);
                    
                    match client.search_masters(&query, "").await {
                        Ok(results) => {
                            search_results.set(results);
                        }
                        Err(e) => {
                            error_message.set(Some(format!("Search failed: {}", e)));
                        }
                    }
                }
                Err(_) => {
                    error_message.set(Some("No API key configured. Please go to Settings to add your Discogs API key.".to_string()));
                }
            }
            
            is_loading.set(false);
        });
    }

    pub fn navigate_to_releases(&mut self, master_id: String, master_title: String) {
        self.current_view.set(SearchView::ReleaseDetails { master_id, master_title });
    }

    pub fn navigate_back_to_search(&mut self) {
        self.current_view.set(SearchView::SearchResults);
    }
}

/// Provider component to make search context available throughout the app
#[component]
pub fn SearchContextProvider(children: Element) -> Element {
    let search_ctx = SearchContext {
        search_query: use_signal(|| String::new()),
        search_results: use_signal(|| Vec::new()),
        is_loading: use_signal(|| false),
        error_message: use_signal(|| None),
        current_view: use_signal(|| SearchView::SearchResults),
    };
    
    use_context_provider(move || search_ctx);
    
    rsx! {
        {children}
    }
}
