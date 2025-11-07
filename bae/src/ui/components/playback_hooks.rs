use crate::playback::{PlaybackHandle, PlaybackProgress, PlaybackState};
use crate::AppContext;
use dioxus::prelude::*;

/// Hook to access the playback service
pub fn use_playback_service() -> PlaybackHandle {
    let context = use_context::<AppContext>();
    context.playback_handle.clone()
}

/// Shared playback state that tracks current playback state across the app
/// This allows components to synchronously read the current state on first render
#[derive(Clone)]
pub struct SharedPlaybackState {
    pub state: Signal<PlaybackState>,
}

/// Provider component to make playback state available throughout the app
#[component]
pub fn PlaybackStateProvider(children: Element) -> Element {
    let state_signal = use_signal(|| PlaybackState::Stopped);
    let shared_state = SharedPlaybackState {
        state: state_signal,
    };

    use_context_provider(|| shared_state.clone());

    // Subscribe to playback progress to keep state updated
    let playback = use_playback_service();
    use_effect({
        let playback = playback.clone();
        let mut state_signal = shared_state.state;
        move || {
            let playback = playback.clone();
            spawn(async move {
                let mut progress_rx = playback.subscribe_progress();
                while let Some(progress) = progress_rx.recv().await {
                    if let PlaybackProgress::StateChanged { state: new_state } = progress {
                        state_signal.set(new_state);
                    }
                }
            });
        }
    });

    rsx! {
        {children}
    }
}

/// Hook to access the current playback state synchronously
pub fn use_playback_state() -> Signal<PlaybackState> {
    let state = use_context::<SharedPlaybackState>();
    state.state
}

/// Queue management hook that provides reactive queue state and management methods
pub struct PlaybackQueue {
    pub tracks: Signal<Vec<String>>,
    pub clear: Box<dyn Fn()>,
}

/// Hook to access and manage the playback queue
pub fn use_playback_queue() -> PlaybackQueue {
    let playback_service = use_playback_service();
    let tracks_signal = use_signal(Vec::<String>::new);

    // Subscribe to queue updates and request initial state
    // This effect sets up a long-running subscription that reactively updates the signal
    // whenever the playback service emits QueueUpdated events. It also requests the initial
    // queue state on mount, which will trigger a QueueUpdated event that the subscription
    // will handle, ensuring the UI reflects the current state immediately.
    use_effect({
        let playback_service = playback_service.clone();
        let mut tracks_signal = tracks_signal;
        move || {
            // Request initial queue state (triggers a QueueUpdated event)
            playback_service.get_queue();

            // Set up subscription to reactively handle all queue updates (including the initial one)
            let playback_service = playback_service.clone();
            spawn(async move {
                let mut progress_rx = playback_service.subscribe_progress();
                while let Some(progress) = progress_rx.recv().await {
                    if let PlaybackProgress::QueueUpdated { tracks } = progress {
                        tracks_signal.set(tracks);
                    }
                }
            });
        }
    });

    let playback_clear = playback_service.clone();

    PlaybackQueue {
        tracks: tracks_signal,
        clear: Box::new(move || playback_clear.clear_queue()),
    }
}
