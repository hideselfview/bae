use dioxus::prelude::*;
use dioxus::desktop::{Config, WindowBuilder};

mod models;
mod discogs;
mod api_keys;
mod components;
mod album_import_context;
mod database;
mod library;
mod chunking;
mod encryption;

use components::*;
use components::album_import::ImportWorkflowManager;
use album_import_context::AlbumImportContextProvider;

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[layout(Navbar)]
    #[route("/")]
    Library {},
    #[route("/import")]
    ImportWorkflowManager {},
    #[route("/settings")]
    Settings {},
}

const FAVICON: Asset = asset!("/assets/favicon.ico");
const MAIN_CSS: Asset = asset!("/assets/main.css");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

fn main() {
    LaunchBuilder::desktop()
        .with_cfg(make_config())
        .launch(App);
}

fn make_config() -> Config {
    Config::default().with_window(make_window())
}

fn make_window() -> WindowBuilder {
    WindowBuilder::new()
        .with_title("bae")
        .with_always_on_top(false)
        .with_inner_size(dioxus::desktop::LogicalSize::new(1200, 800))
}

#[component]
fn App() -> Element {
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS } 
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        AlbumImportContextProvider {
            Router::<Route> {}
        }
    }
}