use dioxus::prelude::*;

#[component]
pub fn TextFileModal(filename: String, content: String, on_close: EventHandler<()>) -> Element {
    rsx! {
        div {
            class: "fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50",
            onclick: move |_| on_close.call(()),
            div {
                class: "bg-gray-800 rounded-lg shadow-xl max-w-4xl w-full max-h-[80vh] flex flex-col",
                onclick: move |e| e.stop_propagation(),

                // Header
                div {
                    class: "flex items-center justify-between p-4 border-b border-gray-700",
                    h3 {
                        class: "text-lg font-semibold text-white",
                        {filename}
                    }
                    button {
                        class: "text-gray-400 hover:text-white transition-colors",
                        onclick: move |_| on_close.call(()),
                        "✕"
                    }
                }

                // Content
                div {
                    class: "flex-1 overflow-auto p-4",
                    pre {
                        class: "text-sm text-gray-300 font-mono whitespace-pre-wrap select-text",
                        {content}
                    }
                }
            }
        }
    }
}
