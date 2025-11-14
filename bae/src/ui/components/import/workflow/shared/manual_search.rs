use super::super::manual_search_panel::ManualSearchPanel;
use crate::import::{FolderMetadata, MatchCandidate};
use dioxus::prelude::*;

#[component]
pub fn ManualSearch(
    detected_metadata: ReadSignal<Option<FolderMetadata>>,
    selected_match_index: ReadSignal<Option<usize>>,
    on_match_select: EventHandler<usize>,
    on_confirm: EventHandler<MatchCandidate>,
) -> Element {
    // Convert ReadSignal to Signal for ManualSearchPanel
    let detected_metadata_signal = use_signal(|| detected_metadata.read().clone());
    let selected_index_signal = use_signal(|| *selected_match_index.read());

    use_effect({
        let mut detected_metadata_signal = detected_metadata_signal;
        let detected_metadata = detected_metadata;
        move || {
            detected_metadata_signal.set(detected_metadata.read().clone());
        }
    });

    use_effect({
        let mut selected_index_signal = selected_index_signal;
        let selected_match_index = selected_match_index;
        move || {
            selected_index_signal.set(*selected_match_index.read());
        }
    });

    rsx! {
        ManualSearchPanel {
            detected_metadata: detected_metadata_signal,
            on_match_select: on_match_select,
            on_confirm: on_confirm,
            selected_index: selected_index_signal,
        }
    }
}
