use dioxus::prelude::*;

/// Error display for album detail page
#[component]
pub fn AlbumDetailError(message: String) -> Element {
    rsx! {
        div { class: "bg-red-900 border border-red-700 text-red-100 px-4 py-3 rounded mb-4",
            p { "{message}" }
        }
    }
}
