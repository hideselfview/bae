use crate::db::DbFile;
use crate::library::use_library_manager;
use dioxus::prelude::*;
use tracing::error;

/// Modal component displaying the file tree for a release
#[component]
pub fn ViewFilesModal(release_id: String, on_close: EventHandler<()>) -> Element {
    let library_manager = use_library_manager();
    let files = use_signal(Vec::<DbFile>::new);
    let is_loading = use_signal(|| true);
    let error_message = use_signal(|| None::<String>);

    use_effect({
        let release_id_clone = release_id.clone();
        let library_manager_clone = library_manager.clone();
        let mut files_signal = files;
        let mut is_loading_signal = is_loading;
        let mut error_message_signal = error_message;

        move || {
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
                        release_files.sort_by(|a, b| a.original_filename.cmp(&b.original_filename));
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
    });

    rsx! {
        div {
            class: "fixed inset-0 bg-black/50 flex items-center justify-center z-50",
            onclick: move |_| {
                on_close.call(());
            },
            div {
                class: "bg-gray-800 rounded-lg shadow-xl max-w-2xl w-full mx-4 max-h-[80vh] flex flex-col",
                onclick: move |evt| {
                    evt.stop_propagation();
                },
                // Header
                div {
                    class: "flex items-center justify-between p-6 border-b border-gray-700",
                    h2 {
                        class: "text-xl font-semibold text-white",
                        "Files"
                    }
                    button {
                        class: "text-gray-400 hover:text-white",
                        onclick: move |_| on_close.call(()),
                        "✕"
                    }
                }

                // Content
                div {
                    class: "p-6 overflow-y-auto flex-1",
                    if is_loading() {
                        div {
                            class: "text-gray-400 text-center py-8",
                            "Loading files..."
                        }
                    } else if let Some(ref error) = error_message() {
                        div {
                            class: "text-red-400 text-center py-8",
                            {error.clone()}
                        }
                    } else if files().is_empty() {
                        div {
                            class: "text-gray-400 text-center py-8",
                            "No files found"
                        }
                    } else {
                        div {
                            class: "space-y-2",
                            for file in files().iter() {
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
