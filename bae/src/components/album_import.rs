use dioxus::prelude::*;

/// Album import page
#[component]
pub fn AlbumImport() -> Element {
    rsx! {
        div {
            class: "container mx-auto p-6",
            h1 { 
                class: "text-3xl font-bold mb-6",
                "Import Album" 
            }
            p { 
                class: "text-gray-600",
                "Album import functionality will be implemented here." 
            }
        }
    }
}
