use crate::discogs::DiscogsSearchResult;
use crate::models::{DiscogsMasterReleaseVersion, ImportItem};
use crate::{config::use_config, discogs};
use dioxus::prelude::*;

#[derive(Debug, Clone, PartialEq)]
pub enum SearchView {
    SearchResults,
    ReleaseDetails {
        master_id: String,
        master_title: String,
    },
}

#[derive(Clone)]
pub struct AlbumImportContext {
    pub search_query: Signal<String>,
    pub search_results: Signal<Vec<DiscogsSearchResult>>,
    pub is_searching_masters: Signal<bool>,
    pub is_loading_versions: Signal<bool>,
    pub is_importing_master: Signal<bool>,
    pub is_importing_release: Signal<bool>,
    pub error_message: Signal<Option<String>>,
    pub current_view: Signal<SearchView>,
    client: discogs::DiscogsClient,
}

impl AlbumImportContext {
    pub fn new(config: &crate::config::Config) -> Self {
        Self {
            search_query: use_signal(String::new),
            search_results: use_signal(Vec::new),
            is_searching_masters: use_signal(|| false),
            is_loading_versions: use_signal(|| false),
            is_importing_master: use_signal(|| false),
            is_importing_release: use_signal(|| false),
            error_message: use_signal(|| None),
            current_view: use_signal(|| SearchView::SearchResults),
            client: discogs::DiscogsClient::new(config.discogs_api_key.clone()),
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

    pub async fn import_master(&mut self, master_id: String) -> Result<ImportItem, String> {
        self.is_importing_master.set(true);
        self.error_message.set(None);

        // Find the thumbnail from search results
        let search_thumb = self
            .search_results
            .read()
            .iter()
            .find(|result| result.id.to_string() == master_id)
            .and_then(|result| result.thumb.clone());

        let result = match self.client.get_master(&master_id).await {
            Ok(mut master) => {
                // If master has no thumbnail but search results had one, use the search thumbnail
                if master.thumb.is_none() && search_thumb.is_some() {
                    master.thumb = search_thumb;
                    println!(
                        "AlbumImportContext: Using search thumbnail for master {}",
                        master.title
                    );
                }
                let import_item = ImportItem::Master(master);
                Ok(import_item)
            }
            Err(e) => {
                let error = format!("Failed to fetch master details: {}", e);
                self.error_message.set(Some(error.clone()));
                Err(error)
            }
        };

        self.is_importing_master.set(false);
        result
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
    ) -> Result<ImportItem, String> {
        self.is_importing_release.set(true);
        self.error_message.set(None);

        let result = match self.client.get_release(&release_id).await {
            Ok(mut release) => {
                release.master_id = Some(master_id);
                let import_item = ImportItem::Release(release);
                Ok(import_item)
            }
            Err(e) => {
                let error = format!("Failed to fetch release details: {}", e);
                self.error_message.set(Some(error.clone()));
                Err(error)
            }
        };

        self.is_importing_release.set(false);
        result
    }
}

/// Provider component to make search context available throughout the app
#[component]
pub fn AlbumImportContextProvider(children: Element) -> Element {
    let config = use_config();
    let album_import_ctx = AlbumImportContext::new(&config);

    use_context_provider(move || album_import_ctx);

    rsx! {
        {children}
    }
}
