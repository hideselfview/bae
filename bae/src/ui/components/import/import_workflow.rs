use crate::discogs::DiscogsAlbum;
use crate::import::ImportRequestParams;
use crate::library::use_import_service;
use crate::ui::import_context::ImportContext;
use crate::ui::Route;
use dioxus::prelude::*;
use rfd::AsyncFileDialog;
use std::path::{Path, PathBuf};
use tracing::{error, info};

/// Import workflow functions using the LibraryManager
/// Callback for when user selects a folder for import
pub fn on_folder_selected(folder_path: String) -> Result<(), String> {
    let path = Path::new(&folder_path);

    // Check if path exists and is a directory
    if !path.exists() {
        return Err("Selected path does not exist".to_string());
    }

    if !path.is_dir() {
        return Err("Selected path is not a directory".to_string());
    }

    // Check for audio files
    let audio_extensions = ["mp3", "flac", "wav", "m4a", "aac", "ogg"];
    let mut has_audio_files = false;

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Some(extension) = entry.path().extension() {
                if let Some(ext_str) = extension.to_str() {
                    if audio_extensions.contains(&ext_str.to_lowercase().as_str()) {
                        has_audio_files = true;
                        break;
                    }
                }
            }
        }
    }

    if !has_audio_files {
        return Err("No audio files found in selected folder".to_string());
    }

    info!("Selected folder: {} (contains audio files)", folder_path);
    Ok(())
}

#[derive(Props, PartialEq, Clone)]
pub struct ImportWorkflowProps {
    pub master_id: String,
    pub release_id: Option<String>,
}

#[derive(PartialEq, Clone)]
pub enum WorkflowStep {
    Loading,
    DataSourceSelection,
    ImportError(String),
}

