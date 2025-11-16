use dioxus::prelude::*;

#[component]
pub fn SelectedSource(
    title: String,
    path: Signal<String>,
    on_clear: EventHandler<()>,
    children: Option<Element>,
) -> Element {
    rsx! {
        div { class: "bg-white rounded-lg shadow p-6",
            div { class: "mb-6 pb-4 border-b border-gray-200",
                div { class: "flex items-start justify-between mb-3",
                    h3 { class: "text-sm font-semibold text-gray-700 uppercase tracking-wide",
                        {title}
                    }
                    button {
                        class: "px-3 py-1 text-sm text-blue-600 hover:text-blue-800 hover:bg-blue-50 rounded-md transition-colors",
                        onclick: move |_| on_clear.call(()),
                        "Clear"
                    }
                }
                div { class: "inline-block px-4 py-2 bg-gray-100 hover:bg-gray-200 rounded-full border border-gray-300 transition-colors",
                    p {
                        class: "text-sm text-gray-900 font-mono select-text cursor-text break-all",
                        {path.read().clone()}
                    }
                }
            }
            if let Some(children) = children {
                {children}
            }
        }
    }
}
