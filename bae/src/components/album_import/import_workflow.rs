use dioxus::prelude::*;
use crate::models::ImportItem;

/// Stub functions for import workflow callbacks
/// These will be implemented when we add the actual file operations

/// Callback for when user selects a folder for import
pub fn on_folder_selected(folder_path: String) -> Result<(), String> {
    // TODO: Validate folder path and check for audio files
    println!("Selected folder: {}", folder_path);
    Ok(())
}

/// Callback for when import process starts
pub fn on_import_started(item: &ImportItem, folder_path: &str) -> Result<(), String> {
    // TODO: Start actual import process
    println!("Starting import for {:?} from folder: {}", item.title(), folder_path);
    println!("Master ID: {:?}", item.master_id());
    println!("Tracklist: {:?}", item.tracklist());
    Ok(())
}

/// Callback for when import process completes
pub fn on_import_completed(item: &ImportItem, folder_path: &str) -> Result<(), String> {
    // TODO: Save to database, update library, etc.
    println!("Import completed for {:?} from folder: {}", item.title(), folder_path);
    Ok(())
}

/// Callback for when import process fails
pub fn on_import_failed(item: &ImportItem, folder_path: &str, error: &str) {
    // TODO: Handle import failure, show error to user
    println!("Import failed for {:?} from folder: {} - Error: {}", item.title(), folder_path, error);
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
    let current_step = use_signal(|| ImportStep::DataSourceSelection);
    let selected_folder = use_signal(|| None::<String>);
    let import_progress = use_signal(|| 0u8);

    let mut on_folder_select = {
        let mut selected_folder = selected_folder;
        move |folder_path: String| {
            selected_folder.set(Some(folder_path));
        }
    };

    let on_start_import = {
        let mut current_step = current_step;
        let mut import_progress = import_progress;
        let item = props.item.clone();
        move |_| {
            if let Some(folder) = selected_folder.read().as_ref() {
                // Start the actual import process
                if let Err(e) = on_import_started(&item, folder) {
                    on_import_failed(&item, folder, &e);
                    return;
                }
                
                current_step.set(ImportStep::ImportProgress);
                import_progress.set(0);
                
                // Simulate import progress with a simple timer
                let item_clone = item.clone();
                let folder_clone = folder.clone();
                spawn(async move {
                    for i in 1..=20 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        import_progress.set((i * 5) as u8);
                    }
                    
                    // Complete the import
                    if let Err(e) = on_import_completed(&item_clone, &folder_clone) {
                        on_import_failed(&item_clone, &folder_clone, &e);
                    } else {
                        current_step.set(ImportStep::ImportComplete);
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
                            class: "text-2xl font-bold text-gray-900",
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
                                    class: "w-24 h-24 bg-gray-200 rounded flex items-center justify-center",
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
                                div {
                                    class: "text-sm font-medium text-gray-900",
                                    "Select a folder containing your music files"
                                }
                                button {
                                    class: "px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700",
                                    onclick: move |_| {
                                        // TODO: Implement actual folder picker
                                        let folder_path = "/Users/dima/Music/Example Album".to_string();
                                        if let Err(e) = on_folder_selected(folder_path.clone()) {
                                            println!("Folder selection error: {}", e);
                                        } else {
                                            on_folder_select(folder_path);
                                        }
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
