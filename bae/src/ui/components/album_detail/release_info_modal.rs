use crate::db::{DbAlbum, DbFile, DbRelease};
use crate::library::use_library_manager;
use dioxus::prelude::*;
use tracing::error;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Details,
    Files,
}

/// Modal component with tabs for release details and files
#[component]
pub fn ReleaseInfoModal(album: DbAlbum, release_id: String, on_close: EventHandler<()>) -> Element {
    let mut active_tab = use_signal(|| Tab::Details);
    let library_manager = use_library_manager();

    // Fetch release data
    let release = use_signal(|| None::<DbRelease>);
    let files = use_signal(Vec::<DbFile>::new);
    let is_loading_files = use_signal(|| false);
    let error_message = use_signal(|| None::<String>);

    // Fetch release details
    use_effect({
        let release_id_clone = release_id.clone();
        let library_manager_clone = library_manager.clone();
        let mut release_signal = release;
        let album_id = album.id.clone();

        move || {
            let release_id = release_id_clone.clone();
            let library_manager = library_manager_clone.clone();
            let album_id = album_id.clone();
            spawn(async move {
                match library_manager
                    .get()
                    .get_releases_for_album(&album_id)
                    .await
                {
                    Ok(releases) => {
                        if let Some(rel) = releases.into_iter().find(|r| r.id == release_id) {
                            release_signal.set(Some(rel));
                        }
                    }
                    Err(e) => {
                        error!("Failed to load release: {}", e);
                    }
                }
            });
        }
    });

    // Fetch files when Files tab is active
    use_effect({
        let release_id_clone = release_id.clone();
        let library_manager_clone = library_manager.clone();
        let mut files_signal = files;
        let mut is_loading_signal = is_loading_files;
        let mut error_message_signal = error_message;
        let tab = *active_tab.read();

        move || {
            if tab == Tab::Files {
                let release_id = release_id_clone.clone();
                let library_manager = library_manager_clone.clone();
                spawn(async move {
                    is_loading_signal.set(true);
                    error_message_signal.set(None);

                    match library_manager
                        .get()
                        .get_files_for_release(&release_id)
                        .await
                    {
                        Ok(mut release_files) => {
                            release_files
                                .sort_by(|a, b| a.original_filename.cmp(&b.original_filename));
                            files_signal.set(release_files);
                            is_loading_signal.set(false);
                        }
                        Err(e) => {
                            error!("Failed to load files: {}", e);
                            error_message_signal.set(Some(format!("Failed to load files: {}", e)));
                            is_loading_signal.set(false);
                        }
                    }
                });
            }
        }
    });

    let current_tab = *active_tab.read();

    rsx! {
        // Modal overlay
        div {
            class: "fixed inset-0 bg-black/50 flex items-center justify-center z-50",
            onclick: move |_| on_close.call(()),

            // Modal content
            div {
                class: "bg-gray-800 rounded-lg shadow-xl max-w-2xl w-full mx-4 max-h-[80vh] flex flex-col",
                onclick: move |e| e.stop_propagation(),

                // Header with tabs
                div { class: "border-b border-gray-700",
                    div { class: "flex items-center justify-between px-6 pt-6 pb-4",
                        h2 { class: "text-xl font-bold text-white", "Release Info" }
                        button {
                            class: "text-gray-400 hover:text-white transition-colors",
                            onclick: move |_| on_close.call(()),
                            "✕"
                        }
                    }

                    // Tabs
                    div { class: "flex px-6",
                        button {
                            class: if current_tab == Tab::Details {
                                "px-4 py-2 text-sm font-medium text-white border-b-2 border-blue-500"
                            } else {
                                "px-4 py-2 text-sm font-medium text-gray-400 hover:text-white border-b-2 border-transparent"
                            },
                            onclick: move |_| active_tab.set(Tab::Details),
                            "Details"
                        }
                        button {
                            class: if current_tab == Tab::Files {
                                "px-4 py-2 text-sm font-medium text-white border-b-2 border-blue-500"
                            } else {
                                "px-4 py-2 text-sm font-medium text-gray-400 hover:text-white border-b-2 border-transparent"
                            },
                            onclick: move |_| active_tab.set(Tab::Files),
                            "Files"
                        }
                    }
                }

                // Tab content
                div { class: "p-6 overflow-y-auto flex-1",
                    if current_tab == Tab::Details {
                        DetailsTab {
                            album: album.clone(),
                            release: release().clone(),
                        }
                    } else {
                        FilesTab {
                            files,
                            is_loading: is_loading_files,
                            error_message,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn DetailsTab(album: DbAlbum, release: Option<DbRelease>) -> Element {
    if let Some(release) = release {
        rsx! {
            div { class: "space-y-4",

                // Release year and format
                if release.year.is_some() || release.format.is_some() {
                    div {
                        if let Some(year) = release.year {
                            span { class: "text-gray-300", "{year}" }
                            if release.format.is_some() {
                                span { class: "text-gray-300", " " }
                            }
                        }
                        if let Some(ref format) = release.format {
                            span { class: "text-gray-300", "{format}" }
                        }
                    }
                }

                // Label and catalog number
                if release.label.is_some() || release.catalog_number.is_some() {
                    div { class: "text-sm text-gray-400",
                        if let Some(ref label) = release.label {
                            span { "{label}" }
                            if release.catalog_number.is_some() {
                                span { " • " }
                            }
                        }
                        if let Some(ref catalog) = release.catalog_number {
                            span { "{catalog}" }
                        }
                    }
                }

                // Country
                if let Some(ref country) = release.country {
                    div { class: "text-sm text-gray-400",
                        span { "{country}" }
                    }
                }

                // Barcode
                if let Some(ref barcode) = release.barcode {
                    div { class: "text-sm text-gray-400",
                        span { class: "font-medium", "Barcode: " }
                        span { class: "font-mono", "{barcode}" }
                    }
                }

                // External links
                div { class: "pt-4 border-t border-gray-700 space-y-2",
                    // MusicBrainz link
                    if let Some(ref mb_release) = album.musicbrainz_release {
                        a {
                            href: "https://musicbrainz.org/release/{mb_release.release_id}",
                            target: "_blank",
                            class: "flex items-center gap-2 text-sm text-blue-400 hover:text-blue-300 transition-colors",
                            span { "🔗" }
                            span { "View on MusicBrainz" }
                        }
                    }

                    // Discogs link
                    if let Some(ref discogs) = album.discogs_release {
                        a {
                            href: "https://www.discogs.com/release/{discogs.release_id}",
                            target: "_blank",
                            class: "flex items-center gap-2 text-sm text-blue-400 hover:text-blue-300 transition-colors",
                            span { "🔗" }
                            span { "View on Discogs" }
                        }
                    }
                }
            }
        }
    } else {
        rsx! {
            div { class: "text-gray-400 text-center py-8",
                "Loading release details..."
            }
        }
    }
}

#[component]
fn FilesTab(
    files: ReadOnlySignal<Vec<DbFile>>,
    is_loading: ReadOnlySignal<bool>,
    error_message: ReadOnlySignal<Option<String>>,
) -> Element {
    let files = files();
    let is_loading = is_loading();
    let error_message = error_message();
    rsx! {
        if is_loading {
            div {
                class: "text-gray-400 text-center py-8",
                "Loading files..."
            }
        } else if let Some(ref error) = error_message {
            div {
                class: "text-red-400 text-center py-8",
                {error.clone()}
            }
        } else if files.is_empty() {
            div {
                class: "text-gray-400 text-center py-8",
                "No files found"
            }
        } else {
            div {
                class: "space-y-2",
                for file in files.iter() {
                    div {
                        class: "flex items-center justify-between py-2 px-3 bg-gray-700/50 rounded hover:bg-gray-700 transition-colors",
                        div {
                            class: "flex-1",
                            div {
                                class: "text-white text-sm font-medium",
                                {file.original_filename.clone()}
                            }
                            div {
                                class: "text-gray-400 text-xs mt-1",
                                {format!("{} • {}", format_file_size(file.file_size), file.format)}
                            }
                        }
                    }
                }
            }
        }
    }
}

fn format_file_size(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
