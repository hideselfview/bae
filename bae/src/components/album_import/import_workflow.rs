use dioxus::prelude::*;
use crate::models::ImportItem;
use crate::library_context::use_library_manager;
use rfd::AsyncFileDialog;
use std::path::Path;

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

/// Callback for when import process starts - now uses real LibraryManager
pub async fn on_import_started_async(
    item: &ImportItem,
    folder_path: &str,
    library_manager: &crate::library_context::SharedLibraryManager,
) -> Result<String, String> {
    println!("Starting real import for {} from folder: {}", item.title(), folder_path);
    
    // Import the album
    let album_id = library_manager.get()
        .import_album(item, Path::new(folder_path))
        .await
        .map_err(|e| format!("Import failed: {}", e))?;
    
    println!("Successfully imported album with ID: {}", album_id);
    Ok(album_id)
}

/// Legacy sync wrapper for the import process
pub fn on_import_started(item: &ImportItem, folder_path: &str) -> Result<(), String> {
    println!("Import started for {} from {}", item.title(), folder_path);
    // The actual async import will be handled in the UI component
    Ok(())
}

/// Callback for when import process completes
pub fn on_import_completed(item: &ImportItem, folder_path: &str) -> Result<(), String> {
    println!("Import completed for {} from folder: {}", item.title(), folder_path);
    Ok(())
}

/// Callback for when import process fails
pub fn on_import_failed(item: &ImportItem, folder_path: &str, error: &str) {
    println!("Import failed for {} from folder: {} - Error: {}", item.title(), folder_path, error);
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
}

#[component]
pub fn ImportWorkflow(props: ImportWorkflowProps) -> Element {
    let library_manager = use_library_manager();
    let current_step = use_signal(|| ImportStep::DataSourceSelection);
    let selected_folder = use_signal(|| None::<String>);
    let import_progress = use_signal(|| 0u8);
    let folder_error = use_signal(|| None::<String>);

    let on_folder_select = {
        let mut selected_folder = selected_folder;
        let mut folder_error = folder_error;
        move |folder_path: String| {
            selected_folder.set(Some(folder_path));
            folder_error.set(None); // Clear any previous errors
        }
    };

    let on_folder_error = {
        let mut folder_error = folder_error;
        let mut selected_folder = selected_folder;
        move |error: String| {
            folder_error.set(Some(error));
            selected_folder.set(None); // Clear selection on error
        }
    };

    let on_start_import = {
        let mut current_step = current_step;
        let mut import_progress = import_progress;
        let item = props.item.clone();
        let library_manager = library_manager.clone();
        move |_| {
            if let Some(folder) = selected_folder.read().as_ref() {
                // Start the actual import process
                if let Err(e) = on_import_started(&item, folder) {
                    on_import_failed(&item, folder, &e);
                    return;
                }
                
                current_step.set(ImportStep::ImportProgress);
                import_progress.set(0);
                
                // Start real import process
                let item_clone = item.clone();
                let folder_clone = folder.clone();
                let library_manager = library_manager.clone();
                spawn(async move {
                    // Update progress to show we're starting
                    import_progress.set(10);
                    
                    // Perform the actual import
                    match on_import_started_async(&item_clone, &folder_clone, &library_manager).await {
                        Ok(album_id) => {
                            import_progress.set(100);
                            println!("Import successful! Album ID: {}", album_id);
                            
                            // Complete the import
                            if let Err(e) = on_import_completed(&item_clone, &folder_clone) {
                                on_import_failed(&item_clone, &folder_clone, &e);
                            } else {
                                current_step.set(ImportStep::ImportComplete);
                            }
                        }
                        Err(e) => {
                            println!("Import failed: {}", e);
                            on_import_failed(&item_clone, &folder_clone, &e);
                            // Stay on the current step to show the error
                        }
                    }
                });
            }
        }
    };

    let on_back_to_search = {
        let on_back = props.on_back.clone();
        move |_| on_back.call(())
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
                                        let mut on_folder_select = on_folder_select.clone();
                                        let mut on_folder_error = on_folder_error.clone();
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
            rsx! {
                div {
                    class: "max-w-4xl mx-auto p-6",
                    div {
                        class: "text-center",
                        h1 {
                            class: "text-2xl font-bold text-gray-900 mb-6",
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
                                        style: "width: {import_progress}%"
                                    }
                                }
                                p {
                                    class: "mt-2 text-sm text-gray-600",
                                    "{import_progress}% Complete"
                                }
                            }
                            
                            div {
                                class: "text-sm text-gray-500",
                                "Processing files and adding to library..."
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
    }
}
