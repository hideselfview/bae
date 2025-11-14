use super::workflow::ImportPage;
use dioxus::prelude::*;

/// Manages the import workflow
#[component]
pub fn ImportWorkflowManager() -> Element {
    rsx! {
        ImportPage {}
    }
}
