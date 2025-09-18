use dioxus::prelude::*;
use crate::api_keys;

/// Settings page
#[component]
pub fn Settings() -> Element {
    let mut api_key_input = use_signal(|| String::new());
    let mut is_saving = use_signal(|| false);
    let mut save_message = use_signal(|| None::<String>);
    let mut has_api_key = use_signal(|| false);
    let mut is_loading = use_signal(|| true);
    let mut is_editing = use_signal(|| false);

    // Check if API key exists on component load
    use_effect(move || {
        spawn(async move {
            let exists = api_keys::check_api_key_exists();
            has_api_key.set(exists);
            is_editing.set(!exists);
            is_loading.set(false);
        });
    });

    let mut save_api_key_action = move || {
        let key = api_key_input.read().clone();
        if key.trim().is_empty() {
            save_message.set(Some("Please enter an API key".to_string()));
            return;
        }

        spawn(async move {
            is_saving.set(true);
            save_message.set(None);

            // Call API key functions directly instead of server functions
            match api_keys::validate_and_store_api_key(&key).await {
                Ok(_) => {
                    save_message.set(Some("API key saved and validated successfully!".to_string()));
                    has_api_key.set(true);
                    is_editing.set(false);
                    api_key_input.set(String::new()); // Clear the input for security
                } 
                Err(e) => {
                    save_message.set(Some(format!("Error: {}", e)));
                }
            }

            is_saving.set(false);
        });
    };

    let delete_api_key_action = move || {
        spawn(async move {
            match api_keys::remove_api_key() {
                Ok(_) => {
                    save_message.set(Some("API key deleted successfully".to_string()));
                    has_api_key.set(false);
                    is_editing.set(false);
                    api_key_input.set(String::new());
                }
                Err(e) => {
                    save_message.set(Some(format!("Error deleting API key: {}", e)));
                }
            }
        });
    };

    let edit_api_key_action = move || {
        spawn(async move {
            match api_keys::retrieve_api_key() {
                Ok(key) => {
                    api_key_input.set(key);
                    is_editing.set(true);
                    save_message.set(None);
                }
                Err(e) => {
                    save_message.set(Some(format!("Error loading API key: {}", e)));
                }
            }
        });
    };

    let mut cancel_edit_action = move || {
        is_editing.set(false);
        api_key_input.set(String::new());
        save_message.set(None);
    };

    rsx! {
        div {
            class: "container mx-auto p-6 max-w-2xl",
            h1 { 
                class: "text-3xl font-bold mb-6",
                "Settings" 
            }

            // API Key Management Section
            div {
                class: "bg-white rounded-lg shadow-md p-6 mb-6",
                h2 {
                    class: "text-xl font-bold mb-4",
                    "Discogs API Key"
                }
                
                p {
                    class: "text-gray-600 mb-4",
                    "To search and import albums, you need a Discogs API key. "
                    a {
                        href: "https://www.discogs.com/settings/developers",
                        target: "_blank",
                        class: "text-blue-500 hover:text-blue-700 underline",
                        "Get your API key here"
                    }
                }

                if *is_loading.read() {
                    p { class: "text-gray-500", "Checking API key status..." }
                } else if *has_api_key.read() && !*is_editing.read() {
                    div {
                        class: "flex items-center justify-between bg-green-50 border border-green-200 rounded p-4 mb-4",
                        div {
                            class: "flex items-center",
                            span {
                                class: "text-green-600 font-medium",
                                "✓ API key configured and valid"
                            }
                        }
                        div {
                            class: "flex gap-2",
                            button {
                                class: "bg-blue-500 text-white px-4 py-2 rounded hover:bg-blue-600 transition-colors",
                                onclick: move |_| edit_api_key_action(),
                                "Edit Key"
                            }
                            button {
                                class: "bg-red-500 text-white px-4 py-2 rounded hover:bg-red-600 transition-colors",
                                onclick: move |_| delete_api_key_action(),
                                "Remove Key"
                            }
                        }
                    }
                } else {
                    div {
                        class: "space-y-4",
                        div {
                            label {
                                class: "block text-sm font-medium text-gray-700 mb-2",
                                "API Key"
                            }
                            input {
                                r#type: "password",
                                class: "w-full p-3 border border-gray-300 rounded-lg",
                                placeholder: "Enter your Discogs API key",
                                value: "{api_key_input}",
                                oninput: move |event| {
                                    api_key_input.set(event.value());
                                    save_message.set(None); // Clear any previous messages
                                }
                            }
                        }
                        
                        div {
                            class: "flex gap-2",
                            button {
                                class: "flex-1 bg-blue-500 text-white px-6 py-2 rounded-lg hover:bg-blue-600 transition-colors disabled:bg-gray-400",
                                disabled: *is_saving.read(),
                                onclick: move |_| save_api_key_action(),
                                if *is_saving.read() {
                                    "Validating..."
                                } else if *is_editing.read() {
                                    "Update & Validate"
                                } else {
                                    "Save & Validate"
                                }
                            }
                            if *is_editing.read() {
                                button {
                                    class: "bg-gray-500 text-white px-6 py-2 rounded-lg hover:bg-gray-600 transition-colors",
                                    onclick: move |_| cancel_edit_action(),
                                    "Cancel"
                                }
                            }
                        }
                    }
                }

                if let Some(message) = save_message.read().as_ref() {
                    div {
                        class: if message.contains("successfully") {
                            "mt-4 p-3 bg-green-100 border border-green-400 text-green-700 rounded"
                        } else {
                            "mt-4 p-3 bg-red-100 border border-red-400 text-red-700 rounded"
                        },
                        "{message}"
                    }
                }
            }

            // Future settings sections can be added here
            div {
                class: "bg-gray-50 rounded-lg p-6",
                h2 {
                    class: "text-xl font-bold mb-4 text-gray-600",
                    "More Settings"
                }
                p {
                    class: "text-gray-500",
                    "Additional settings will be available here in future updates."
                }
            }
        }
    }
}
