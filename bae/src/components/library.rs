use dioxus::prelude::*;

/// Library browser page
#[component]
pub fn Library() -> Element {
    rsx! {
        div {
            class: "container mx-auto p-6",
            h1 { 
                class: "text-3xl font-bold mb-6",
                "Music Library" 
            }
            p { 
                class: "text-gray-600",
                "Your music library will appear here." 
            }
        }
    }
}
