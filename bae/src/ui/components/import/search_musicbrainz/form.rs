use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;
use std::rc::Rc;

/// Search MusicBrainz form with artist, album, and optional year fields
#[component]
pub fn SearchMusicBrainzForm() -> Element {
    let album_import_ctx = use_context::<Rc<ImportContext>>();
    let mut artist = use_signal(|| String::new());
    let mut album = use_signal(|| String::new());
    let mut year = use_signal(|| Option::<u32>::None);
    let mut year_input = use_signal(|| String::new());
    let is_searching = album_import_ctx.is_searching_mb;

    let on_search_click = {
        let album_import_ctx = album_import_ctx.clone();
        let artist = artist.clone();
        let album = album.clone();
        let year = year.clone();

        move |_event| {
            let artist_val = artist.read().clone();
            let album_val = album.read().clone();
            let year_val = *year.read();

            if artist_val.is_empty() || album_val.is_empty() {
                return;
            }

            let album_import_ctx = album_import_ctx.clone();
            let mut is_searching = album_import_ctx.is_searching_mb;
            let mut mb_error = album_import_ctx.mb_error_message;
            let mut mb_results = album_import_ctx.mb_search_results;

            is_searching.set(true);
            mb_error.set(None);
            mb_results.set(Vec::new());

            spawn(async move {
                use crate::import::FolderMetadata;
                let metadata = FolderMetadata {
                    artist: Some(artist_val.clone()),
                    album: Some(album_val.clone()),
                    year: year_val,
                    discid: None,
                    mb_discid: None,
                    track_count: None,
                    confidence: 0.0,
                };

                let mut is_searching = album_import_ctx.is_searching_mb;
                let mut mb_error = album_import_ctx.mb_error_message;
                let mut mb_results = album_import_ctx.mb_search_results;

                match album_import_ctx
                    .search_musicbrainz_by_metadata(&metadata)
                    .await
                {
                    Ok(results) => {
                        mb_results.set(results);
                        is_searching.set(false);
                    }
                    Err(e) => {
                        mb_error.set(Some(e));
                        is_searching.set(false);
                    }
                }
            });
        }
    };

    rsx! {
        div { class: "mb-6 bg-white rounded-lg shadow p-6",
            h2 { class: "text-xl font-semibold mb-4", "Search MusicBrainz" }
            div { class: "space-y-4",
                div { class: "flex gap-2",
                    input {
                        class: "flex-1 p-3 border border-gray-300 rounded-lg",
                        placeholder: "Artist",
                        value: "{artist.read()}",
                        oninput: move |event: FormEvent| {
                            artist.set(event.value());
                        },
                        onkeydown: {
                            let album_import_ctx = album_import_ctx.clone();
                            let artist = artist.clone();
                            let album = album.clone();
                            let year = year.clone();
                            move |event: KeyboardEvent| {
                                if event.key() == Key::Enter {
                                    let artist_val = artist.read().clone();
                                    let album_val = album.read().clone();
                                    let year_val = *year.read();

                                    if !artist_val.is_empty() && !album_val.is_empty() {
                                        let album_import_ctx = album_import_ctx.clone();
                                        let mut is_searching = album_import_ctx.is_searching_mb;
                                        let mut mb_error = album_import_ctx.mb_error_message;
                                        let mut mb_results = album_import_ctx.mb_search_results;

                                        is_searching.set(true);
                                        mb_error.set(None);
                                        mb_results.set(Vec::new());

                                        spawn(async move {
                                            use crate::import::FolderMetadata;
                                            let metadata = FolderMetadata {
                                                artist: Some(artist_val.clone()),
                                                album: Some(album_val.clone()),
                                                year: year_val,
                                                discid: None,
                                                mb_discid: None,
                                                track_count: None,
                                                confidence: 0.0,
                                            };

                                            let mut is_searching = album_import_ctx.is_searching_mb;
                                            let mut mb_error = album_import_ctx.mb_error_message;
                                            let mut mb_results = album_import_ctx.mb_search_results;

                                            match album_import_ctx.search_musicbrainz_by_metadata(&metadata).await {
                                                Ok(results) => {
                                                    mb_results.set(results);
                                                    is_searching.set(false);
                                                }
                                                Err(e) => {
                                                    mb_error.set(Some(e));
                                                    is_searching.set(false);
                                                }
                                            }
                                        });
                                    }
                                }
                            }
                        },
                    }
                    input {
                        class: "flex-1 p-3 border border-gray-300 rounded-lg",
                        placeholder: "Album",
                        value: "{album.read()}",
                        oninput: move |event: FormEvent| {
                            album.set(event.value());
                        },
                        onkeydown: {
                            let album_import_ctx = album_import_ctx.clone();
                            let artist = artist.clone();
                            let album = album.clone();
                            let year = year.clone();
                            move |event: KeyboardEvent| {
                                if event.key() == Key::Enter {
                                    let artist_val = artist.read().clone();
                                    let album_val = album.read().clone();
                                    let year_val = *year.read();

                                    if !artist_val.is_empty() && !album_val.is_empty() {
                                        let album_import_ctx = album_import_ctx.clone();
                                        let mut is_searching = album_import_ctx.is_searching_mb;
                                        let mut mb_error = album_import_ctx.mb_error_message;
                                        let mut mb_results = album_import_ctx.mb_search_results;

                                        is_searching.set(true);
                                        mb_error.set(None);
                                        mb_results.set(Vec::new());

                                        spawn(async move {
                                            use crate::import::FolderMetadata;
                                            let metadata = FolderMetadata {
                                                artist: Some(artist_val.clone()),
                                                album: Some(album_val.clone()),
                                                year: year_val,
                                                discid: None,
                                                mb_discid: None,
                                                track_count: None,
                                                confidence: 0.0,
                                            };

                                            let mut is_searching = album_import_ctx.is_searching_mb;
                                            let mut mb_error = album_import_ctx.mb_error_message;
                                            let mut mb_results = album_import_ctx.mb_search_results;

                                            match album_import_ctx.search_musicbrainz_by_metadata(&metadata).await {
                                                Ok(results) => {
                                                    mb_results.set(results);
                                                    is_searching.set(false);
                                                }
                                                Err(e) => {
                                                    mb_error.set(Some(e));
                                                    is_searching.set(false);
                                                }
                                            }
                                        });
                                    }
                                }
                            }
                        },
                    }
                    input {
                        class: "w-32 p-3 border border-gray-300 rounded-lg",
                        placeholder: "Year (optional)",
                        value: "{year_input.read()}",
                        oninput: move |event: FormEvent| {
                            year_input.set(event.value());
                            if let Ok(y) = event.value().parse::<u32>() {
                                if y >= 1900 && y <= 2100 {
                                    year.set(Some(y));
                                } else {
                                    year.set(None);
                                }
                            } else if event.value().is_empty() {
                                year.set(None);
                            } else {
                                year.set(None);
                            }
                        },
                        onkeydown: {
                            let album_import_ctx = album_import_ctx.clone();
                            let artist = artist.clone();
                            let album = album.clone();
                            let year = year.clone();
                            move |event: KeyboardEvent| {
                                if event.key() == Key::Enter {
                                    let artist_val = artist.read().clone();
                                    let album_val = album.read().clone();
                                    let year_val = *year.read();

                                    if !artist_val.is_empty() && !album_val.is_empty() {
                                        let album_import_ctx = album_import_ctx.clone();
                                        let mut is_searching = album_import_ctx.is_searching_mb;
                                        let mut mb_error = album_import_ctx.mb_error_message;
                                        let mut mb_results = album_import_ctx.mb_search_results;

                                        is_searching.set(true);
                                        mb_error.set(None);
                                        mb_results.set(Vec::new());

                                        spawn(async move {
                                            use crate::import::FolderMetadata;
                                            let metadata = FolderMetadata {
                                                artist: Some(artist_val.clone()),
                                                album: Some(album_val.clone()),
                                                year: year_val,
                                                discid: None,
                                                mb_discid: None,
                                                track_count: None,
                                                confidence: 0.0,
                                            };

                                            let mut is_searching = album_import_ctx.is_searching_mb;
                                            let mut mb_error = album_import_ctx.mb_error_message;
                                            let mut mb_results = album_import_ctx.mb_search_results;

                                            match album_import_ctx.search_musicbrainz_by_metadata(&metadata).await {
                                                Ok(results) => {
                                                    mb_results.set(results);
                                                    is_searching.set(false);
                                                }
                                                Err(e) => {
                                                    mb_error.set(Some(e));
                                                    is_searching.set(false);
                                                }
                                            }
                                        });
                                    }
                                }
                            }
                        },
                    }
                }
                button {
                    class: "w-full px-6 py-3 bg-purple-600 text-white rounded-lg hover:bg-purple-700 font-medium disabled:opacity-50",
                    disabled: *is_searching.read() || artist.read().is_empty() || album.read().is_empty(),
                    onclick: on_search_click,
                    if *is_searching.read() {
                        "Searching..."
                    } else {
                        "Search"
                    }
                }
            }
        }
    }
}
