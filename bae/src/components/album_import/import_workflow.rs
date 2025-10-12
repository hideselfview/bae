use crate::import_service::{ImportProgress, ImportRequest};
use crate::library_context::use_import_service;
use crate::models::ImportItem;
use dioxus::prelude::*;
use rfd::AsyncFileDialog;
use std::path::{Path, PathBuf};

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

    println!("Selected folder: {} (contains audio files)", folder_path);
    Ok(())
}

#[derive(Props, PartialEq, Clone)]
pub struct ImportWorkflowProps {
    pub item: ImportItem,
    pub on_back: EventHandler<()>,
}

#[derive(PartialEq, Clone)]
pub enum ImportStep {
    DataSourceSelection,
    ImportProgress,
    ImportComplete,
    ImportError(String),
}

#[component]
pub fn ImportWorkflow(props: ImportWorkflowProps) -> Element {
    let import_service = use_import_service();
    let mut current_step = use_signal(|| ImportStep::DataSourceSelection);
    let mut import_progress = use_signal(|| 0u8);
    let mut selected_folder = use_signal(|| None::<String>);
    let mut folder_error = use_signal(|| None::<String>);

    let mut on_folder_select = move |folder_path: String| {
        selected_folder.set(Some(folder_path));
        folder_error.set(None); // Clear any previous errors
    };

    let mut on_folder_error = move |error: String| {
        folder_error.set(Some(error));
        selected_folder.set(None); // Clear selection on error
    };

    // Poll for progress updates from import service
    let import_service_for_polling = import_service.clone();
    use_effect(move || {
        if *current_step.read() == ImportStep::ImportProgress {
            let import_service_clone = import_service_for_polling.clone();
            spawn(async move {
                loop {
                    // Check for progress updates
                    if let Some(progress) = import_service_clone.try_recv_progress() {
                        match progress {
                            ImportProgress::Started { album_title, .. } => {
                                println!("Import started: {}", album_title);
                                import_progress.set(0);
                            }
                            ImportProgress::ProcessingProgress { percent, .. } => {
                                import_progress.set(percent);
                            }
                            ImportProgress::TrackComplete { .. } => {
                                println!("Track completed");
                            }
                            ImportProgress::Complete { album_id } => {
                                println!("Import completed: {}", album_id);
                                import_progress.set(100);
                                current_step.set(ImportStep::ImportComplete);
                                break;
                            }
                            ImportProgress::Failed { error, .. } => {
                                println!("Import failed: {}", error);
                                current_step.set(ImportStep::ImportError(error));
                                break;
                            }
                        }
                    }

                    // Sleep briefly before next poll
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            });
        }
    });

    let on_start_import = {
        let item = props.item.clone();
        let import_service = import_service.clone();

        move |_| {
            if let Some(folder) = selected_folder.read().as_ref() {
                println!("Import started for {} from {}", item.title(), folder);

                // Send import request to service
                let request = ImportRequest::ImportAlbum {
                    item: item.clone(),
                    folder: PathBuf::from(folder),
                };

                if let Err(e) = import_service.send_request(request) {
                    println!("Failed to send import request: {}", e);
                    current_step.set(ImportStep::ImportError(e));
                    return;
                }

                // Transition to progress screen
                current_step.set(ImportStep::ImportProgress);
                import_progress.set(0);
            }
        }
    };

    let on_back_to_search = move |_| {
        props.on_back.call(());
    };

    let current_step_value = current_step.read().clone();

    match current_step_value {
        ImportStep::DataSourceSelection => {
            rsx! {
                div {
                    class: "max-w-4xl mx-auto p-6",
                    // Header
                    div {
                        class: "mb-6",
                        button {
                            class: "text-blue-600 hover:text-blue-800 mb-4",
                            onclick: on_back_to_search,
                            "← Back to Search"
                        }
                        h1 {
                            class: "text-2xl font-bold text-white",
                            "Import Album"
                        }
                    }

                    // Album Info
                    div {
                        class: "bg-white rounded-lg shadow p-6 mb-6",
                        div {
                            class: "flex items-start space-x-4",
                            if let Some(thumb) = props.item.thumb() {
                                img {
                                    class: "w-24 h-24 object-cover rounded",
                                    src: "{thumb}",
                                    alt: "Album cover"
                                }
                            } else {
                                div {
                                    class: "w-24 h-24 bg-gray-200 rounded flex items-center justify-center text-gray-500 text-sm",
                                    "No Image"
                                }
                            }
                            div {
                                class: "flex-1",
                                h2 {
                                    class: "text-xl font-semibold text-gray-900",
                                    "{props.item.title()}"
                                }
                                if props.item.is_master() {
                                    div {
                                        class: "inline-block bg-blue-100 text-blue-800 text-xs px-2 py-1 rounded-full mb-2",
                                        "Master Release"
                                    }
                                }
                                if let Some(year) = props.item.year() {
                                    p {
                                        class: "text-gray-600",
                                        "Released: {year}"
                                    }
                                }
                                if !props.item.format().is_empty() {
                                    p {
                                        class: "text-gray-600",
                                        "Format: {props.item.format().join(\", \")}"
                                    }
                                }
                                if !props.item.label().is_empty() {
                                    p {
                                        class: "text-gray-600",
                                        "Label: {props.item.label().join(\", \")}"
                                    }
                                }
                            }
                        }
                    }

                    // Data Source Selection
                    div {
                        class: "bg-white rounded-lg shadow p-6",
                        h3 {
                            class: "text-lg font-semibold text-gray-900 mb-4",
                            "Select Data Source"
                        }

                        div {
                            class: "border border-gray-200 rounded-lg p-4",
                            div {
                                class: "flex items-center justify-between",
                                if selected_folder.read().is_none() {
                                    div {
                                        class: "text-sm font-medium text-gray-900",
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
                                div {
                                    class: "mt-2 text-sm text-gray-600",
                                    "Selected: {folder}"
                                }
                            }
                            if let Some(error) = folder_error.read().as_ref() {
                                div {
                                    class: "mt-2 text-sm text-red-600",
                                    "Error: {error}"
                                }
                            }
                        }

                        // Import Button
                        div {
                            class: "mt-6 flex justify-end",
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
        ImportStep::ImportProgress => {
            let progress = *import_progress.read();
            let progress_clamped = progress.clamp(0, 100);
            rsx! {
                div {
                    class: "max-w-4xl mx-auto p-6",
                    div {
                        class: "text-center",
                        h1 {
                            class: "text-2xl font-bold text-white mb-6",
                            "Importing Album"
                        }

                        div {
                            class: "bg-white rounded-lg shadow p-8",
                            div {
                                class: "mb-6",
                                div {
                                    class: "w-full bg-gray-200 rounded-full h-2",
                                    div {
                                        class: "bg-blue-600 h-2 rounded-full transition-all duration-300",
                                        style: "width: {progress_clamped}%"
                                    }
                                }
                                p {
                                    class: "mt-2 text-sm text-gray-600",
                                    "{progress_clamped}% Complete"
                                }
                            }

                            div {
                                class: "text-sm text-gray-500",
                                "Processing files and adding to library..."
                            }
                            div {
                                class: "text-xs text-gray-400 mt-2",
                                "This may take several minutes for large albums."
                            }
                        }
                    }
                }
            }
        }
        ImportStep::ImportComplete => {
            rsx! {
                div {
                    class: "max-w-4xl mx-auto p-6",
                    div {
                        class: "text-center",
                        div {
                            class: "mb-6",
                            div {
                                class: "w-16 h-16 bg-green-100 rounded-full flex items-center justify-center mx-auto mb-4",
                                "✓"
                            }
                            h1 {
                                class: "text-2xl font-bold text-gray-900 mb-2",
                                "Import Complete!"
                            }
                            p {
                                class: "text-gray-600",
                                "Album has been successfully added to your library."
                            }
                        }

                        div {
                            class: "space-x-4",
                            button {
                                class: "px-6 py-2 bg-blue-600 text-white rounded hover:bg-blue-700",
                                onclick: on_back_to_search,
                                "Import Another Album"
                            }
                            button {
                                class: "px-6 py-2 bg-gray-600 text-white rounded hover:bg-gray-700",
                                onclick: move |_| {
                                    // TODO: Navigate to library view
                                    println!("Navigate to library");
                                },
                                "View Library"
                            }
                        }
                    }
                }
            }
        }
        ImportStep::ImportError(error_msg) => {
            let error_display = error_msg.clone();
            rsx! {
                div {
                    class: "max-w-4xl mx-auto p-6",
                    div {
                        class: "text-center",
                        div {
                            class: "mb-6",
                            div {
                                class: "w-16 h-16 bg-red-100 rounded-full flex items-center justify-center mx-auto mb-4 text-red-600 text-2xl",
                                "✕"
                            }
                            h1 {
                                class: "text-2xl font-bold text-gray-900 mb-2",
                                "Import Failed"
                            }
                            p {
                                class: "text-gray-600 mb-4",
                                "An error occurred while importing the album."
                            }
                            div {
                                class: "bg-red-50 border border-red-200 rounded-lg p-4 mb-6 text-left max-w-2xl mx-auto",
                                p {
                                    class: "text-sm font-medium text-red-800 mb-1",
                                    "Error Details:"
                                }
                                p {
                                    class: "text-sm text-red-700 font-mono break-words",
                                    "{error_display}"
                                }
                            }
                        }

                        div {
                            class: "space-x-4",
                            button {
                                class: "px-6 py-2 bg-blue-600 text-white rounded hover:bg-blue-700",
                                onclick: move |_| {
                                    current_step.set(ImportStep::DataSourceSelection);
                                },
                                "Try Again"
                            }
                            button {
                                class: "px-6 py-2 bg-gray-600 text-white rounded hover:bg-gray-700",
                                onclick: on_back_to_search,
                                "Back to Search"
                            }
                        }
                    }
                }
            }
        }
    }
}
