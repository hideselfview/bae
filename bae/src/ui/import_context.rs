use crate::config::use_config;
use crate::discogs::client::DiscogsSearchResult;
use crate::discogs::{DiscogsAlbum, DiscogsClient, DiscogsMasterReleaseVersion};
use dioxus::prelude::*;
use tracing::debug;

#[derive(Debug, Clone, PartialEq)]
pub enum SearchView {
    SearchResults,
    ReleaseDetails {
        master_id: String,
        master_title: String,
    },
}

#[derive(Clone)]
pub struct ImportContext {
    pub search_query: Signal<String>,
    pub search_results: Signal<Vec<DiscogsSearchResult>>,
    pub is_searching_masters: Signal<bool>,
    pub is_loading_versions: Signal<bool>,
    pub error_message: Signal<Option<String>>,
    pub current_view: Signal<SearchView>,
    client: DiscogsClient,
}

impl ImportContext {
    pub fn new(config: &crate::config::Config) -> Self {
        Self {
            search_query: use_signal(String::new),
            search_results: use_signal(Vec::new),
            is_searching_masters: use_signal(|| false),
            is_loading_versions: use_signal(|| false),
            error_message: use_signal(|| None),
            current_view: use_signal(|| SearchView::SearchResults),
            client: DiscogsClient::new(config.discogs_api_key.clone()),
        }
    }

    pub fn search_albums(&self, query: String) {
        let mut search_results = self.search_results;

        if query.trim().is_empty() {
            search_results.set(Vec::new());
            return;
        }

        // Copy signals to avoid borrowing conflicts (Signal implements Copy)
        let mut is_searching = self.is_searching_masters;
        let mut error_message = self.error_message;

        is_searching.set(true);
        error_message.set(None);

        let client = self.client.clone();

        spawn(async move {
            match client.search_masters(&query, "").await {
                Ok(results) => {
                    search_results.set(results);
                }
                Err(e) => {
                    error_message.set(Some(format!("Search failed: {}", e)));
                }
            }

            is_searching.set(false);
        });
    }

    pub fn navigate_to_releases(&mut self, master_id: String, master_title: String) {
        self.current_view.set(SearchView::ReleaseDetails {
            master_id,
            master_title,
        });
    }

    pub fn navigate_back_to_search(&mut self) {
        self.current_view.set(SearchView::SearchResults);
    }

    pub fn reset(&mut self) {
        self.search_query.set(String::new());
        self.search_results.set(Vec::new());
        self.is_searching_masters.set(false);
        self.is_loading_versions.set(false);
        self.error_message.set(None);
        self.current_view.set(SearchView::SearchResults);
    }

    pub async fn import_master(&mut self, master_id: String) -> Result<DiscogsAlbum, String> {
        self.error_message.set(None);

        // Find the thumbnail from search results
        let search_thumb = self
            .search_results
            .read()
            .iter()
            .find(|result| result.id.to_string() == master_id)
            .and_then(|result| result.thumb.clone());

        match self.client.get_master(&master_id).await {
            Ok(mut master) => {
                // If master has no thumbnail but search results had one, use the search thumbnail
                if master.thumb.is_none() && search_thumb.is_some() {
                    master.thumb = search_thumb;
                    debug!("Using search thumbnail for master {}", master.title);
                }
                Ok(DiscogsAlbum::Master(master))
            }
            Err(e) => {
                let error = format!("Failed to fetch master details: {}", e);
                self.error_message.set(Some(error.clone()));
                Err(error)
            }
        }
    }

    pub async fn get_master_versions(
        &mut self,
        master_id: String,
    ) -> Result<Vec<DiscogsMasterReleaseVersion>, String> {
        self.is_loading_versions.set(true);
        self.error_message.set(None);

        let result = match self.client.get_master_versions(&master_id).await {
            Ok(versions) => Ok(versions),
            Err(e) => {
                let error = format!("Failed to load releases: {}", e);
                self.error_message.set(Some(error.clone()));
                Err(error)
            }
        };

        self.is_loading_versions.set(false);
        result
    }

    pub async fn import_release(
        &mut self,
        release_id: String,
        master_id: String,
    ) -> Result<DiscogsAlbum, String> {
        self.error_message.set(None);

        match self.client.get_release(&release_id).await {
            Ok(mut release) => {
                release.master_id = Some(master_id);
                Ok(DiscogsAlbum::Release(release))
            }
            Err(e) => {
                let error = format!("Failed to fetch release details: {}", e);
                self.error_message.set(Some(error.clone()));
                Err(error)
            }
        }
    }
}

/// Provider component to make search context available throughout the app
#[component]
pub fn AlbumImportContextProvider(children: Element) -> Element {
    let config = use_config();
    let album_import_ctx = ImportContext::new(&config);

    use_context_provider(move || album_import_ctx);

    rsx! {
        {children}
    }
}
