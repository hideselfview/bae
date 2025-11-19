use crate::library::use_library_manager;
use dioxus::prelude::*;
use tracing::error;

#[component]
pub fn DeleteReleaseDialog(
    release_id: String,
    is_last_release: bool,
    is_deleting: Signal<bool>,
    on_confirm: EventHandler<()>,
    on_cancel: EventHandler<()>,
) -> Element {
    let library_manager = use_library_manager();

    rsx! {
        div {
            class: "fixed inset-0 bg-black/50 flex items-center justify-center z-50",
            onclick: move |_| {
                if !is_deleting() {
                    on_cancel.call(());
                }
            },
            div {
                class: "bg-gray-800 rounded-lg p-6 max-w-md w-full mx-4",
                onclick: move |evt| evt.stop_propagation(),
                h2 { class: "text-xl font-bold text-white mb-4", "Delete Release?" }
                p { class: "text-gray-300 mb-6",
                    "Are you sure you want to delete this release? This will delete all tracks and associated data for this release."
                    if is_last_release {
                        " Since this is the only release, the album will also be deleted."
                    } else {
                        ""
                    }
                }
                div { class: "flex gap-3 justify-end",
                    button {
                        class: "px-4 py-2 bg-gray-700 hover:bg-gray-600 text-white rounded-lg",
                        disabled: is_deleting(),
                        onclick: move |_| {
                            if !is_deleting() {
                                on_cancel.call(());
                            }
                        },
                        "Cancel"
                    }
                    button {
                        class: "px-4 py-2 bg-red-600 hover:bg-red-500 text-white rounded-lg",
                        disabled: is_deleting(),
                        onclick: {
                            let release_id = release_id.clone();
                            let library_manager = library_manager.clone();
                            move |_| {
                                if is_deleting() {
                                    return;
                                }
                                is_deleting.set(true);
                                let release_id = release_id.clone();
                                let library_manager = library_manager.clone();
                                spawn(async move {
                                    match library_manager.get().delete_release(&release_id).await {
                                        Ok(_) => {
                                            is_deleting.set(false);
                                            on_confirm.call(());
                                        }
                                        Err(e) => {
                                            error!("Failed to delete release: {}", e);
                                            is_deleting.set(false);
                                        }
                                    }
                                });
                            }
                        },
                        if is_deleting() {
                            "Deleting..."
                        } else {
                            "Delete"
                        }
                    }
                }
            }
        }
    }
}
