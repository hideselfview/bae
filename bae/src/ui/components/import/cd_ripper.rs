//! CD ripper component for selecting drive and ripping CD

use crate::cd::CdDrive;
use crate::import::{ImportPhase, ImportProgress};
use crate::library::use_import_service;
use crate::ui::Route;
use dioxus::prelude::*;
use std::path::PathBuf;

#[component]
pub fn CdRipper(on_drive_select: EventHandler<PathBuf>, on_error: EventHandler<String>) -> Element {
    let mut drives = use_signal(|| Vec::<CdDrive>::new());
    let mut selected_drive = use_signal(|| None::<PathBuf>);
    let mut is_scanning = use_signal(|| false);
    let import_service = use_import_service();
    let navigator = use_navigator();
    
    // Track active import state
    let mut active_import_info = use_signal(|| None::<(String, String)>); // (release_id, album_id)
    let mut import_progress = use_signal(|| None::<u8>);
    let mut import_phase: Signal<Option<ImportPhase>> = use_signal(|| None);

    // Clone import_service for use in closures
    let import_service_for_effect = import_service.clone();
    let import_service_for_onchange = import_service.clone();

    use_effect(move || {
        let import_service_for_effect = import_service_for_effect.clone();
        let mut active_import_info_for_effect = active_import_info;
        let mut import_progress_for_effect = import_progress;
        let mut import_phase_for_effect = import_phase;
        spawn(async move {
            is_scanning.set(true);
            match CdDrive::detect_drives() {
                Ok(detected_drives) => {
                    if let Some(first_drive) = detected_drives.first() {
                        let path = first_drive.device_path.clone();
                        selected_drive.set(Some(path.clone()));
                        on_drive_select.call(path.clone());

                        // Check for active import on initial drive selection
                        if let Some((release_id, album_id)) =
                            import_service_for_effect.get_active_cd_import(&path)
                        {
                            active_import_info_for_effect
                                .set(Some((release_id.clone(), album_id.clone())));
                            import_progress_for_effect.set(Some(0));

                            // Subscribe to progress updates
                            let mut progress_rx =
                                import_service_for_effect.subscribe_release(release_id);
                            let mut active_import_info_for_progress = active_import_info_for_effect;
                            let mut import_progress_for_progress = import_progress_for_effect;
                            let mut import_phase_for_progress = import_phase_for_effect;
                            spawn(async move {
                                while let Some(progress_event) = progress_rx.recv().await {
                                    match progress_event {
                                        ImportProgress::Progress { percent, phase, .. } => {
                                            import_progress_for_progress.set(Some(percent));
                                            import_phase_for_progress.set(phase);
                                        }
                                        ImportProgress::Complete { .. }
                                        | ImportProgress::Failed { .. } => {
                                            // Import finished, clear active import state
                                            active_import_info_for_progress.set(None);
                                            import_progress_for_progress.set(None);
                                            import_phase_for_progress.set(None);
                                            break;
                                        }
                                        ImportProgress::Started { .. } => {
                                            import_progress_for_progress.set(Some(0));
                                        }
                                    }
                                }
                            });
                        }
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
                                        let import_service_for_change = import_service_for_onchange.clone();
                                        selected_drive.set(Some(path.clone()));
                                        on_drive_select.call(path.clone());
                                        
                                        // Check for active import on this drive
                                        if let Some((release_id, album_id)) = import_service_for_change.get_active_cd_import(&path) {
                                            active_import_info.set(Some((release_id.clone(), album_id.clone())));
                                            import_progress.set(Some(0));
                                            
                                            // Subscribe to progress updates
                                            let mut progress_rx = import_service_for_change.subscribe_release(release_id);
                                            let mut active_import_info_for_progress = active_import_info;
                                            let mut import_progress_for_progress = import_progress;
                                            let mut import_phase_for_progress = import_phase;
                                            spawn(async move {
                                                while let Some(progress_event) = progress_rx.recv().await {
                                                    match progress_event {
                                                        ImportProgress::Progress { percent, phase, .. } => {
                                                            import_progress_for_progress.set(Some(percent));
                                                            import_phase_for_progress.set(phase);
                                                        }
                                                        ImportProgress::Complete { .. } | ImportProgress::Failed { .. } => {
                                                            // Import finished, clear active import state
                                                            active_import_info_for_progress.set(None);
                                                            import_progress_for_progress.set(None);
                                                            import_phase_for_progress.set(None);
                                                            break;
                                                        }
                                                        ImportProgress::Started { .. } => {
                                                            import_progress_for_progress.set(Some(0));
                                                        }
                                                    }
                                                }
                                            });
                                        } else {
                                            active_import_info.set(None);
                                            import_progress.set(None);
                                            import_phase.set(None);
                                        }
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
                            
                            // Display active import progress if any
                            if let Some((release_id, album_id)) = active_import_info.read().clone() {
                                div { class: "mt-4 p-4 bg-blue-50 border border-blue-200 rounded-lg",
                                    div { class: "space-y-3",
                                        div { class: "flex items-center justify-between",
                                            div { class: "flex items-center space-x-2",
                                                span { class: "text-sm font-medium text-blue-900",
                                                    "Import in progress"
                                                }
                                                if let Some(phase) = import_phase.read().as_ref() {
                                                    span { class: "text-xs text-blue-600",
                                                        match phase {
                                                            ImportPhase::Rip => "Ripping CD...",
                                                            ImportPhase::Chunk => "Uploading...",
                                                        }
                                                    }
                                                }
                                            }
                                            button {
                                                class: "text-sm text-blue-600 hover:text-blue-800 underline",
                                                onclick: move |_| {
                                                    let (release_id_clone, album_id_clone) = (release_id.clone(), album_id.clone());
                                                    navigator.push(Route::AlbumDetail {
                                                        album_id: album_id_clone,
                                                        release_id: release_id_clone,
                                                    });
                                                },
                                                "View progress →"
                                            }
                                        }
                                        if let Some(percent) = import_progress.read().as_ref() {
                                            div { class: "w-full bg-blue-200 rounded-full h-2",
                                                div {
                                                    class: "bg-blue-600 h-2 rounded-full transition-all duration-300",
                                                    style: "width: {percent}%",
                                                }
                                            }
                                            div { class: "text-xs text-blue-600 text-right",
                                                "{percent}%"
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
    }
}
