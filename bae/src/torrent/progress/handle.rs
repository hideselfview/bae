use crate::torrent::progress::TorrentProgress;
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
    Torrent { info_hash: String },
}

impl SubscriptionFilter {
    fn matches(&self, progress: &TorrentProgress) -> bool {
        match self {
            SubscriptionFilter::Torrent { info_hash } => {
                let progress_info_hash = match progress {
                    TorrentProgress::WaitingForMetadata { info_hash } => info_hash,
                    TorrentProgress::TorrentInfoReady { info_hash, .. } => info_hash,
                    TorrentProgress::StatusUpdate { info_hash, .. } => info_hash,
                    TorrentProgress::MetadataFilesDetected { info_hash, .. } => info_hash,
                    TorrentProgress::MetadataProgress { info_hash, .. } => info_hash,
                    TorrentProgress::MetadataComplete { info_hash, .. } => info_hash,
                    TorrentProgress::Error { info_hash, .. } => info_hash,
                };
                progress_info_hash == info_hash
            }
        }
    }
}

struct Subscription {
    filter: SubscriptionFilter,
    tx: tokio_mpsc::UnboundedSender<TorrentProgress>,
}

/// Handle for subscribing to torrent progress updates
#[derive(Clone)]
pub struct TorrentProgressHandle {
    subscriptions: Arc<Mutex<HashMap<SubscriptionId, Subscription>>>,
    next_id: Arc<AtomicU64>,
}

impl TorrentProgressHandle {
    /// Create a new progress handle and spawn background task to process progress updates
    pub fn new(
        mut progress_rx: tokio_mpsc::UnboundedReceiver<TorrentProgress>,
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
                        info!("Torrent progress channel closed, exiting");
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

    /// Subscribe to progress updates for a specific torrent
    /// Returns a receiver that yields only progress updates for the specified torrent
    /// Subscription is automatically removed when receiver is dropped
    pub fn subscribe_torrent(
        &self,
        info_hash: String,
    ) -> tokio_mpsc::UnboundedReceiver<TorrentProgress> {
        let (tx, rx) = tokio_mpsc::unbounded_channel();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let subscription = Subscription {
            filter: SubscriptionFilter::Torrent { info_hash },
            tx,
        };

        self.subscriptions.lock().unwrap().insert(id, subscription);
        rx
    }
}
