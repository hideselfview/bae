use crate::db::ImportStatus;
use crate::import::ImportProgress;
use crate::library::use_import_service;
use dioxus::prelude::*;

#[derive(Debug, Clone, PartialEq)]
pub enum TrackImportState {
    NotStarted,
    Queued,
    Importing { percent: u8 },
    Complete,
}

/// Hook to track import progress for a specific track
/// Returns the current import state of the track
pub fn use_track_progress(
    release_id: String,
    track_id: String,
    import_status: ImportStatus,
) -> Signal<TrackImportState> {
    let import_service = use_import_service();
    let mut state = use_signal(|| match import_status {
        ImportStatus::Queued => TrackImportState::Queued,
        ImportStatus::Importing => TrackImportState::Importing { percent: 0 },
        ImportStatus::Complete => TrackImportState::Complete,
        ImportStatus::Failed => TrackImportState::NotStarted,
    });

    use_effect(move || {
        let is_active =
            import_status == ImportStatus::Importing || import_status == ImportStatus::Queued;

        if is_active {
            let import_service = import_service.clone();
            let release_id = release_id.clone();
            let track_id = track_id.clone();

            spawn(async move {
                let mut progress_rx = import_service.subscribe_track(release_id, track_id);

                while let Some(progress_event) = progress_rx.recv().await {
                    match progress_event {
                        ImportProgress::Started { .. } => {
                            state.set(TrackImportState::Importing { percent: 0 });
                        }
                        ImportProgress::ProcessingProgress { percent, .. } => {
                            state.set(TrackImportState::Importing { percent });
                        }
                        ImportProgress::TrackComplete { .. } => {
                            state.set(TrackImportState::Complete);
                            break;
                        }
                        ImportProgress::Complete { .. } | ImportProgress::Failed { .. } => {
                            break;
                        }
                    }
                }
            });
        }
    });

    state
}
