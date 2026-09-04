use std::{collections::HashMap, sync::Arc};

use pubky_watcher::WatcherClient;
use serde::Serialize;
use tokio::sync::{broadcast, RwLock};

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
}

impl AppState {
    pub fn new(client: WatcherClient) -> Self {
        let (updates, _) = broadcast::channel(128);
        Self {
            client,
            game: Arc::new(RwLock::new(Game::new())),
            registrations: Arc::new(RwLock::new(HashMap::new())),
            updates,
        }
    }

    pub fn notify(&self, kind: &str) {
        let _ = self.updates.send(kind.to_owned());
    }
}
