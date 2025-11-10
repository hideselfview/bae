use dioxus::desktop::{Config as DioxusConfig, WindowBuilder};
use dioxus::prelude::*;

use crate::ui::components::import::ImportWorkflowManager;
use crate::ui::components::*;
#[cfg(target_os = "macos")]
use crate::ui::window_activation::setup_macos_window_activation;
use crate::ui::AppContext;

pub const FAVICON: Asset = asset!("/assets/favicon.ico");
pub const MAIN_CSS: Asset = asset!("/assets/main.css");
pub const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Navbar)]
    #[route("/")]
    Library {},
    #[route("/album/:album_id?:release_id")]
    AlbumDetail { 
        album_id: String,
        release_id: String,
    },
    #[route("/import")]
    ImportWorkflowManager {},
    #[route("/settings")]
    Settings {},
}

pub fn make_config() -> DioxusConfig {
    DioxusConfig::default()
        .with_window(make_window())
        // Enable native file drop handler (false = don't disable) to get full file paths
        // On macOS/Linux: Native handler captures paths and merges them with HTML drag events
        // On Windows: Native handler captures paths and uses WindowsDragDrop events to bridge to HTML drag events
        .with_disable_drag_drop_handler(false)
}

fn make_window() -> WindowBuilder {
    WindowBuilder::new()
        .with_title("bae")
        .with_always_on_top(false)
        .with_decorations(true)
        .with_inner_size(dioxus::desktop::LogicalSize::new(1200, 800))
}

pub fn launch_app(context: AppContext) {
    #[cfg(target_os = "macos")]
    setup_macos_window_activation();

    LaunchBuilder::desktop()
        .with_cfg(make_config())
        .with_context_provider(move || Box::new(context.clone()))
        .launch(App);
}
