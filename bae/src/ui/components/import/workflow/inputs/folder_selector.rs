use dioxus::prelude::*;
use rfd::AsyncFileDialog;
use std::path::Path;

#[component]
pub fn FolderSelector(on_select: EventHandler<String>, on_error: EventHandler<String>) -> Element {
    let mut is_dragging = use_signal(|| false);
    let mut drag_counter = use_signal(|| 0i32);

    let on_drag_enter = move |evt: dioxus::html::DragEvent| {
        evt.prevent_default();
        let current = *drag_counter.read();
        drag_counter.set(current + 1);
        is_dragging.set(true);
    };

    let on_drag_over = move |evt: dioxus::html::DragEvent| {
        evt.prevent_default();
    };

    let on_drag_leave = move |evt: dioxus::html::DragEvent| {
        evt.prevent_default();
        let current: i32 = *drag_counter.read();
        drag_counter.set(current.saturating_sub(1));
        if *drag_counter.read() == 0 {
            is_dragging.set(false);
        }
    };

    let on_drop = {
        let mut is_dragging = is_dragging;
        let mut drag_counter = drag_counter;
        move |evt: dioxus::html::DragEvent| {
            tracing::info!("Drop event triggered!");
            evt.prevent_default();
            is_dragging.set(false);
            drag_counter.set(0);

            let data = evt.data();
            tracing::info!("Got drag data");
            // Use files() directly on DragData to get full paths from native handler
            // data_transfer().files() only has HTML drag-and-drop data (names only)
            // DragData.files() uses DesktopFileDragEvent which has full paths from native handler
            use dioxus::html::HasFileData;
            let files = data.files();
            tracing::info!("Files count: {}", files.len());
            if files.is_empty() {
                tracing::warn!("No files in drop event");
                on_error.call("No files or folders were dropped.".to_string());
                return;
            }

            // Get the first dropped item
            let first_file = &files[0];
            let first_path = first_file.path();
            tracing::info!("First file path: {}", first_path.display());

            // The path from drag-and-drop might be just the name, not an absolute path
            // Try to resolve it to an absolute path
            let path = Path::new(&first_path);

            // If the path is not absolute, try to resolve it
            let resolved_path = if path.is_absolute() {
                tracing::info!("Path is absolute: {}", path.display());
                path.to_path_buf()
            } else {
                tracing::info!("Path is relative, trying to resolve: {}", path.display());
                // Try to canonicalize relative to current directory
                std::env::current_dir()
                    .ok()
                    .and_then(|cwd| {
                        tracing::info!("Current dir: {}", cwd.display());
                        cwd.join(path).canonicalize().ok()
                    })
                    .or_else(|| {
                        // If that fails, the path might just be a name
                        // On macOS, dragged folders might not provide full paths via HTML drag-and-drop
                        // In this case, we can't resolve it - show helpful error
                        tracing::warn!("Failed to resolve relative path");
                        None
                    })
                    .unwrap_or_else(|| {
                        // Fallback: return the original path (might not work, but worth trying)
                        tracing::warn!("Using original path as fallback");
                        path.to_path_buf()
                    })
            };

            tracing::info!(
                "Resolved path: {}, exists: {}, is_dir: {}",
                resolved_path.display(),
                resolved_path.exists(),
                resolved_path.is_dir()
            );

            // Check if resolved path exists and is a directory
            if resolved_path.exists() && resolved_path.is_dir() {
                tracing::info!("Calling on_select with: {}", resolved_path.display());
                on_select.call(resolved_path.to_string_lossy().to_string());
            } else if resolved_path.exists() && resolved_path.is_file() {
                tracing::info!("Dropped item is a file, using parent directory");
                // If it's a file, try to get its parent directory
                if let Some(parent) = resolved_path.parent() {
                    if parent.is_dir() {
                        tracing::info!("Calling on_select with parent: {}", parent.display());
                        on_select.call(parent.to_string_lossy().to_string());
                    } else {
                        tracing::warn!("Parent is not a directory");
                        on_error.call("Please drop a folder, not individual files.".to_string());
                    }
                } else {
                    tracing::warn!("File has no parent directory");
                    on_error.call("Please drop a folder containing your music files.".to_string());
                }
            } else {
                // Path couldn't be resolved - this shouldn't happen if native handler is working correctly
                tracing::error!(
                    "Could not resolve path: {} (exists: {}, is_dir: {})",
                    resolved_path.display(),
                    resolved_path.exists(),
                    resolved_path.is_dir()
                );
                on_error.call("Could not resolve folder path from drag-and-drop. Please use the 'Select Folder' button below.".to_string());
            }
        }
    };

    let on_button_click = move |_| {
        spawn(async move {
            if let Some(folder_handle) = AsyncFileDialog::new()
                .set_title("Select Music Folder")
                .pick_folder()
                .await
            {
                let folder_path = folder_handle.path().to_string_lossy().to_string();
                on_select.call(folder_path);
            }
        });
    };

    let drag_classes = if *is_dragging.read() {
        "border-blue-500 bg-blue-50 border-solid"
    } else {
        "border-gray-300 bg-white border-dashed"
    };

    rsx! {
        div {
            class: "border-2 rounded-lg p-12 transition-all duration-200 {drag_classes}",
            ondragenter: on_drag_enter,
            ondragover: on_drag_over,
            ondragleave: on_drag_leave,
            ondrop: on_drop,
            div { class: "flex flex-col items-center justify-center space-y-6",
                // Icon/visual indicator
                div { class: "w-16 h-16 text-gray-400",
                    svg {
                        xmlns: "http://www.w3.org/2000/svg",
                        fill: "none",
                        view_box: "0 0 24 24",
                        stroke_width: "1.5",
                        stroke: "currentColor",
                        class: "w-full h-full",
                        path {
                            stroke_linecap: "round",
                            stroke_linejoin: "round",
                            d: "M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5m-13.5-9L12 3m0 0l4.5 4.5M12 3v13.5m-7.5 0h15"
                        }
                    }
                }

                // Main text
                div { class: "text-center space-y-2",
                    h3 { class: "text-xl font-semibold text-gray-900",
                        "Select your music folder"
                    }
                    p { class: "text-sm text-gray-600",
                        "Click the button below to choose a folder containing your music files"
                    }
                }

                // Button fallback
                button {
                    class: "px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors font-medium",
                    onclick: on_button_click,
                    "Select Folder"
                }
            }
        }
    }
}
