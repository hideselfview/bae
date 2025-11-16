use dioxus::prelude::*;

#[component]
pub fn DetectingMetadata(message: String, on_skip: Option<EventHandler<()>>) -> Element {
    rsx! {
        div { class: "text-center py-8",
            p { class: "text-gray-600 mb-4", {message} }
            if let Some(on_skip) = on_skip {
                button {
                    class: "px-4 py-2 bg-gray-200 hover:bg-gray-300 text-gray-800 rounded transition-colors",
                    onclick: move |_| on_skip.call(()),
                    "Skip and search manually"
                }
            }
        }
    }
}
