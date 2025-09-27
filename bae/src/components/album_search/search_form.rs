use dioxus::prelude::*;
use crate::search_context::SearchContext;
use super::search_list::SearchList;

#[component]
pub fn SearchForm() -> Element {
    let search_ctx = use_context::<SearchContext>();
    let search_ctx_clone = search_ctx.clone();

    rsx! {
        div {
            class: "container mx-auto p-6",
            h1 { 
                class: "text-3xl font-bold mb-6",
                "Search Albums" 
            }
            
            div {
                class: "mb-6 flex gap-2",
                input {
                    class: "flex-1 p-3 border border-gray-300 rounded-lg text-lg",
                    placeholder: "Search for albums, artists, or releases...",
                    value: "{search_ctx.search_query}",
                    oninput: {
                        let mut search_ctx = search_ctx_clone.clone();
                        move |event: FormEvent| {
                            search_ctx.search_query.set(event.value());
                        }
                    },
                    onkeydown: {
                        let mut search_ctx = search_ctx_clone.clone();
                        move |event: KeyboardEvent| {
                            if event.key() == Key::Enter {
                                let query = search_ctx.search_query.read().clone();
                                search_ctx.search_albums(query);
                            }
                        }
                    }
                }
                button {
                    class: "px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 font-medium",
                    onclick: {
                        let mut search_ctx = search_ctx_clone.clone();
                        move |_| {
                            let query = search_ctx.search_query.read().clone();
                            search_ctx.search_albums(query);
                        }
                    },
                    "Search"
                }
            }

            if *search_ctx.is_searching_masters.read() {
                div {
                    class: "text-center py-8",
                    p { 
                        class: "text-gray-600",
                        "Searching..." 
                    }
                }
            } else if let Some(error) = search_ctx.error_message.read().as_ref() {
                div {
                    class: "bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-4",
                    "{error}"
                }
            }

            SearchList {}
        }
    }
}
