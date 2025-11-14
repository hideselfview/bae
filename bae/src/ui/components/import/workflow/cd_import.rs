use super::handlers::handle_confirmation;
use super::inputs::CdRipper;
use super::shared::{Confirmation, ErrorDisplay, ExactLookup, ManualSearch};
use crate::import::{MatchCandidate, MatchSource};
use crate::library::{use_import_service, use_library_manager};
use crate::musicbrainz::lookup_by_discid;
use crate::ui::import_context::{ImportContext, ImportPhase};
use dioxus::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;

#[component]
pub fn CdImport() -> Element {
    let import_context = use_context::<Rc<ImportContext>>();
    let library_manager = use_library_manager();
    let import_service = use_import_service();
    let navigator = use_navigator();

    // Get signals via getters (signals are Copy)
    let folder_path = import_context.folder_path();
    let detected_metadata = import_context.detected_metadata();
    let import_phase = import_context.import_phase();
    let exact_match_candidates = import_context.exact_match_candidates();
    let selected_match_index = import_context.selected_match_index();
    let confirmed_candidate = import_context.confirmed_candidate();
    let is_looking_up = import_context.is_looking_up();
    let import_error_message = import_context.import_error_message();
    let duplicate_album_id = import_context.duplicate_album_id();
    let cd_toc_info: Signal<Option<(String, u8, u8)>> = use_signal(|| None); // (disc_id, first_track, last_track)

    let on_drive_select = {
        let import_context = import_context.clone();
        move |drive_path: PathBuf| {
            import_context.set_folder_path(drive_path.to_string_lossy().to_string());
            import_context.set_import_phase(ImportPhase::ExactLookup);
            import_context.set_is_looking_up(true);
            import_context.set_exact_match_candidates(Vec::new());
            import_context.set_detected_metadata(None);
            import_context.set_import_error_message(None);

            // Read TOC from CD and look up by DiscID
            let drive_path_clone = drive_path.clone();
            let mut cd_toc_info_async = cd_toc_info;
            let import_context_async = import_context.clone();

            spawn(async move {
                use crate::cd::CdDrive;

                let drive = CdDrive {
                    device_path: drive_path_clone.clone(),
                    name: drive_path_clone.to_string_lossy().to_string(),
                };

                match drive.read_toc() {
                    Ok(toc) => {
                        // Store CD info for display
                        cd_toc_info_async.set(Some((
                            toc.disc_id.clone(),
                            toc.first_track,
                            toc.last_track,
                        )));

                        // Look up by DiscID
                        match lookup_by_discid(&toc.disc_id).await {
                            Ok((matches, _external_urls)) => {
                                import_context_async.set_is_looking_up(false);
                                if matches.is_empty() {
                                    // No exact match, go to manual search
                                    import_context_async
                                        .set_import_phase(ImportPhase::ManualSearch);
                                    import_context_async
                                        .set_search_query(format!("DiscID: {}", toc.disc_id));
                                } else if matches.len() == 1 {
                                    // Single exact match, convert to MatchCandidate and auto-confirm
                                    let mb_release = matches[0].clone();
                                    let candidate = MatchCandidate {
                                        source: MatchSource::MusicBrainz(mb_release),
                                        confidence: 100.0, // Exact DiscID match
                                        match_reasons: vec!["Exact DiscID match".to_string()],
                                    };
                                    import_context_async.set_confirmed_candidate(Some(candidate));
                                    import_context_async
                                        .set_import_phase(ImportPhase::Confirmation);
                                } else {
                                    // Multiple matches, convert to MatchCandidates and show selection
                                    let candidates: Vec<MatchCandidate> = matches
                                        .into_iter()
                                        .map(|mb_release| MatchCandidate {
                                            source: MatchSource::MusicBrainz(mb_release),
                                            confidence: 100.0, // Exact DiscID match
                                            match_reasons: vec!["Exact DiscID match".to_string()],
                                        })
                                        .collect();
                                    import_context_async.set_exact_match_candidates(candidates);
                                }
                            }
                            Err(e) => {
                                import_context_async.set_is_looking_up(false);
                                import_context_async.set_import_error_message(Some(format!(
                                    "Failed to look up DiscID: {}",
                                    e
                                )));
                                import_context_async.set_import_phase(ImportPhase::ManualSearch);
                            }
                        }
                    }
                    Err(e) => {
                        import_context_async.set_is_looking_up(false);
                        import_context_async.set_import_error_message(Some(format!(
                            "Failed to read CD TOC: {}",
                            e
                        )));
                    }
                }
            });
        }
    };

    let on_confirm_from_manual = {
        let import_context_for_reset = import_context.clone();
        let library_manager = library_manager.clone();
        let import_service = import_service.clone();
        let detected_metadata_signal = detected_metadata;
        move |candidate: MatchCandidate| {
            let folder = folder_path.read().clone();
            let metadata = detected_metadata_signal.read().clone();
            let import_service = import_service.clone();
            let duplicate_album_id = duplicate_album_id;
            let import_error_message = import_error_message;
            let import_context_for_reset = import_context_for_reset.clone();
            let library_manager = library_manager.clone();

            spawn(async move {
                handle_confirmation(
                    candidate,
                    folder,
                    metadata,
                    crate::ui::components::import::ImportSource::Cd,
                    None,
                    false,
                    import_context_for_reset,
                    library_manager,
                    import_service,
                    navigator,
                    duplicate_album_id,
                    import_error_message,
                )
                .await;
            });
        }
    };

    let on_change_folder = {
        let import_context_clone = import_context.clone();
        move |_| {
            import_context_clone.reset();
        }
    };

    rsx! {
        div { class: "space-y-6",
            // Phase 1: CD Drive Selection
            if *import_phase.read() == ImportPhase::FolderSelection {
                div { class: "bg-white rounded-lg shadow p-6",
                    CdRipper {
                        on_drive_select: on_drive_select,
                        on_error: {
                            let import_context = import_context.clone();
                            move |e: String| {
                                import_context.set_import_error_message(Some(e));
                            }
                        }
                    }
                }
            } else {
                div { class: "space-y-6",
                    // Show selected CD
                    div { class: "bg-white rounded-lg shadow p-6",
                        div { class: "mb-6 pb-4 border-b border-gray-200",
                            div { class: "flex items-start justify-between mb-3",
                                h3 { class: "text-sm font-semibold text-gray-700 uppercase tracking-wide",
                                    "Selected CD"
                                }
                                button {
                                    class: "px-3 py-1 text-sm text-blue-600 hover:text-blue-800 hover:bg-blue-50 rounded-md transition-colors",
                                    onclick: on_change_folder,
                                    "Clear"
                                }
                            }
                            div { class: "inline-block px-4 py-2 bg-gray-100 hover:bg-gray-200 rounded-full border border-gray-300 transition-colors",
                                p {
                                    class: "text-sm text-gray-900 font-mono select-text cursor-text break-all",
                                    "{folder_path.read()}"
                                }
                            }
                            if let Some((disc_id, first_track, last_track)) = cd_toc_info.read().as_ref() {
                                div { class: "mt-4 p-4 bg-blue-50 border border-blue-200 rounded-lg",
                                    div { class: "space-y-2",
                                        div { class: "flex items-center",
                                            span { class: "text-sm font-medium text-gray-700 w-24", "DiscID:" }
                                            span { class: "text-sm text-gray-900 font-mono", "{disc_id}" }
                                        }
                                        div { class: "flex items-center",
                                            span { class: "text-sm font-medium text-gray-700 w-24", "Tracks:" }
                                            span { class: "text-sm text-gray-900",
                                                "{last_track - first_track + 1} tracks ({first_track}-{last_track})"
                                            }
                                        }
                                    }
                                }
                            } else if *is_looking_up.read() {
                                div { class: "mt-4 p-4 bg-gray-50 border border-gray-200 rounded-lg text-center",
                                    p { class: "text-sm text-gray-600", "Reading CD table of contents..." }
                                }
                            }
                        }
                    }

                    // Phase 2: Exact Lookup
                    if *import_phase.read() == ImportPhase::ExactLookup {
                        ExactLookup {
                            is_looking_up: is_looking_up,
                            exact_match_candidates: exact_match_candidates,
                            selected_match_index: selected_match_index,
                            on_select: {
                                let import_context = import_context.clone();
                                move |index| {
                                    import_context.set_selected_match_index(Some(index));
                                    if let Some(candidate) = import_context.exact_match_candidates().read().get(index) {
                                        import_context.set_confirmed_candidate(Some(candidate.clone()));
                                        import_context.set_import_phase(ImportPhase::Confirmation);
                                    }
                                }
                            },
                        }
                    }

                    // Phase 3: Manual Search
                    if *import_phase.read() == ImportPhase::ManualSearch {
                        ManualSearch {
                            detected_metadata: detected_metadata,
                            selected_match_index: selected_match_index,
                            on_match_select: {
                                let import_context = import_context.clone();
                                move |index| {
                                    import_context.set_selected_match_index(Some(index));
                                }
                            },
                            on_confirm: {
                                let import_context = import_context.clone();
                                move |candidate: MatchCandidate| {
                                    import_context.set_confirmed_candidate(Some(candidate.clone()));
                                    import_context.set_import_phase(ImportPhase::Confirmation);
                                }
                            },
                        }
                    }

                    // Phase 4: Confirmation
                    if *import_phase.read() == ImportPhase::Confirmation {
                        Confirmation {
                            confirmed_candidate: confirmed_candidate,
                            on_edit: {
                                let import_context = import_context.clone();
                                move || {
                                    import_context.set_confirmed_candidate(None);
                                    import_context.set_selected_match_index(None);
                                    if !import_context.exact_match_candidates().read().is_empty() {
                                        import_context.set_import_phase(ImportPhase::ExactLookup);
                                    } else {
                                        import_context.set_import_phase(ImportPhase::ManualSearch);
                                    }
                                }
                            },
                            on_confirm: {
                                let on_confirm_from_manual_local = on_confirm_from_manual;
                                move || {
                                    if let Some(candidate) = confirmed_candidate.read().as_ref().cloned() {
                                        on_confirm_from_manual_local(candidate);
                                    }
                                }
                            },
                        }
                    }

                    // Error messages
                    ErrorDisplay {
                        error_message: import_error_message,
                        duplicate_album_id: duplicate_album_id,
                    }
                }
            }
        }
    }
}
