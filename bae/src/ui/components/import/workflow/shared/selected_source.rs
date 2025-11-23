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
            div { class: "mb-0 pb-4 border-b border-gray-700",
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
                    class: "flex items-center gap-2 px-3 py-2 bg-gray-900/30 border border-gray-700/40 rounded",
                    // Folder icon
                    svg {
                        xmlns: "http://www.w3.org/2000/svg",
                        fill: "none",
                        view_box: "0 0 24 24",
                        stroke_width: "1.5",
                        stroke: "currentColor",
                        class: "w-4 h-4 text-gray-400 flex-shrink-0",
                        path {
                            stroke_linecap: "round",
                            stroke_linejoin: "round",
                            d: "M2.25 12.75V12A2.25 2.25 0 014.5 9.75h15A2.25 2.25 0 0121.75 12v.75m-8.69-6.44l-2.12-2.12a1.5 1.5 0 00-1.061-.44H4.5A2.25 2.25 0 002.25 6v12a2.25 2.25 0 002.25 2.25h15A2.25 2.25 0 0021.75 18V9a2.25 2.25 0 00-2.25-2.25h-5.379a1.5 1.5 0 01-1.06-.44z"
                        }
                    }
                    p {
                        class: "text-sm text-gray-100 select-text cursor-text break-words",
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
