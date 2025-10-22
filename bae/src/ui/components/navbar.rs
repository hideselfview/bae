use crate::Route;
use dioxus::prelude::*;

/// Shared navbar component.
#[component]
pub fn Navbar() -> Element {
    rsx! {
        div {
            id: "navbar",
            class: "bg-gray-800 text-white p-4 flex space-x-6",
            Link {
                to: Route::Library {},
                class: "hover:text-blue-300 transition-colors",
                "Library"
            }
            Link {
                to: Route::ImportWorkflowManager {},
                class: "hover:text-blue-300 transition-colors",
                "Import"
            }
            Link {
                to: Route::Settings {},
                class: "hover:text-blue-300 transition-colors",
                "Settings"
            }
        }

        Outlet::<Route> {}
    }
}
