use super::super::match_list::MatchList;
use crate::import::MatchCandidate;
use dioxus::prelude::*;

#[component]
pub fn ExactLookup(
    is_looking_up: ReadSignal<bool>,
    exact_match_candidates: ReadSignal<Vec<MatchCandidate>>,
    selected_match_index: ReadSignal<Option<usize>>,
    on_select: EventHandler<usize>,
) -> Element {
    if *is_looking_up.read() {
        rsx! {
            div { class: "bg-gray-800 rounded-lg shadow p-6 text-center",
                p { class: "text-gray-400", "Looking up release by DiscID..." }
            }
        }
    } else if !exact_match_candidates.read().is_empty() {
        rsx! {
            div { class: "bg-gray-800 rounded-lg shadow p-6",
                h3 { class: "text-lg font-semibold text-white mb-4", "Multiple Exact Matches Found" }
                p { class: "text-sm text-gray-400 mb-4", "Select the correct release:" }
                div { class: "mt-4",
                MatchList {
                    candidates: exact_match_candidates.read().clone(),
                    selected_index: selected_match_index.read().as_ref().copied(),
                    on_select: move |index| on_select.call(index),
                    }
                }
            }
        }
    } else {
        rsx! { div {} }
    }
}
