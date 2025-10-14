use super::types::ImportProgress;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use tokio::sync::mpsc as tokio_mpsc;

type SubscriptionId = u64;

/// Filter criteria for progress subscriptions
#[derive(Debug, Clone)]
enum SubscriptionFilter {
    Album { album_id: String },
    Track { album_id: String, track_id: String },
}

impl SubscriptionFilter {
    fn matches(&self, progress: &ImportProgress) -> bool {
        match self {
            SubscriptionFilter::Album { album_id } => match progress {
                ImportProgress::Started { album_id: aid, .. } => aid == album_id,
                ImportProgress::ProcessingProgress { album_id: aid, .. } => aid == album_id,
                ImportProgress::TrackComplete { album_id: aid, .. } => aid == album_id,
                ImportProgress::Complete { album_id: aid } => aid == album_id,
                ImportProgress::Failed { album_id: aid, .. } => aid == album_id,
            },
            SubscriptionFilter::Track { album_id, track_id } => match progress {
                ImportProgress::TrackComplete {
                    album_id: aid,
                    track_id: tid,
                } => aid == album_id && tid == track_id,
                // Also include album-level updates for context
                ImportProgress::Started { album_id: aid, .. } => aid == album_id,
                ImportProgress::Complete { album_id: aid } => aid == album_id,
                ImportProgress::Failed { album_id: aid, .. } => aid == album_id,
                _ => false,
            },
        }
    }
}

struct Subscription {
    filter: SubscriptionFilter,
    tx: tokio_mpsc::UnboundedSender<ImportProgress>,
}

/// Progress service that broadcasts import progress updates
#[derive(Clone)]
pub struct ImportProgressService {
    subscriptions: Arc<Mutex<HashMap<SubscriptionId, Subscription>>>,
    next_id: Arc<AtomicU64>,
}

impl ImportProgressService {
    /// Create a new progress service and spawn background task to process progress updates
    pub fn new(
        mut progress_rx: tokio_mpsc::UnboundedReceiver<ImportProgress>,
        runtime_handle: tokio::runtime::Handle,
    ) -> Self {
        let subscriptions: Arc<Mutex<HashMap<SubscriptionId, Subscription>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let subscriptions_clone = subscriptions.clone();

        // Spawn async task to receive progress updates and dispatch to subscribers
        runtime_handle.spawn(async move {
            loop {
                match progress_rx.recv().await {
                    Some(progress) => {
                        // Dispatch to all matching subscribers
                        let mut subs = subscriptions_clone.lock().unwrap();
                        let mut to_remove = Vec::new();

                        for (id, subscription) in subs.iter() {
                            if subscription.filter.matches(&progress) {
                                // If send fails, receiver was dropped - mark for removal
                                if subscription.tx.send(progress.clone()).is_err() {
                                    to_remove.push(*id);
                                }
                            }
                        }

                        // Clean up dropped subscriptions
                        for id in to_remove {
                            subs.remove(&id);
                        }
                    }
                    None => {
                        println!("ProgressService: Channel closed, exiting");
                        break;
                    }
                }
            }
        });

        Self {
            subscriptions,
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Subscribe to progress updates for a specific album
    /// Returns a receiver that yields only progress updates for the specified album
    /// Subscription is automatically removed when receiver is dropped
    pub fn subscribe_album(
        &self,
        album_id: String,
    ) -> tokio_mpsc::UnboundedReceiver<ImportProgress> {
        let (tx, rx) = tokio_mpsc::unbounded_channel();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let subscription = Subscription {
            filter: SubscriptionFilter::Album { album_id },
            tx,
        };

        self.subscriptions.lock().unwrap().insert(id, subscription);
        rx
    }

    /// Subscribe to progress updates for a specific track
    /// Returns a receiver that yields only progress updates for the specified track
    /// Subscription is automatically removed when receiver is dropped
    pub fn subscribe_track(
        &self,
        album_id: String,
        track_id: String,
    ) -> tokio_mpsc::UnboundedReceiver<ImportProgress> {
        let (tx, rx) = tokio_mpsc::unbounded_channel();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let subscription = Subscription {
            filter: SubscriptionFilter::Track { album_id, track_id },
            tx,
        };

        self.subscriptions.lock().unwrap().insert(id, subscription);
        rx
    }
}
