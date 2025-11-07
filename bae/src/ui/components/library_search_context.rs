use dioxus::prelude::*;

/// Shared library search state that tracks search query across the app
#[derive(Clone)]
pub struct LibrarySearchState {
    pub search_query: Signal<String>,
}

/// Provider component to make library search state available throughout the app
#[component]
pub fn LibrarySearchContextProvider(children: Element) -> Element {
    let search_query = use_signal(String::new);
    let search_state = LibrarySearchState { search_query };

    use_context_provider(|| search_state.clone());

    rsx! {
        {children}
    }
}

/// Hook to access the library search query
pub fn use_library_search() -> Signal<String> {
    let state = use_context::<LibrarySearchState>();
    state.search_query
}

