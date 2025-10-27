use dioxus::prelude::*;

/// Loading spinner for album detail page
#[component]
pub fn AlbumDetailLoading() -> Element {
    rsx! {
        div {
            class: "flex justify-center items-center py-12",
            div {
                class: "animate-spin rounded-full h-12 w-12 border-b-2 border-blue-500"
            }
            p {
                class: "ml-4 text-gray-300",
                "Loading album details..."
            }
        }
    }
}
