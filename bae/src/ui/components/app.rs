use crate::ui::import_context::AlbumImportContextProvider;
use crate::ui::{Route, FAVICON, MAIN_CSS, TAILWIND_CSS};
use dioxus::prelude::*;
use tracing::debug;

use super::playback_hooks::PlaybackStateProvider;
use super::queue_sidebar::QueueSidebarState;

#[component]
pub fn App() -> Element {
    debug!("Rendering app component");

    let queue_sidebar_state = QueueSidebarState {
        is_open: use_signal(|| false),
    };
    use_context_provider(|| queue_sidebar_state.clone());

    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        PlaybackStateProvider {
            AlbumImportContextProvider {
                div { class: "pb-24", Router::<Route> {} }
            }
        }
    }
}
