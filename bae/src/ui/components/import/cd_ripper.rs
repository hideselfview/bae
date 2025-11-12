//! CD ripper component for selecting drive and ripping CD

use crate::cd::CdDrive;
use dioxus::prelude::*;
use std::path::PathBuf;

#[component]
pub fn CdRipper(
    on_drive_select: EventHandler<PathBuf>,
    on_error: EventHandler<String>,
) -> Element {
    let mut drives = use_signal(|| Vec::<CdDrive>::new());
    let mut selected_drive = use_signal(|| None::<PathBuf>);
    let mut is_scanning = use_signal(|| false);

    use_effect(move || {
        spawn(async move {
            is_scanning.set(true);
            match CdDrive::detect_drives() {
                Ok(detected_drives) => {
                    if let Some(first_drive) = detected_drives.first() {
                        selected_drive.set(Some(first_drive.device_path.clone()));
                        on_drive_select.call(first_drive.device_path.clone());
                    }
                    drives.set(detected_drives);
                }
                Err(e) => {
                    on_error.call(format!("Failed to detect CD drives: {}", e));
                }
            }
            is_scanning.set(false);
        });
    });

    rsx! {
        div { class: "space-y-4",
            if *is_scanning.read() {
                div { class: "text-center py-4",
                    "Scanning for CD drives..."
                }
            } else {
                div { class: "space-y-4",
                    if drives.read().is_empty() {
                        div { class: "text-center py-8 text-gray-500",
                            "No CD drives detected"
                        }
                    } else {
                        div { class: "space-y-2",
                            label { class: "block text-sm font-medium text-gray-700",
                                "Select CD Drive"
                            }
                            select {
                                class: "w-full px-3 py-2 border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-blue-500 focus:border-blue-500",
                                onchange: move |evt| {
                                    let value = evt.value();
                                    if !value.is_empty() {
                                        let path = PathBuf::from(value);
                                        selected_drive.set(Some(path.clone()));
                                        on_drive_select.call(path);
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