#[component]
pub fn ImportWorkflow(props: ImportWorkflowProps) -> Element {
    let import_service = use_import_service();
    let navigator = use_navigator();
    let import_context = use_context::<ImportContext>();
    let mut current_step = use_signal(|| WorkflowStep::Loading);
    let mut discogs_album = use_signal(|| None::<DiscogsAlbum>);
    let mut selected_folder = use_signal(|| None::<String>);
    let mut folder_error = use_signal(|| None::<String>);

    // Load the album data on mount
    use_effect({
        let master_id = props.master_id.clone();
        let release_id = props.release_id.clone();
        let import_context = import_context.clone();

        move || {
            let master_id = master_id.clone();
            let release_id = release_id.clone();
            let mut import_context = import_context.clone();

            spawn(async move {
                let result = if let Some(release_id) = release_id {
                    import_context.import_release(release_id, master_id).await
                } else {
                    import_context.import_master(master_id).await
                };

                match result {
                    Ok(album) => {
                        discogs_album.set(Some(album));
                        current_step.set(WorkflowStep::DataSourceSelection);
                    }
                    Err(e) => {
                        current_step.set(WorkflowStep::ImportError(e));
                    }
                }
            });
        }
    });

    let mut on_folder_select = move |folder_path: String| {
        selected_folder.set(Some(folder_path));
        folder_error.set(None); // Clear any previous errors
    };

    let mut on_folder_error = move |error: String| {
        folder_error.set(Some(error));
        selected_folder.set(None); // Clear selection on error
    };

    let on_start_import = {
        let import_service = import_service.clone();
        let import_context = import_context.clone();

        move |_| {
            if let (Some(folder), Some(album)) = (
                selected_folder.read().as_ref(),
                discogs_album.read().as_ref(),
            ) {
                let discogs_album = album.clone();
                let import_service = import_service.clone();
                let folder = folder.clone();
                let mut import_context = import_context.clone();

                spawn(async move {
                    info!(
                        "Import started for {} from {}",
                        discogs_album.title(),
                        folder
                    );

                    // Send import request to service (validates and queues)
                    let request = ImportRequestParams::FromFolder {
                        discogs_album: discogs_album.clone(),
                        folder: PathBuf::from(folder),
                    };

                    match import_service.send_request(request).await {
                        Ok((album_id, release_id)) => {
                            info!(
                                "Release queued for import with ID: {} (album: {})",
                                release_id, album_id
                            );

                            // Navigate to album detail page to show import progress for this specific release
                            navigator.push(Route::AlbumDetail {
                                album_id,
                                release_id,
                            });

                            // Reset import state after navigation (so next visit to import page is fresh)
                            import_context.reset();
                        }
                        Err(e) => {
                            error!("Failed to validate/queue import: {}", e);
                            current_step.set(WorkflowStep::ImportError(e));
                        }
                    }
                });
            }
        }
    };

    let on_back = {
        let mut import_context = import_context.clone();
        move |_| {
            import_context.navigate_back();
        }
    };

    let back_button_text = if props.release_id.is_some() {
        "← Back to Releases"
    } else {
        "← Back to Search"
    };

    let current_step_value = current_step.read().clone();

    match current_step_value {
        WorkflowStep::Loading => {
            rsx! {
                div { class: "max-w-4xl mx-auto p-6",
                    div { class: "mb-6",
                        button {
                            class: "text-blue-600 hover:text-blue-800 mb-4",
                            onclick: on_back,
                            "{back_button_text}"
                        }
                        h1 { class: "text-2xl font-bold text-white", "Import Album" }
                    }
                    div { class: "bg-white rounded-lg shadow p-6 text-center",
                        p { class: "text-gray-600", "Loading album details..." }
                    }
                }
            }
        }
        WorkflowStep::DataSourceSelection => {
            let album = discogs_album.read();
            let album_ref = album.as_ref();

            if album_ref.is_none() {
                return rsx! {
                    div { class: "max-w-4xl mx-auto p-6",
                        div { class: "bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded",
                            "Failed to load album data"
                        }
                    }
                };
            }

            let album_data = album_ref.unwrap();

            rsx! {
                div { class: "max-w-4xl mx-auto p-6",
                    // Header
                    div { class: "mb-6",
                        button {
                            class: "text-blue-600 hover:text-blue-800 mb-4",
                            onclick: on_back,
                            "{back_button_text}"
                        }
                        h1 { class: "text-2xl font-bold text-white", "Import Album" }
                    }

                    // Album Info
                    div { class: "bg-white rounded-lg shadow p-6 mb-6",
                        div { class: "flex items-start space-x-4",
                            if let Some(thumb) = album_data.thumb() {
                                img {
                                    class: "w-24 h-24 object-cover rounded",
                                    src: "{thumb}",
                                    alt: "Album cover",
                                }
                            } else {
                                div { class: "w-24 h-24 bg-gray-200 rounded flex items-center justify-center text-gray-500 text-sm",
                                    "No Image"
                                }
                            }
                            div { class: "flex-1",
                                h2 { class: "text-xl font-semibold text-gray-900",
                                    "{album_data.title()}"
                                }
                                if album_data.is_master() {
                                    div { class: "inline-block bg-blue-100 text-blue-800 text-xs px-2 py-1 rounded-full mb-2",
                                        "Master Release"
                                    }
                                }
                                if let Some(year) = album_data.year() {
                                    p { class: "text-gray-600", "Released: {year}" }
                                }
                                if !album_data.format().is_empty() {
                                    p { class: "text-gray-600",
                                        "Format: {album_data.format().join(\", \")}"
                                    }
                                }
                                if !album_data.label().is_empty() {
                                    p { class: "text-gray-600",
                                        "Label: {album_data.label().join(\", \")}"
                                    }
                                }
                                // Discogs links
                                div { class: "flex flex-col gap-1 mt-2 text-sm text-gray-600",
                                    match album_data {
                                        crate::discogs::DiscogsAlbum::Master(master) => {
                                            rsx! {
                                                div {
                                                    "Discogs Album: "
                                                    a {
                                                        href: "https://www.discogs.com/master/{master.id}",
                                                        target: "_blank",
                                                        rel: "noopener noreferrer",
                                                        class: "text-blue-600 hover:text-blue-800 underline",
                                                        "{master.id}"
                                                    }
                                                }
                                                div {
                                                    "Release: None"
                                                }
                                            }
                                        }
                                        crate::discogs::DiscogsAlbum::Release(release) => {
                                            rsx! {
                                                div {
                                                    "Discogs Album: "
                                                    if let Some(master_id) = &release.master_id {
                                                        a {
                                                            href: "https://www.discogs.com/master/{master_id}",
                                                            target: "_blank",
                                                            rel: "noopener noreferrer",
                                                            class: "text-blue-600 hover:text-blue-800 underline",
                                                            "{master_id}"
                                                        }
                                                    } else {
                                                        "None"
                                                    }
                                                }
                                                div {
                                                    "Discogs Release: "
                                                    a {
                                                        href: "https://www.discogs.com/release/{release.id}",
                                                        target: "_blank",
                                                        rel: "noopener noreferrer",
                                                        class: "text-blue-600 hover:text-blue-800 underline",
                                                        "{release.id}"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Data Source Selection
                    div { class: "bg-white rounded-lg shadow p-6",
                        h3 { class: "text-lg font-semibold text-gray-900 mb-4", "Select Data Source" }

                        div { class: "border border-gray-200 rounded-lg p-4",
                            div { class: "flex items-center justify-between",
                                if selected_folder.read().is_none() {
                                    div { class: "text-sm font-medium text-gray-900",
                                        "Select a folder containing your music files"
                                    }
                                }
                                button {
                                    class: "px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700",
                                    onclick: move |_| {
                                        spawn(async move {
                                            if let Some(folder_handle) = AsyncFileDialog::new()
                                                .set_title("Select Music Folder")
                                                .pick_folder()
                                                .await
                                            {
                                                let folder_path = folder_handle.path().to_string_lossy().to_string();
                                                if let Err(e) = on_folder_selected(folder_path.clone()) {
                                                    on_folder_error(e);
                                                } else {
                                                    on_folder_select(folder_path);
                                                }
                                            }
                                        });
                                    },
                                    "Select Folder"
                                }
                            }
                            if let Some(folder) = selected_folder.read().as_ref() {
                                div { class: "mt-2 text-sm text-gray-600", "Selected: {folder}" }
                            }
                            if let Some(error) = folder_error.read().as_ref() {
                                div { class: "mt-2 text-sm text-red-600", "Error: {error}" }
                            }
                        }

                        // Import Button
                        div { class: "mt-6 flex justify-end",
                            button {
                                class: "px-6 py-2 bg-green-600 text-white rounded hover:bg-green-700 disabled:opacity-50",
                                disabled: selected_folder.read().is_none(),
                                onclick: on_start_import,
                                "Start Import"
                            }
                        }
                    }
                }
            }
        }
        WorkflowStep::ImportError(error_msg) => {
            let error_display = error_msg.clone();
            rsx! {
                div { class: "max-w-4xl mx-auto p-6",
                    div { class: "text-center",
                        div { class: "mb-6",
                            div { class: "w-16 h-16 bg-red-100 rounded-full flex items-center justify-center mx-auto mb-4 text-red-600 text-2xl",
                                "✕"
                            }
                            h1 { class: "text-2xl font-bold text-gray-900 mb-2", "Import Failed" }
                            p { class: "text-gray-600 mb-4",
                                "An error occurred while importing the album."
                            }
                            div { class: "bg-red-50 border border-red-200 rounded-lg p-4 mb-6 text-left max-w-2xl mx-auto",
                                p { class: "text-sm font-medium text-red-800 mb-1",
                                    "Error Details:"
                                }
                                p { class: "text-sm text-red-700 font-mono break-words",
                                    "{error_display}"
                                }
                            }
                        }

                        div { class: "space-x-4",
                            button {
                                class: "px-6 py-2 bg-blue-600 text-white rounded hover:bg-blue-700",
                                onclick: move |_| {
                                    current_step.set(WorkflowStep::DataSourceSelection);
                                },
                                "Try Again"
                            }
                            button {
                                class: "px-6 py-2 bg-gray-600 text-white rounded hover:bg-gray-700",
                                onclick: on_back,
                                "{back_button_text.trim_start_matches(\"← \")}"
                            }
                        }
                    }
                }
            }
        }
    }
}
