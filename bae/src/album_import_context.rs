use dioxus::prelude::*;
use crate::{discogs, api_keys};
use crate::discogs::DiscogsSearchResult;
use crate::models::{ImportItem, DiscogsMasterReleaseVersion};

#[derive(Debug, Clone, PartialEq)]
pub enum SearchView {
    SearchResults,
    ReleaseDetails { master_id: String, master_title: String },
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
    client: Option<discogs::DiscogsClient>, // Single client instance
}

impl AlbumImportContext {
    
    fn get_client(&mut self) -> Result<&discogs::DiscogsClient, String> {
        if self.client.is_none() {
            match api_keys::retrieve_api_key() {
                Ok(api_key) => {
                    self.client = Some(discogs::DiscogsClient::new(api_key));
                }
                Err(_) => {
                    return Err("No API key configured. Please go to Settings to add your Discogs API key.".to_string());
                }
            }
        }
        Ok(self.client.as_ref().unwrap())
    }

    pub fn search_albums(&mut self, query: String) {
        if query.trim().is_empty() {
            self.search_results.set(Vec::new());
            return;
        }

        // Clone signals first to avoid borrowing conflicts
        let mut search_results = self.search_results.clone();
        let mut is_searching = self.is_searching_masters.clone();
        let mut error_message = self.error_message.clone();

        is_searching.set(true);
        error_message.set(None);

        let client = match self.get_client() {
            Ok(client) => client.clone(),
            Err(error) => {
                error_message.set(Some(error));
                is_searching.set(false);
                return;
            }
        };

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
        self.current_view.set(SearchView::ReleaseDetails { master_id, master_title });
    }

    pub fn navigate_back_to_search(&mut self) {
        self.current_view.set(SearchView::SearchResults);
    }

    pub async fn import_master(&mut self, master_id: String) -> Result<ImportItem, String> {
        let client = match self.get_client() {
            Ok(client) => client.clone(),
            Err(error) => {
                self.error_message.set(Some(error.clone()));
                return Err(error);
            }
        };

        // Find the thumbnail from search results
        let search_thumb = self.search_results.read()
            .iter()
            .find(|result| result.id.to_string() == master_id)
            .and_then(|result| result.thumb.clone());

        self.is_importing_master.set(true);
        self.error_message.set(None);

        let result = match client.get_master(&master_id).await {
            Ok(mut master) => {
                // If master has no thumbnail but search results had one, use the search thumbnail
                if master.thumb.is_none() && search_thumb.is_some() {
                    master.thumb = search_thumb;
                    println!("AlbumImportContext: Using search thumbnail for master {}", master.title);
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

    pub async fn get_master_versions(&mut self, master_id: String) -> Result<Vec<DiscogsMasterReleaseVersion>, String> {
        let client = match self.get_client() {
            Ok(client) => client.clone(),
            Err(error) => {
                self.error_message.set(Some(error.clone()));
                return Err(error);
            }
        };

        self.is_loading_versions.set(true);
        self.error_message.set(None);

        let result = match client.get_master_versions(&master_id).await {
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

    pub async fn import_release(&mut self, release_id: String, master_id: String) -> Result<ImportItem, String> {
        let client = match self.get_client() {
            Ok(client) => client.clone(),
            Err(error) => {
                self.error_message.set(Some(error.clone()));
                return Err(error);
            }
        };

        self.is_importing_release.set(true);
        self.error_message.set(None);

        let result = match client.get_release(&release_id).await {
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
    let album_import_ctx = AlbumImportContext {
        search_query: use_signal(|| String::new()),
        search_results: use_signal(|| Vec::new()),
        is_searching_masters: use_signal(|| false),
        is_loading_versions: use_signal(|| false),
        is_importing_master: use_signal(|| false),
        is_importing_release: use_signal(|| false),
        error_message: use_signal(|| None),
        current_view: use_signal(|| SearchView::SearchResults),
        client: None,
    };
    
    use_context_provider(move || album_import_ctx);
    
    rsx! {
        {children}
    }
}
