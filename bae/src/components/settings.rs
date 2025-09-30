use dioxus::prelude::*;
use crate::api_keys;
use crate::s3_config::{self, S3ConfigData};

/// Settings page
#[component]
pub fn Settings() -> Element {
    let mut api_key_input = use_signal(|| String::new());
    let mut is_saving = use_signal(|| false);
    let mut save_message = use_signal(|| None::<String>);
    let mut has_api_key = use_signal(|| false);
    let mut is_loading = use_signal(|| true);
    let mut is_editing = use_signal(|| false);

    // S3 configuration state
    let mut s3_bucket = use_signal(|| String::new());
    let mut s3_region = use_signal(|| String::new());
    let mut s3_access_key = use_signal(|| String::new());
    let mut s3_secret_key = use_signal(|| String::new());
    let mut s3_endpoint = use_signal(|| String::new());
    let mut has_s3_config = use_signal(|| false);
    let mut is_saving_s3 = use_signal(|| false);
    let mut s3_save_message = use_signal(|| None::<String>);
    let mut is_editing_s3 = use_signal(|| false);

    // Check if API key and S3 config exist on component load
    use_effect(move || {
        spawn(async move {
            let exists = api_keys::check_api_key_exists();
            has_api_key.set(exists);
            is_editing.set(!exists);
            
            let s3_exists = s3_config::check_s3_config_exists();
            has_s3_config.set(s3_exists);
            is_editing_s3.set(!s3_exists);
            
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

    // S3 configuration handlers
    let mut save_s3_config_action = move || {
        let bucket = s3_bucket.read().clone();
        let region = s3_region.read().clone();
        let access_key = s3_access_key.read().clone();
        let secret_key = s3_secret_key.read().clone();
        let endpoint = s3_endpoint.read().clone();
        
        if bucket.trim().is_empty() || region.trim().is_empty() || 
           access_key.trim().is_empty() || secret_key.trim().is_empty() {
            s3_save_message.set(Some("Please fill in all required fields".to_string()));
            return;
        }

        spawn(async move {
            is_saving_s3.set(true);
            s3_save_message.set(None);

            let config = S3ConfigData {
                bucket_name: bucket,
                region,
                access_key_id: access_key,
                secret_access_key: secret_key,
                endpoint_url: if endpoint.trim().is_empty() { None } else { Some(endpoint) },
            };

            match s3_config::validate_and_store_s3_config(&config).await {
                Ok(_) => {
                    s3_save_message.set(Some("S3 configuration saved and validated successfully!".to_string()));
                    has_s3_config.set(true);
                    is_editing_s3.set(false);
                    // Clear inputs for security
                    s3_bucket.set(String::new());
                    s3_region.set(String::new());
                    s3_access_key.set(String::new());
                    s3_secret_key.set(String::new());
                    s3_endpoint.set(String::new());
                } 
                Err(e) => {
                    s3_save_message.set(Some(format!("Error: {}", e)));
                }
            }

            is_saving_s3.set(false);
        });
    };

    let delete_s3_config_action = move || {
        spawn(async move {
            match s3_config::remove_s3_config() {
                Ok(_) => {
                    s3_save_message.set(Some("S3 configuration deleted successfully".to_string()));
                    has_s3_config.set(false);
                    is_editing_s3.set(false);
                    s3_bucket.set(String::new());
                    s3_region.set(String::new());
                    s3_access_key.set(String::new());
                    s3_secret_key.set(String::new());
                    s3_endpoint.set(String::new());
                }
                Err(e) => {
                    s3_save_message.set(Some(format!("Error deleting S3 config: {}", e)));
                }
            }
        });
    };

    let edit_s3_config_action = move || {
        spawn(async move {
            match s3_config::retrieve_s3_config() {
                Ok(config) => {
                    s3_bucket.set(config.bucket_name);
                    s3_region.set(config.region);
                    s3_access_key.set(config.access_key_id);
                    s3_secret_key.set(config.secret_access_key);
                    s3_endpoint.set(config.endpoint_url.unwrap_or_default());
                    is_editing_s3.set(true);
                    s3_save_message.set(None);
                }
                Err(e) => {
                    s3_save_message.set(Some(format!("Error loading S3 config: {}", e)));
                }
            }
        });
    };

    let mut cancel_s3_edit_action = move || {
        is_editing_s3.set(false);
        s3_bucket.set(String::new());
        s3_region.set(String::new());
        s3_access_key.set(String::new());
        s3_secret_key.set(String::new());
        s3_endpoint.set(String::new());
        s3_save_message.set(None);
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

            // S3 Cloud Storage Configuration Section
            div {
                class: "bg-white rounded-lg shadow-md p-6",
                h2 {
                    class: "text-xl font-bold mb-4",
                    "Cloud Storage (S3/MinIO)"
                }
                
                p {
                    class: "text-gray-600 mb-4",
                    "Configure S3-compatible cloud storage for your music library. Supports AWS S3, MinIO, and other S3-compatible services."
                }

                if *is_loading.read() {
                    p { class: "text-gray-500", "Loading configuration..." }
                } else if *has_s3_config.read() && !*is_editing_s3.read() {
                    div {
                        class: "flex items-center justify-between bg-green-50 border border-green-200 rounded p-4 mb-4",
                        div {
                            class: "flex items-center",
                            span {
                                class: "text-green-600 font-medium",
                                "✓ Cloud storage configured"
                            }
                        }
                        div {
                            class: "flex gap-2",
                            button {
                                class: "bg-blue-500 text-white px-4 py-2 rounded hover:bg-blue-600 transition-colors",
                                onclick: move |_| edit_s3_config_action(),
                                "Edit Config"
                            }
                            button {
                                class: "bg-red-500 text-white px-4 py-2 rounded hover:bg-red-600 transition-colors",
                                onclick: move |_| delete_s3_config_action(),
                                "Remove Config"
                            }
                        }
                    }
                } else {
                    div {
                        class: "space-y-4",
                        
                        // Bucket Name
                        div {
                            label {
                                class: "block text-sm font-medium text-gray-700 mb-2",
                                "Bucket Name *"
                            }
                            input {
                                r#type: "text",
                                class: "w-full p-3 border border-gray-300 rounded-lg",
                                placeholder: "my-music-bucket",
                                value: "{s3_bucket}",
                                oninput: move |event| {
                                    s3_bucket.set(event.value());
                                    s3_save_message.set(None);
                                }
                            }
                        }

                        // Region
                        div {
                            label {
                                class: "block text-sm font-medium text-gray-700 mb-2",
                                "Region *"
                            }
                            input {
                                r#type: "text",
                                class: "w-full p-3 border border-gray-300 rounded-lg",
                                placeholder: "us-east-1",
                                value: "{s3_region}",
                                oninput: move |event| {
                                    s3_region.set(event.value());
                                    s3_save_message.set(None);
                                }
                            }
                        }

                        // Access Key ID
                        div {
                            label {
                                class: "block text-sm font-medium text-gray-700 mb-2",
                                "Access Key ID *"
                            }
                            input {
                                r#type: "text",
                                class: "w-full p-3 border border-gray-300 rounded-lg",
                                placeholder: "AKIAIOSFODNN7EXAMPLE",
                                value: "{s3_access_key}",
                                oninput: move |event| {
                                    s3_access_key.set(event.value());
                                    s3_save_message.set(None);
                                }
                            }
                        }

                        // Secret Access Key
                        div {
                            label {
                                class: "block text-sm font-medium text-gray-700 mb-2",
                                "Secret Access Key *"
                            }
                            input {
                                r#type: "password",
                                class: "w-full p-3 border border-gray-300 rounded-lg",
                                placeholder: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
                                value: "{s3_secret_key}",
                                oninput: move |event| {
                                    s3_secret_key.set(event.value());
                                    s3_save_message.set(None);
                                }
                            }
                        }

                        // Endpoint URL (optional for MinIO)
                        div {
                            label {
                                class: "block text-sm font-medium text-gray-700 mb-2",
                                "Endpoint URL (Optional - for MinIO/Custom S3)"
                            }
                            input {
                                r#type: "text",
                                class: "w-full p-3 border border-gray-300 rounded-lg",
                                placeholder: "http://localhost:9000 or https://s3.example.com",
                                value: "{s3_endpoint}",
                                oninput: move |event| {
                                    s3_endpoint.set(event.value());
                                    s3_save_message.set(None);
                                }
                            }
                            p {
                                class: "text-xs text-gray-500 mt-1",
                                "Leave empty for AWS S3. For MinIO, use: http://localhost:9000"
                            }
                        }
                        
                        div {
                            class: "flex gap-2",
                            button {
                                class: "flex-1 bg-blue-500 text-white px-6 py-2 rounded-lg hover:bg-blue-600 transition-colors disabled:bg-gray-400",
                                disabled: *is_saving_s3.read(),
                                onclick: move |_| save_s3_config_action(),
                                if *is_saving_s3.read() {
                                    "Validating..."
                                } else if *is_editing_s3.read() {
                                    "Update & Validate"
                                } else {
                                    "Save & Validate"
                                }
                            }
                            if *is_editing_s3.read() {
                                button {
                                    class: "bg-gray-500 text-white px-6 py-2 rounded-lg hover:bg-gray-600 transition-colors",
                                    onclick: move |_| cancel_s3_edit_action(),
                                    "Cancel"
                                }
                            }
                        }
                    }
                }

                if let Some(message) = s3_save_message.read().as_ref() {
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
        }
    }
}
