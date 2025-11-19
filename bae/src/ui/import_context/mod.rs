pub mod detection;
pub mod import;
pub mod navigation;
pub mod search;
pub mod state;
pub mod types;

pub use state::ImportContext;
pub use types::ImportPhase;

use crate::config::use_config;
use crate::ui::components::dialog_context::DialogContext;
use crate::ui::AppContext;
use dioxus::prelude::*;
use std::rc::Rc;

/// Provider component to make search context available throughout the app
#[component]
pub fn ImportContextProvider(children: Element) -> Element {
    let config = use_config();

    let app_context = use_context::<AppContext>();
    let dialog = use_context::<DialogContext>();

    let import_ctx = ImportContext::new(
        &config,
        app_context.torrent_manager.clone(),
        app_context.library_manager.clone(),
        app_context.import_handle.clone(),
        dialog,
    );

    use_context_provider(move || Rc::new(import_ctx));

    rsx! {
        {children}
    }
}
