use dioxus::prelude::*;
use crate::Route;

/// Home page
#[component]
pub fn Home() -> Element {
    rsx! {
        div {
            class: "container mx-auto p-6",
            div {
                class: "text-center py-12",
                h1 {
                    class: "text-4xl font-bold mb-4",
                    "Welcome to bae"
                }
                p {
                    class: "text-xl text-gray-600 mb-8",
                    "Your personal music library manager"
                }
                div {
                    class: "flex justify-center space-x-4",
                    Link {
                        to: Route::AlbumSearchManager {},
                        class: "bg-blue-500 text-white px-6 py-3 rounded-lg hover:bg-blue-600 transition-colors",
                        "Import Albums"
                    }
                    Link {
                        to: Route::Library {},
                        class: "bg-gray-500 text-white px-6 py-3 rounded-lg hover:bg-gray-600 transition-colors",
                        "Browse Library"
                    }
                }
            }
            
            div {
                class: "grid grid-cols-1 md:grid-cols-3 gap-8 mt-12",
                div {
                    class: "text-center p-6",
                    h3 {
                        class: "text-xl font-bold mb-3",
                        "Find & Import"
                    }
                    p {
                        class: "text-gray-600",
                        "Find albums using the Discogs database and add them to your library"
                    }
                }
                div {
                    class: "text-center p-6",
                    h3 {
                        class: "text-xl font-bold mb-3",
                        "Import & Organize"
                    }
                    p {
                        class: "text-gray-600",
                        "Import your music collection from local files or remote sources"
                    }
                }
                div {
                    class: "text-center p-6",
                    h3 {
                        class: "text-xl font-bold mb-3",
                        "Stream & Enjoy"
                    }
                    p {
                        class: "text-gray-600",
                        "Access your music anywhere with built-in streaming capabilities"
                    }
                }
            }
        }
    }
}
