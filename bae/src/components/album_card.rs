use crate::database::{DbAlbum, ImportStatus};
use crate::library_context::use_import_service;
use crate::Route;
use dioxus::prelude::*;

/// Individual album card component
#[component]
pub fn AlbumCard(album: DbAlbum) -> Element {
    let import_service = use_import_service();
    let mut progress_percent = use_signal(|| 0u8);
    let mut import_complete = use_signal(|| false);

    use_effect({
        let album_id = album.id.clone();
        let import_service = import_service.clone();
        let is_importing = album.import_status == ImportStatus::Importing;

        move || {
            if is_importing {
                let import_service = import_service.clone();
                let album_id = album_id.clone();
                spawn(async move {
                    let mut rx = import_service.subscribe_album(album_id);

                    // Await progress updates - filtered to this album only!
                    while let Some(progress) = rx.recv().await {
                        match progress {
                            crate::import::ImportProgress::ProcessingProgress {
                                percent, ..
                            } => {
                                progress_percent.set(percent);
                            }
                            crate::import::ImportProgress::Complete { .. } => {
                                progress_percent.set(100);
                                import_complete.set(true);
                                break;
                            }
                            crate::import::ImportProgress::Failed { .. } => {
                                import_complete.set(true);
                                break;
                            }
                            _ => {}
                        }
                    }
                });
            }
        }
    });

    // Determine visual styling based on import status
    let (card_class, overlay_class, status_badge) = match album.import_status {
        ImportStatus::Complete => (
            "bg-gray-800 rounded-lg overflow-hidden shadow-lg hover:shadow-xl transition-shadow duration-300 cursor-pointer",
            "",
            None
        ),
        ImportStatus::Queued => (
            "bg-gray-800 rounded-lg overflow-hidden shadow-lg hover:shadow-xl transition-shadow duration-300 cursor-pointer relative",
            "absolute inset-0 bg-black bg-opacity-30",
            Some(("Queued", 0u8, "bg-yellow-600"))
        ),
        ImportStatus::Importing => {
            let progress = progress_percent();
            (
                "bg-gray-800 rounded-lg overflow-hidden shadow-lg hover:shadow-xl transition-shadow duration-300 cursor-pointer relative",
                "absolute inset-0 bg-black bg-opacity-50",
                Some(("Importing", progress, "bg-blue-600"))
            )
        },
        ImportStatus::Failed => (
            "bg-gray-800 rounded-lg overflow-hidden shadow-lg hover:shadow-xl transition-shadow duration-300 cursor-pointer relative opacity-75",
            "absolute inset-0 bg-red-900 bg-opacity-30",
            Some(("Failed", 0u8, "bg-red-600"))
        ),
    };

    rsx! {
        div {
            class: "{card_class}",
            onclick: {
                let album_id = album.id.clone();
                let navigator = navigator();
                move |_| {
                    navigator.push(Route::AlbumDetail { album_id: album_id.clone() });
                }
            },

            // Album cover
            div {
                class: "aspect-square bg-gray-700 flex items-center justify-center relative",
                if let Some(cover_url) = &album.cover_art_url {
                    img {
                        src: "{cover_url}",
                        alt: "Album cover for {album.title}",
                        class: "w-full h-full object-cover"
                    }
                } else {
                    div {
                        class: "text-gray-500 text-4xl",
                        "🎵"
                    }
                }

                // Overlay for importing/failed albums
                if !overlay_class.is_empty() {
                    div { class: "{overlay_class}" }
                }

                // Status badge
                if let Some((label, progress, badge_color)) = status_badge {
                    div {
                        class: "absolute top-2 right-2 px-2 py-1 {badge_color} text-white text-xs rounded",
                        "{label}"
                    }
                    // Progress bar for importing albums
                    if progress > 0 {
                        div {
                            class: "absolute bottom-0 left-0 right-0 h-1 bg-gray-800",
                            div {
                                class: "h-full bg-blue-500 transition-all duration-300",
                                style: "width: {progress}%"
                            }
                        }
                    }
                }
            }

            // Album info
            div {
                class: "p-4",
                h3 {
                    class: "font-bold text-white text-lg mb-1 truncate",
                    title: "{album.title}",
                    "{album.title}"
                }
                p {
                    class: "text-gray-400 text-sm truncate",
                    title: "{album.artist_name}",
                    "{album.artist_name}"
                }
                if let Some(year) = album.year {
                    p {
                        class: "text-gray-500 text-xs mt-1",
                        "{year}"
                    }
                }
            }
        }
    }
}
