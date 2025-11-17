use dioxus::prelude::*;
use std::path::PathBuf;

#[component]
pub fn SelectedSource(
    title: String,
    path: Signal<String>,
    on_clear: EventHandler<()>,
    children: Option<Element>,
) -> Element {
    let full_path = path.read().clone();
    let path_buf = PathBuf::from(&full_path);

    let display_name = path_buf
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&full_path)
        .to_string();

    // Extract breadcrumb segments (parent directories), skipping the first component (root)
    let mut breadcrumbs = Vec::new();
    if let Some(parent) = path_buf.parent() {
        for component in parent.components().skip(1) {
            if let Some(segment) = component.as_os_str().to_str() {
                breadcrumbs.push(segment.to_string());
            }
        }
    }

    rsx! {
        div { class: "bg-gray-800 rounded-lg shadow p-6",
            div { class: "mb-6 pb-4 border-b border-gray-700",
                div { class: "flex items-start justify-between mb-3",
                    h3 { class: "text-sm font-semibold text-gray-300 uppercase tracking-wide",
                        {title}
                    }
                    button {
                        class: "px-3 py-1 text-sm text-blue-400 hover:text-blue-300 hover:bg-gray-700 rounded-md transition-colors",
                        onclick: move |_| on_clear.call(()),
                        "Clear"
                    }
                }

                // Breadcrumb path
                if !breadcrumbs.is_empty() {
                    div {
                        class: "mb-2 flex flex-wrap items-center gap-1 text-xs text-gray-400",
                        span { class: "text-gray-500", "/" }
                        for segment in breadcrumbs.iter() {
                            span { class: "font-mono", {segment.clone()} }
                            span { class: "text-gray-500 mx-1", "/" }
                        }
                    }
                }

                // Filename badge
                div {
                    class: "inline-flex items-center px-3 py-1.5 bg-blue-900/30 border border-blue-700 rounded-md hover:bg-blue-900/40 transition-colors",
                    p {
                        class: "text-sm text-blue-300 font-medium tracking-tight select-text cursor-text break-words",
                        {display_name}
                    }
                }
            }
            if let Some(children) = children {
                {children}
            }
        }
    }
}
