use crate::ui::import_context::AlbumImportContextProvider;
use crate::ui::window_activation::setup_transparent_titlebar;
use crate::ui::{Route, FAVICON, MAIN_CSS, TAILWIND_CSS};
use dioxus::prelude::*;
use tracing::debug;

use super::library_search_context::LibrarySearchContextProvider;
use super::playback_hooks::PlaybackStateProvider;
use super::queue_sidebar::QueueSidebarState;

#[component]
pub fn App() -> Element {
    debug!("Rendering app component");

    let queue_sidebar_state = QueueSidebarState {
        is_open: use_signal(|| false),
    };
    use_context_provider(|| queue_sidebar_state.clone());

    // Setup transparent titlebar on macOS after window is created
    use_effect(move || {
        setup_transparent_titlebar();
    });

    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        PlaybackStateProvider {
            AlbumImportContextProvider {
                LibrarySearchContextProvider {
                    div {
                        // On macOS: pt-10 accounts for custom titlebar
                        // On other platforms: no extra padding needed (OS handles titlebar)
                        class: if cfg!(target_os = "macos") {
                            "pb-24 pt-10 h-screen overflow-y-auto"
                        } else {
                            "pb-24 h-screen overflow-y-auto"
                        },
                        Router::<Route> {}
                    }
                }
            }
        }
    }
}
