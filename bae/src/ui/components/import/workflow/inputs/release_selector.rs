use crate::ui::import_context::ImportContext;
use dioxus::prelude::*;
use std::rc::Rc;
use tracing::warn;

#[component]
pub fn ReleaseSelector() -> Element {
    let import_context = use_context::<Rc<ImportContext>>();
    let detected_releases = import_context.detected_releases();
    let mut selected_indices = use_signal(|| Vec::<usize>::new());

    let on_select_all = move |_| {
        let releases = detected_releases.read();
        selected_indices.set((0..releases.len()).collect());
    };

    let on_deselect_all = move |_| {
        selected_indices.set(Vec::new());
    };

    let on_import_selected = move |_| {
        let indices = selected_indices.read().clone();
        if indices.is_empty() {
            return;
        }

        let import_context = import_context.clone();
        spawn(async move {
            // Store selected indices and start with the first one
            import_context.set_selected_release_indices(indices.clone());
            import_context.set_current_release_index(0);

            // Load the first selected release
            if let Err(e) = import_context.load_selected_release(indices[0]).await {
                warn!("Failed to load selected release: {}", e);
                import_context.set_import_error_message(Some(e));
            }
        });
    };

    rsx! {
        div { class: "space-y-6",
            // Header
            div { class: "text-center",
                h2 { class: "text-2xl font-semibold text-gray-100 mb-2",
                    "Multiple Releases Detected"
                }
                p { class: "text-gray-400",
                    "Select the releases you want to import"
                }
            }

            // Selection controls
            div { class: "flex justify-between items-center",
                div { class: "text-sm text-gray-400",
                    {format!("{} of {} selected", selected_indices.read().len(), detected_releases.read().len())}
                }
                div { class: "flex gap-2",
                    button {
                        class: "px-3 py-1 text-sm bg-gray-700 hover:bg-gray-600 text-gray-200 rounded transition-colors",
                        onclick: on_select_all,
                        "Select All"
                    }
                    button {
                        class: "px-3 py-1 text-sm bg-gray-700 hover:bg-gray-600 text-gray-200 rounded transition-colors",
                        onclick: on_deselect_all,
                        "Deselect All"
                    }
                }
            }

            // Release list
            div { class: "space-y-2 max-h-96 overflow-y-auto",
                for (index , release) in detected_releases.read().iter().enumerate() {
                    {
                        let is_selected = selected_indices.read().contains(&index);
                        let checkbox_class = if is_selected {
                            "w-5 h-5 text-blue-500 bg-blue-500 border-blue-500 rounded focus:ring-2 focus:ring-blue-500"
                        } else {
                            "w-5 h-5 text-gray-400 bg-gray-700 border-gray-600 rounded focus:ring-2 focus:ring-blue-500"
                        };

                        rsx! {
                            div {
                                key: "{index}",
                                class: "flex items-start gap-3 p-4 bg-gray-800 rounded-lg hover:bg-gray-750 transition-colors cursor-pointer",
                                onclick: move |_| {
                                    let mut indices = selected_indices.write();
                                    if let Some(pos) = indices.iter().position(|&i| i == index) {
                                        indices.remove(pos);
                                    } else {
                                        indices.push(index);
                                        indices.sort_unstable();
                                    }
                                },

                                input {
                                    r#type: "checkbox",
                                    class: "{checkbox_class}",
                                    checked: is_selected,
                                    onclick: |e| e.stop_propagation(),
                                }

                                div { class: "flex-1 min-w-0",
                                    div { class: "font-medium text-gray-100 mb-1",
                                        {release.name.clone()}
                                    }
                                    div { class: "text-sm text-gray-400 truncate",
                                        {release.path.display().to_string()}
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Import button
            div { class: "flex justify-center pt-4",
                button {
                    class: if selected_indices.read().is_empty() {
                        "px-6 py-3 bg-gray-700 text-gray-500 rounded-lg cursor-not-allowed"
                    } else {
                        "px-6 py-3 bg-blue-600 hover:bg-blue-700 text-white rounded-lg transition-colors"
                    },
                    disabled: selected_indices.read().is_empty(),
                    onclick: on_import_selected,
                    {
                        let count = selected_indices.read().len();
                        if count == 0 {
                            "Select releases to import".to_string()
                        } else if count == 1 {
                            "Import 1 Release".to_string()
                        } else {
                            format!("Import {} Releases", count)
                        }
                    }
                }
            }
        }
    }
}
