use super::folder_detection::FolderDetectionPage;
use dioxus::prelude::*;

/// Manages the import workflow - simplified to folder-based import only
#[component]
pub fn ImportWorkflowManager() -> Element {
    rsx! {
        FolderDetectionPage {}
    }
}
