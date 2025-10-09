use crate::album_import_context::AlbumImportContext;
use crate::secure_config::use_secure_config;
use dioxus::prelude::*;

/// Search masters input form with text field and search button
#[component]
pub fn SearchMastersForm() -> Element {
    let album_import_ctx = use_context::<AlbumImportContext>();
    let album_import_ctx_clone = album_import_ctx.clone();
    let secure_config = use_secure_config();

    rsx! {
        div {
            class: "mb-6 flex gap-2",
            input {
                class: "flex-1 p-3 border border-gray-300 rounded-lg text-lg",
                onmounted: move |element| {
                    spawn(async move {
                        let _ = element.set_focus(true).await;
                    });
                },
                placeholder: "Search for albums, artists, or releases...",
                value: "{album_import_ctx.search_query}",
                oninput: {
                    let mut album_import_ctx = album_import_ctx_clone.clone();
                    move |event: FormEvent| {
                        album_import_ctx.search_query.set(event.value());
                    }
                },
                onkeydown: {
                    let mut album_import_ctx = album_import_ctx_clone.clone();
                    let secure_config = secure_config.clone();
                    move |event: KeyboardEvent| {
                        if event.key() == Key::Enter {
                            let query = album_import_ctx.search_query.read().clone();
                            album_import_ctx.search_albums(query, &secure_config);
                        }
                    }
                }
            }
            button {
                class: "px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 font-medium",
                onclick: {
                    let mut album_import_ctx = album_import_ctx_clone.clone();
                    let secure_config = secure_config.clone();
                    move |_| {
                        let query = album_import_ctx.search_query.read().clone();
                        album_import_ctx.search_albums(query, &secure_config);
                    }
                },
                "Search"
            }
        }
    }
}
