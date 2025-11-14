use super::match_item::MatchItem;
use crate::import::MatchCandidate;
use dioxus::prelude::*;

#[component]
pub fn MatchList(
    candidates: Vec<MatchCandidate>,
    selected_index: Option<usize>,
    on_select: EventHandler<usize>,
) -> Element {
    if candidates.is_empty() {
        return rsx! {
            div { class: "bg-white rounded-lg shadow p-6",
                p { class: "text-gray-600 text-center", "No matches found. Try selecting a different folder or search manually." }
            }
        };
    }

    rsx! {
        div { class: "bg-white rounded-lg shadow p-6",
            h3 { class: "text-lg font-semibold text-gray-900 mb-2", "Possible matches" }
            p { class: "text-sm text-gray-500 mb-4", "Select a release continue" }

            div { class: "space-y-3",
                for (index, candidate) in candidates.iter().enumerate() {
                    MatchItem {
                        candidate: candidate.clone(),
                        is_selected: selected_index == Some(index),
                        on_select: move |_| on_select.call(index),
                    }
                }
            }
        }
    }
}
