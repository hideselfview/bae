//! CD ripper component for selecting drive and ripping CD

use crate::cd::CdDrive;
use dioxus::prelude::*;
use std::path::PathBuf;

#[component]
pub fn CdRipper(on_drive_select: EventHandler<PathBuf>, on_error: EventHandler<String>) -> Element {
    let mut drives = use_signal(Vec::<CdDrive>::new);
    let mut selected_drive = use_signal(|| None::<PathBuf>);
    let mut is_scanning = use_signal(|| false);

    use_effect(move || {
        spawn(async move {
            is_scanning.set(true);
            match CdDrive::detect_drives() {
                Ok(detected_drives) => {
                    if let Some(first_drive) = detected_drives.first() {
                        let path = first_drive.device_path.clone();
                        selected_drive.set(Some(path.clone()));
                        on_drive_select.call(path.clone());
                    }
                    drives.set(detected_drives);
                }
                Err(e) => {
                    #[cfg(target_os = "macos")]
                    {
                        let error_msg = if e.to_string().contains("Permission denied")
                            || e.to_string().contains("Operation not permitted")
                        {
                            format!("Failed to access CD drive: {}\n\nOn macOS, you may need to grant Full Disk Access permission:\n1. Open System Settings → Privacy & Security → Full Disk Access\n2. Add this application to the list", e)
                        } else {
                            format!("Failed to detect CD drives: {}", e)
                        };
                        on_error.call(error_msg);
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        on_error.call(format!("Failed to detect CD drives: {}", e));
                    }
                }
            }
            is_scanning.set(false);
        });
    });

    rsx! {
        div { class: "space-y-4",
            if *is_scanning.read() {
                div { class: "text-center py-4 text-gray-400",
                    "Scanning for CD drives..."
                }
            } else {
                div { class: "space-y-4",
                    if drives.read().is_empty() {
                        div { class: "text-center py-8 text-gray-400",
                            "No CD drives detected"
                        }
                    } else {
                        div { class: "space-y-2",
                            label { class: "block text-sm font-medium text-gray-300",
                                "Select CD Drive"
                            }
                            select {
                                class: "w-full px-3 py-2 border border-gray-600 bg-gray-700 text-white rounded-md shadow-sm focus:outline-none focus:ring-blue-500 focus:border-blue-500",
                                onchange: move |evt| {
                                    let value = evt.value();
                                    if !value.is_empty() {
                                        let path = PathBuf::from(value);
                                        selected_drive.set(Some(path.clone()));
                                        on_drive_select.call(path.clone());
                                    }
                                },
                                for drive in drives.read().iter() {
                                    option {
                                        value: "{drive.device_path.display()}",
                                        selected: selected_drive.read().as_ref().map(|p| p == &drive.device_path).unwrap_or(false),
                                        "{drive.name}"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
