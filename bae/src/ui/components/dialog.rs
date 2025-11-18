use crate::ui::components::dialog_context::DialogContext;
use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;
use std::rc::Rc;

#[component]
pub fn GlobalDialog() -> Element {
    let dialog = use_context::<DialogContext>();
    let import_context = use_context::<Rc<ImportContext>>();
    let dialog_for_cancel = dialog.clone();
    let dialog_for_confirm = dialog.clone();
    let dialog_for_overlay = dialog.clone();
    let import_context_for_confirm = import_context.clone();

    rsx! {
        if *dialog.is_open.read() {
            div {
                class: "fixed inset-0 bg-black/50 flex items-center justify-center z-[40]",
                onclick: move |_| {
                    dialog_for_overlay.hide();
                },
                div {
                    class: "bg-gray-800 rounded-lg p-6 max-w-md w-full mx-4",
                    onclick: move |evt| evt.stop_propagation(),
                    h2 { class: "text-xl font-bold text-white mb-4", "{dialog.title.read()}" }
                    p { class: "text-gray-300 mb-6",
                        "{dialog.message.read()}"
                    }
                    div { class: "flex gap-3 justify-end",
                        button {
                            class: "px-4 py-2 bg-gray-700 hover:bg-gray-600 text-white rounded-lg",
                            onclick: move |_| {
                                dialog_for_cancel.hide();
                            },
                            "{dialog.cancel_label.read()}"
                        }
                        button {
                            class: "px-4 py-2 bg-red-600 hover:bg-red-700 text-white rounded-lg",
                            onclick: move |_| {
                                // Check action_id and call appropriate ImportContext method
                                if let Some(action_id) = dialog_for_confirm.confirm_action_id.read().as_ref() {
                                    match action_id.as_str() {
                                        "switch_import_tab" => {
                                            import_context_for_confirm.confirm_pending_import_source_switch();
                                        }
                                        "switch_torrent_mode" => {
                                            import_context_for_confirm.confirm_pending_torrent_mode_switch();
                                        }
                                        _ => {}
                                    }
                                }
                                dialog_for_confirm.confirm();
                            },
                            "{dialog.confirm_label.read()}"
                        }
                    }
                }
            }
        }
    }
}
