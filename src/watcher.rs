use std::{collections::HashMap, io, sync::Arc, time::Duration};

use async_trait::async_trait;
use pubky::{Event, EventCursor, EventType, PublicKey};
use pubky_watcher::{read_stream_capped, EventHandler, ResourceReader, Watcher, WatcherError};
use tokio::sync::watch;
use tracing::{debug, warn};

use crate::{
    game::{ApplyResult, Move},
    state::{AppState, Registration},
};

const MOVES_PATH: &str = "/pub/pubky-watcher-canvas/moves/";
const EVENTS_PER_POLL: u16 = 100;
const MAX_MOVE_BYTES: usize = 1_024;

#[derive(Clone)]
struct MoveHandler {
    state: AppState,
    reader: Arc<dyn ResourceReader>,
}

#[async_trait]
impl EventHandler<Event, WatcherError> for MoveHandler {
    async fn handle(&self, event: &Event) -> Result<(), WatcherError> {
        if matches!(&event.event_type, EventType::Delete) {
            return Ok(());
        }

        let Some(move_id) = event.resource.path.as_str().strip_prefix(MOVES_PATH) else {
            return Ok(());
        };
        if move_id.is_empty() || move_id.contains('/') {
            return Ok(());
        }

        let resource = event.resource.to_string();
        let response = self.reader.get_resource(&resource).await?;
        if !response.status.is_success() {
            return Err(
                io::Error::other(format!("resource read failed with {}", response.status)).into(),
            );
        }

        let (body, exceeded) = read_stream_capped(response.body, MAX_MOVE_BYTES).await?;
        if exceeded {
            self.reject(format!("ignored oversized move {move_id}"))
                .await;
            return Ok(());
        }
        let next: Move = match serde_json::from_slice(&body) {
            Ok(next) => next,
            Err(_) => {
                self.reject(format!("ignored malformed move {move_id}"))
                    .await;
                return Ok(());
            }
        };

        let owner = event.resource.owner.z32();
        let result = self
            .state
            .game
            .write()
            .await
            .apply_move(&resource, &owner, next);
        match result {
            ApplyResult::Applied { resized_to } => {
                debug!(%owner, ?resized_to, "applied watched move");
                self.state.notify(if resized_to.is_some() {
                    "resize"
                } else {
                    "move"
                });
            }
            ApplyResult::Duplicate => {}
            ApplyResult::Rejected(reason) => self.reject(reason).await,
        }
        Ok(())
    }
}

impl MoveHandler {
    async fn reject(&self, reason: String) {
        self.state.game.write().await.note("rejected", reason);
        self.state.notify("rejected");
    }
}

pub async fn run(state: AppState, shutdown_rx: watch::Receiver<bool>) {
    let mut interval = tokio::time::interval(Duration::from_millis(750));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = interval.tick() => poll_once(&state, shutdown_rx.clone()).await,
            changed = wait_for_shutdown(shutdown_rx.clone()) => {
                if changed {
                    break;
                }
            }
        }
    }
}

async fn wait_for_shutdown(mut shutdown_rx: watch::Receiver<bool>) -> bool {
    shutdown_rx.changed().await.is_err() || *shutdown_rx.borrow()
}

async fn poll_once(state: &AppState, shutdown_rx: watch::Receiver<bool>) {
    let registrations: Vec<Registration> =
        state.registrations.read().await.values().cloned().collect();
    let mut groups: HashMap<String, Vec<Registration>> = HashMap::new();
    for registration in registrations {
        groups
            .entry(registration.homeserver.clone())
            .or_default()
            .push(registration);
    }

    for (homeserver, registrations) in groups {
        if *shutdown_rx.borrow() {
            return;
        }
        if let Err(error) =
            poll_homeserver(state, &homeserver, registrations, shutdown_rx.clone()).await
        {
            warn!(%homeserver, %error, "watcher poll failed; cursor retained for retry");
            state.game.write().await.note(
                "retry",
                format!("Watcher will retry {}: {error}", short_key(&homeserver)),
            );
            state.notify("retry");
        }
    }
}

async fn poll_homeserver(
    state: &AppState,
    homeserver: &str,
    registrations: Vec<Registration>,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<(), WatcherError> {
    let homeserver = homeserver.parse::<PublicKey>()?;
    let users = registrations
        .iter()
        .map(|registration| {
            Ok((
                registration.public_key.parse::<PublicKey>()?,
                EventCursor::new(registration.cursor),
            ))
        })
        .collect::<Result<Vec<_>, WatcherError>>()?;

    let watcher = Watcher::key_stream(state.client.clone(), homeserver, users)
        .handler(MoveHandler {
            state: state.clone(),
            reader: Arc::new(state.client.clone()),
        })
        .events_limit(EVENTS_PER_POLL)
        .path(MOVES_PATH)
        .build(shutdown_rx)?;
    let outcome = watcher.run().await?;

    let mut current = state.registrations.write().await;
    for (public_key, cursor) in outcome.cursors {
        if let Some(registration) = current.get_mut(&public_key.z32()) {
            registration.cursor = cursor.id();
        }
    }
    Ok(())
}

fn short_key(key: &str) -> &str {
    key.get(..8).unwrap_or(key)
}
