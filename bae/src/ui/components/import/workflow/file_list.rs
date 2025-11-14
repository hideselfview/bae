use dioxus::prelude::*;

#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub format: String,
}

#[component]
pub fn FileList(files: Vec<FileInfo>) -> Element {
    rsx! {
        if files.is_empty() {
            div { class: "text-gray-400 text-center py-8",
                "No files found"
            }
        } else {
            div { class: "space-y-2",
                for file in files.iter() {
                    div {
                        class: "flex items-center justify-between py-2 px-3 bg-gray-50 rounded hover:bg-gray-100 transition-colors",
                        div {
                            class: "flex-1",
                            div {
                                class: "text-gray-900 text-sm font-medium",
                                {file.name.clone()}
                            }
                            div {
                                class: "text-gray-500 text-xs mt-1",
                                {format!("{} • {}", format_file_size(file.size as i64), file.format)}
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
