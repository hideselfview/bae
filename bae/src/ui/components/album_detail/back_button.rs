use crate::ui::Route;
use dioxus::prelude::*;

/// Back to library navigation button
#[component]
pub fn BackButton() -> Element {
    rsx! {
        div {
            class: "mb-6",
            Link {
                to: Route::Library {},
                class: "inline-flex items-center text-blue-400 hover:text-blue-300 transition-colors",
                "← Back to Library"
            }
        }
    }
}
