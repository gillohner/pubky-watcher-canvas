use std::{collections::HashMap, sync::Arc};

use pubky_watcher::WatcherClient;
use serde::Serialize;
use tokio::sync::{broadcast, Notify, RwLock};

use crate::game::Game;

#[derive(Clone, Debug, Serialize)]
pub struct Registration {
    pub public_key: String,
    pub homeserver: String,
    pub cursor: u64,
}

#[derive(Clone)]
pub struct AppState {
    pub client: WatcherClient,
    pub game: Arc<RwLock<Game>>,
    pub registrations: Arc<RwLock<HashMap<String, Registration>>>,
    pub updates: broadcast::Sender<String>,
    pub watcher_requests: Arc<Notify>,
}

impl AppState {
    pub fn new(client: WatcherClient) -> Self {
        let (updates, _) = broadcast::channel(128);
        Self {
            client,
            game: Arc::new(RwLock::new(Game::new())),
            registrations: Arc::new(RwLock::new(HashMap::new())),
            updates,
            watcher_requests: Arc::new(Notify::new()),
        }
    }

    pub fn notify(&self, kind: &str) {
        let _ = self.updates.send(kind.to_owned());
    }

    pub fn request_watcher_poll(&self) {
        self.watcher_requests.notify_one();
    }
}
