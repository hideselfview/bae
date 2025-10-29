use crate::import::types::ImportProgress;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::info;

type SubscriptionId = u64;

/// Filter criteria for progress subscriptions
#[derive(Debug, Clone)]
enum SubscriptionFilter {
    Release { release_id: String },
    Track { track_id: String },
}

impl SubscriptionFilter {
    fn matches(&self, progress: &ImportProgress) -> bool {
        match self {
            SubscriptionFilter::Release { release_id } => match progress {
                ImportProgress::Started { id } => id == release_id,
                ImportProgress::Progress { id, .. } => id == release_id,
                ImportProgress::Complete { id } => id == release_id,
                ImportProgress::Failed { id, .. } => id == release_id,
            },
            SubscriptionFilter::Track { track_id } => match progress {
                ImportProgress::Started { id } => id == track_id,
                ImportProgress::Progress { id, .. } => id == track_id,
                ImportProgress::Complete { id } => id == track_id,
                ImportProgress::Failed { id, .. } => id == track_id,
            },
        }
    }
}

struct Subscription {
    filter: SubscriptionFilter,
    tx: tokio_mpsc::UnboundedSender<ImportProgress>,
}

/// Handle for subscribing to import progress updates
#[derive(Clone)]
pub struct ImportProgressHandle {
    subscriptions: Arc<Mutex<HashMap<SubscriptionId, Subscription>>>,
    next_id: Arc<AtomicU64>,
}

impl ImportProgressHandle {
    /// Create a new progress handle and spawn background task to process progress updates
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
                        info!("Channel closed, exiting");
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

    /// Subscribe to progress updates for a specific release
    /// Returns a receiver that yields only progress updates for the specified release
    /// Subscription is automatically removed when receiver is dropped
    pub fn subscribe_release(
        &self,
        release_id: String,
    ) -> tokio_mpsc::UnboundedReceiver<ImportProgress> {
        let (tx, rx) = tokio_mpsc::unbounded_channel();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let subscription = Subscription {
            filter: SubscriptionFilter::Release { release_id },
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
        track_id: String,
    ) -> tokio_mpsc::UnboundedReceiver<ImportProgress> {
        let (tx, rx) = tokio_mpsc::unbounded_channel();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let subscription = Subscription {
            filter: SubscriptionFilter::Track { track_id },
            tx,
        };

        self.subscriptions.lock().unwrap().insert(id, subscription);
        rx
    }
}
