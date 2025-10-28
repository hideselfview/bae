use dioxus::prelude::*;

/// Album art with import progress spinner overlay
#[component]
pub fn AlbumArt(
    title: String,
    cover_url: Option<String>,
    import_progress: ReadOnlySignal<Option<(usize, usize, u8)>>,
) -> Element {
    rsx! {
        div { class: "aspect-square bg-gray-700 rounded-lg flex items-center justify-center overflow-hidden relative",
            if let Some(ref url) = cover_url {
                img {
                    src: "{url}",
                    alt: "Album cover for {title}",
                    class: "w-full h-full object-cover",
                }
            } else {
                div { class: "text-gray-500 text-6xl", "🎵" }
            }

            if let Some((_current, _total, percent)) = import_progress() {
                div { class: "absolute inset-0 bg-black bg-opacity-30 flex items-center justify-center",
                    div { class: "w-[30%] h-[30%] flex flex-col items-center justify-center gap-3",
                        div { class: "w-full aspect-square border-4 border-blue-500 border-t-transparent rounded-full animate-spin" }
                        div { class: "text-white text-sm font-medium whitespace-nowrap",
                            "{percent}%"
                        }
                    }
                }
            }
        }
    }
}
