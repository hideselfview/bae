use dioxus::prelude::*;
use crate::{Route, FAVICON, MAIN_CSS, TAILWIND_CSS};
use crate::album_import_context::AlbumImportContextProvider;

#[component]
pub fn App() -> Element {
    println!("App: Rendering app component");
    
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS } 
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        AlbumImportContextProvider {
            Router::<Route> {}
        }
    }
}
