use crate::ui::components::dialog_context::DialogContext;
use dioxus::prelude::*;

#[component]
pub fn GlobalDialog() -> Element {
    let dialog = use_context::<DialogContext>();
    let dialog_for_cancel = dialog.clone();
    let dialog_for_confirm = dialog.clone();
    let dialog_for_overlay = dialog.clone();

    rsx! {
        if *dialog.is_open.read() {
            div {
                class: "fixed inset-0 bg-black/50 flex items-center justify-center z-[3000]",
                onclick: move |_| {
                    dialog_for_overlay.hide();
                },
                div {
                    class: "bg-gray-800 rounded-lg p-6 max-w-md w-full mx-4",
                    onclick: move |evt| evt.stop_propagation(),
                    h2 { class: "text-xl font-bold text-white mb-4", "{dialog.title()}" }
                    p { class: "text-gray-300 mb-6",
                        "{dialog.message()}"
                    }
                    div { class: "flex gap-3 justify-end",
                        button {
                            class: "px-4 py-2 bg-gray-700 hover:bg-gray-600 text-white rounded-lg",
                            onclick: move |_| {
                                dialog_for_cancel.hide();
                            },
                            "{dialog.cancel_label()}"
                        }
                        button {
                            class: "px-4 py-2 bg-red-600 hover:bg-red-700 text-white rounded-lg",
                            onclick: move |_| {
                                let callback = dialog_for_confirm.on_confirm();
                                if let Some(callback) = callback {
                                    dialog_for_confirm.hide();
                                    callback();
                                } else {
                                    dialog_for_confirm.hide();
                                }
                            },
                            "{dialog.confirm_label()}"
                        }
                    }
                }
            }
        }
    }
}
