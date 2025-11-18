use crate::ui::components::import::TorrentInputMode;
use crate::ui::import_context::ImportContext;
use dioxus::html::KeyboardEvent;
use dioxus::prelude::*;
use rfd::AsyncFileDialog;
use std::path::PathBuf;
use std::rc::Rc;

#[component]
pub fn TorrentInput(
    on_file_select: EventHandler<(PathBuf, bool)>,
    on_magnet_link: EventHandler<(String, bool)>,
    on_error: EventHandler<String>,
    show_seed_checkbox: bool,
) -> Element {
    let import_context = use_context::<Rc<ImportContext>>();
    let magnet_link = import_context.magnet_link();
    let input_mode = import_context.torrent_input_mode();
    let seed_after_download = import_context.seed_after_download();

    let on_file_button_click = {
        let import_context = import_context.clone();
        let on_file_select = on_file_select;
        move |_| {
            let seed_flag = *import_context.seed_after_download().read();
            spawn(async move {
                if let Some(file_handle) = AsyncFileDialog::new()
                    .set_title("Select Torrent File")
                    .add_filter("Torrent", &["torrent"])
                    .pick_file()
                    .await
                {
                    on_file_select.call((file_handle.path().to_path_buf(), seed_flag));
                }
            });
        }
    };

    let on_magnet_submit = {
        let import_context = import_context.clone();
        let on_error = on_error;
        let on_magnet_link = on_magnet_link;
        move |_| {
            let link = import_context.magnet_link().read().trim().to_string();
            if link.is_empty() {
                on_error.call("Please enter a magnet link".to_string());
                return;
            }
            if !link.starts_with("magnet:") {
                on_error.call("Invalid magnet link format".to_string());
                return;
            }
            let seed_flag = *import_context.seed_after_download().read();
            on_magnet_link.call((link, seed_flag));
        }
    };

    let on_file_mode_click = {
        let import_context = import_context.clone();
        move |_| import_context.set_torrent_input_mode(TorrentInputMode::File)
    };

    let on_magnet_mode_click = {
        let import_context = import_context.clone();
        move |_| import_context.set_torrent_input_mode(TorrentInputMode::Magnet)
    };

    let import_context_for_input = import_context.clone();
    let import_context_for_seed = import_context.clone();
    let on_magnet_submit_for_keydown = on_magnet_submit.clone();
    let on_magnet_keydown = {
        move |evt: KeyboardEvent| {
            if evt.key() == dioxus::html::Key::Enter {
                on_magnet_submit_for_keydown(());
            }
        }
    };

    rsx! {
        div { class: "space-y-4",
            // Mode selector
            div { class: "mb-3",
                div { class: "flex space-x-1 border-b border-gray-600",
                    button {
                        class: if *input_mode.read() == TorrentInputMode::File {
                            "px-3 py-1.5 text-sm font-medium transition-colors text-blue-400 border-b-2 border-blue-400"
                        } else {
                            "px-3 py-1.5 text-sm font-medium transition-colors text-gray-400 hover:text-gray-300"
                        },
                        onclick: on_file_mode_click,
                        "Torrent File"
                    }
                    button {
                        class: if *input_mode.read() == TorrentInputMode::Magnet {
                            "px-3 py-1.5 text-sm font-medium transition-colors text-blue-400 border-b-2 border-blue-400"
                        } else {
                            "px-3 py-1.5 text-sm font-medium transition-colors text-gray-400 hover:text-gray-300"
                        },
                        onclick: on_magnet_mode_click,
                        "Magnet Link"
                    }
                }
            }

            // File input mode
            if *input_mode.read() == TorrentInputMode::File {
                div { class: "border-2 border-dashed border-gray-600 rounded-lg p-10 text-center",
                    div { class: "space-y-4",
                        svg {
                            xmlns: "http://www.w3.org/2000/svg",
                            fill: "none",
                            view_box: "0 0 24 24",
                            stroke_width: "1.5",
                            stroke: "currentColor",
                            class: "w-16 h-16 mx-auto text-gray-400",
                            path {
                                stroke_linecap: "round",
                                stroke_linejoin: "round",
                                d: "M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m0 12.75h7.5m-7.5 3H12M10.5 2.25H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z"
                            }
                        }
                        div {
                            h3 { class: "text-lg font-semibold text-gray-200 mb-2",
                                "Select a torrent file"
                            }
                            p { class: "text-sm text-gray-400 mb-4",
                                "Choose a .torrent file from your computer"
                            }
                            button {
                                class: "px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors font-medium",
                                onclick: on_file_button_click,
                                "Browse Files"
                            }
                        }
                    }
                }
            }

            // Magnet link input mode
            if *input_mode.read() == TorrentInputMode::Magnet {
                div { class: "space-y-3",
                    div {
                        label { class: "block text-sm font-medium text-gray-300 mb-2",
                            "Magnet Link"
                        }
                        div { class: "flex space-x-2",
                            input {
                                class: "flex-1 px-4 py-2 border border-gray-600 bg-gray-700 text-white rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500",
                                r#type: "text",
                                placeholder: "magnet:?xt=urn:btih:...",
                                value: "{magnet_link}",
                                oninput: move |evt| {
                                    import_context_for_input.set_magnet_link(evt.value());
                                },
                                onkeydown: on_magnet_keydown,
                            }
                            button {
                                class: "px-6 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors font-medium",
                                onclick: move |_| on_magnet_submit(()),
                                "Import"
                            }
                        }
                    }
                    p { class: "text-xs text-gray-400",
                        "Paste a magnet link to start downloading"
                    }
                }
            }

            // Seed after download option (only show when a torrent is selected)
            if show_seed_checkbox {
                div { class: "mt-4 flex items-center space-x-2",
                    input {
                        r#type: "checkbox",
                        id: "seed-after-download",
                        checked: *seed_after_download.read(),
                        onchange: move |evt| {
                            import_context_for_seed.set_seed_after_download(evt.checked());
                        },
                        class: "w-4 h-4 text-blue-600 border-gray-600 rounded focus:ring-blue-500 bg-gray-700"
                    }
                    label {
                        r#for: "seed-after-download",
                        class: "text-sm text-gray-300",
                        "Seed after download"
                    }
                }
            }
        }
    }
}
