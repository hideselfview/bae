use dioxus::prelude::*;
use crate::{Route, FAVICON, MAIN_CSS, TAILWIND_CSS};
use crate::library_context::{get_library, LibraryContextProvider};
use crate::album_import_context::AlbumImportContextProvider;

#[component]
pub fn App() -> Element {
    println!("App: Rendering app component");
    let library_manager = get_library();
    
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS } 
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        LibraryContextProvider {
            library_manager: library_manager,
            AlbumImportContextProvider {
                Router::<Route> {}
            }
        }
    }
}
