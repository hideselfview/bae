use dioxus::prelude::*;

#[derive(Debug, Clone, PartialEq)]
pub enum SearchSource {
    MusicBrainz,
    Discogs,
}

#[component]
pub fn SearchSourceSelector(
    selected_source: Signal<SearchSource>,
    on_select: EventHandler<SearchSource>,
) -> Element {
    rsx! {
        div { class: "flex gap-4 mb-4",
            label {
                class: "flex items-center gap-2 cursor-pointer",
                input {
                    r#type: "radio",
                    name: "search_source",
                    checked: *selected_source.read() == SearchSource::MusicBrainz,
                    onchange: move |_| {
                        selected_source.set(SearchSource::MusicBrainz);
                        on_select.call(SearchSource::MusicBrainz);
                    }
                }
                span { class: "text-sm font-medium text-gray-700", "MusicBrainz" }
            }
            label {
                class: "flex items-center gap-2 cursor-pointer",
                input {
                    r#type: "radio",
                    name: "search_source",
                    checked: *selected_source.read() == SearchSource::Discogs,
                    onchange: move |_| {
                        selected_source.set(SearchSource::Discogs);
                        on_select.call(SearchSource::Discogs);
                    }
                }
                span { class: "text-sm font-medium text-gray-700", "Discogs" }
            }
        }
    }
}
