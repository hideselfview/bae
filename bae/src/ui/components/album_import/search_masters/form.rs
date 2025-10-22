use crate::ui::album_import_context::AlbumImportContext;
use dioxus::prelude::*;

/// Search masters input form with text field and search button
#[component]
pub fn SearchMastersForm() -> Element {
    let album_import_ctx = use_context::<AlbumImportContext>();
    let mut search_query = album_import_ctx.search_query;

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
                    // let mut album_import_ctx = album_import_ctx_clone.clone();
                    move |event: FormEvent| {
                        search_query.set(event.value());
                    }
                },
                onkeydown: {
                    let album_import_ctx = album_import_ctx.clone();

                    move |event: KeyboardEvent| {
                        if event.key() == Key::Enter {
                            let query = search_query.read().clone();
                            album_import_ctx.search_albums(query);
                        }
                    }
                }
            }
            button {
                class: "px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 font-medium",
                onclick: {
                    move |_| {
                        let query = search_query.read().clone();
                        album_import_ctx.search_albums(query);
                    }
                },
                "Search"
            }
        }
    }
}
