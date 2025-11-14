use crate::ui::Route;
use dioxus::prelude::*;

#[component]
pub fn ErrorDisplay(
    error_message: ReadSignal<Option<String>>,
    duplicate_album_id: ReadSignal<Option<String>>,
) -> Element {
    let navigator = use_navigator();

    if let Some(ref error) = error_message.read().as_ref() {
        rsx! {
            div { class: "bg-red-50 border border-red-200 rounded-lg p-4",
                p {
                    class: "text-sm text-red-700 select-text break-words font-mono",
                    "Error: {error}"
                }
                {
                    let dup_id_opt = duplicate_album_id.read().clone();
                    if let Some(dup_id) = dup_id_opt {
                        let dup_id_clone = dup_id.clone();
                        rsx! {
                            div { class: "mt-2",
                                a {
                                    href: "#",
                                    class: "text-sm text-blue-600 hover:underline",
                                    onclick: move |_| {
                                        navigator.push(Route::AlbumDetail {
                                            album_id: dup_id_clone.clone(),
                                            release_id: String::new(),
                                        });
                                    },
                                    "View existing album"
                                }
                            }
                        }
                    } else {
                        rsx! { div {} }
                    }
                }
            }
        }
    } else {
        rsx! { div {} }
    }
}
