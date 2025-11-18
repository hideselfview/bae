use crate::ui::Route;
use dioxus::prelude::*;

use super::dialog::GlobalDialog;
use super::queue_sidebar::QueueSidebar;
use super::NowPlayingBar;
use super::TitleBar;

/// Layout component that includes title bar and content
#[component]
pub fn Navbar() -> Element {
    rsx! {
        // On macOS, render custom title bar with native traffic lights
        // On other platforms, use native OS title bar
        {
            #[cfg(target_os = "macos")]
            {
                rsx! { TitleBar {} }
            }
            #[cfg(not(target_os = "macos"))]
            {
                rsx! {}
            }
        }
        Outlet::<Route> {}
        NowPlayingBar {}
        QueueSidebar {}
        GlobalDialog {}
    }
}
