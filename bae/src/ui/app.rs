use dioxus::desktop::{Config as DioxusConfig, WindowBuilder};
use dioxus::prelude::*;

use crate::ui::components::album_import::ImportWorkflowManager;
use crate::ui::components::*;

pub const FAVICON: Asset = asset!("/assets/favicon.ico");
pub const MAIN_CSS: Asset = asset!("/assets/main.css");
pub const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Navbar)]
    #[route("/")]
    Library {},
    #[route("/album/:album_id")]
    AlbumDetail { album_id: String },
    #[route("/import")]
    ImportWorkflowManager {},
    #[route("/settings")]
    Settings {},
}

pub fn make_config() -> DioxusConfig {
    DioxusConfig::default().with_window(make_window())
}

fn make_window() -> WindowBuilder {
    WindowBuilder::new()
        .with_title("bae")
        .with_always_on_top(false)
        .with_inner_size(dioxus::desktop::LogicalSize::new(1200, 800))
}
