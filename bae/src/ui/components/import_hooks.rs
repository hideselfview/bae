use crate::db::ImportStatus;
use crate::import::ImportProgress;
use crate::library::use_import_service;
use dioxus::prelude::*;
use tracing::info;

#[derive(Debug, Clone, PartialEq)]
pub enum TrackImportState {
    Queued,
    Importing { percent: u8 },
    Complete,
    Failed,
}

/// Hook to track import progress for a specific track
/// Returns the current import state of the track
pub fn use_track_progress(
    track_id: String,
    import_status: ImportStatus,
) -> Signal<TrackImportState> {
    let import_service = use_import_service();

    let mut state = use_signal(|| match import_status {
        ImportStatus::Queued => TrackImportState::Queued,
        ImportStatus::Importing => TrackImportState::Importing { percent: 0 },
        ImportStatus::Complete => TrackImportState::Complete,
        ImportStatus::Failed => TrackImportState::Failed,
    });

    use_effect(move || {
        let is_active =
            import_status == ImportStatus::Importing || import_status == ImportStatus::Queued;

        if is_active {
            let import_service = import_service.clone();
            let track_id = track_id.clone();

            spawn(async move {
                let mut progress_rx = import_service.subscribe_track(track_id.clone());

                info!("Track progress subscription started for track {}", track_id);

                while let Some(progress_event) = progress_rx.recv().await {
                    info!(
                        "Track {} received progress event: {:?}",
                        track_id, progress_event
                    );

                    match progress_event {
                        ImportProgress::Started { .. } => {
                            info!("Track {} started importing", track_id);
                            state.set(TrackImportState::Importing { percent: 0 });
                        }
                        ImportProgress::Progress { percent, .. } => {
                            info!("Track {} progress: {}%", track_id, percent);
                            state.set(TrackImportState::Importing { percent });
                        }
                        ImportProgress::Complete { .. } => {
                            info!("Track {} complete", track_id);
                            state.set(TrackImportState::Complete);
                            break;
                        }
                        ImportProgress::Failed { .. } => {
                            info!("Track {} failed", track_id);
                            state.set(TrackImportState::Failed);
                            break;
                        }
                    }
                }

                info!("Track progress subscription ended for track {}", track_id);
            });
        }
    });

    state
}
