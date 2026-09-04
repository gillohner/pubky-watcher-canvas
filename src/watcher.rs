use std::{
    collections::HashMap,
    io,
    sync::Arc,
    time::{Duration, Instant},
};

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
const EVENTS_PER_POLL: u16 = 50;
const MAX_MOVE_BYTES: usize = 1_024;
const SCHEDULER_TICK: Duration = Duration::from_secs(1);
const HEALTHY_POLL_INTERVAL: Duration = Duration::from_secs(60);
const MIN_TRIGGERED_POLL_INTERVAL: Duration = Duration::from_secs(10);
const INITIAL_RETRY_BACKOFF: Duration = Duration::from_secs(10);
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(60);

#[derive(Debug)]
struct PollSchedule {
    next_attempt: Instant,
    retry_backoff: Duration,
    last_attempt: Option<Instant>,
    backing_off: bool,
}

impl PollSchedule {
    fn ready(now: Instant) -> Self {
        Self {
            next_attempt: now,
            retry_backoff: INITIAL_RETRY_BACKOFF,
            last_attempt: None,
            backing_off: false,
        }
    }

    fn is_due(&self, now: Instant) -> bool {
        now >= self.next_attempt
    }

    fn record_success(&mut self, now: Instant) {
        self.last_attempt = Some(now);
        self.next_attempt = now + HEALTHY_POLL_INTERVAL;
        self.retry_backoff = INITIAL_RETRY_BACKOFF;
        self.backing_off = false;
    }

    fn record_failure(&mut self, now: Instant) -> Duration {
        let delay = self.retry_backoff;
        self.last_attempt = Some(now);
        self.next_attempt = now + delay;
        self.retry_backoff = self.retry_backoff.saturating_mul(2).min(MAX_RETRY_BACKOFF);
        self.backing_off = true;
        delay
    }

    fn request_poll(&mut self, now: Instant) {
        if self.backing_off {
            return;
        }

        let earliest = self.last_attempt.map_or(now, |last_attempt| {
            last_attempt + MIN_TRIGGERED_POLL_INTERVAL
        });
        self.next_attempt = self.next_attempt.min(now.max(earliest));
    }
}

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
    let mut schedules = HashMap::new();
    let mut interval = tokio::time::interval(SCHEDULER_TICK);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = interval.tick() => {
                poll_once(&state, &mut schedules, shutdown_rx.clone()).await
            },
            _ = state.watcher_requests.notified() => {
                let now = Instant::now();
                for schedule in schedules.values_mut() {
                    schedule.request_poll(now);
                }
                poll_once(&state, &mut schedules, shutdown_rx.clone()).await;
            },
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

async fn poll_once(
    state: &AppState,
    schedules: &mut HashMap<String, PollSchedule>,
    shutdown_rx: watch::Receiver<bool>,
) {
    let registrations: Vec<Registration> =
        state.registrations.read().await.values().cloned().collect();
    // A user key identifies the tenant; its resolved homeserver key identifies
    // the server endpoint. Grouping shares that endpoint, not cursors or feeds:
    // KeyStreamWatcher still requests and advances each user stream separately.
    let mut groups: HashMap<String, Vec<Registration>> = HashMap::new();
    for registration in registrations {
        groups
            .entry(registration.homeserver.clone())
            .or_default()
            .push(registration);
    }
    schedules.retain(|homeserver, _| groups.contains_key(homeserver));

    for (homeserver, registrations) in groups {
        if *shutdown_rx.borrow() {
            return;
        }

        let now = Instant::now();
        let is_due = schedules
            .get(&homeserver)
            .is_none_or(|schedule| schedule.is_due(now));
        if !is_due {
            continue;
        }

        let result = poll_homeserver(state, &homeserver, registrations, shutdown_rx.clone()).await;
        let schedule = schedules
            .entry(homeserver.clone())
            .or_insert_with(|| PollSchedule::ready(now));

        match result {
            Ok(()) => schedule.record_success(Instant::now()),
            Err(error) => {
                let retry_after = schedule.record_failure(Instant::now());
                warn!(
                    %homeserver,
                    %error,
                    retry_after_seconds = retry_after.as_secs(),
                    "watcher poll failed; cursor retained for retry"
                );
                state.game.write().await.note(
                    "retry",
                    format!(
                        "Watcher paused {}s for {} after an error: {error}",
                        retry_after.as_secs(),
                        short_key(&homeserver)
                    ),
                );
                state.notify("retry");
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_poll_waits_for_healthy_interval() {
        let now = Instant::now();
        let mut schedule = PollSchedule::ready(now);

        assert!(schedule.is_due(now));
        schedule.record_success(now);

        assert!(!schedule.is_due(now + HEALTHY_POLL_INTERVAL - Duration::from_millis(1)));
        assert!(schedule.is_due(now + HEALTHY_POLL_INTERVAL));
    }

    #[test]
    fn failures_back_off_exponentially_and_stop_at_cap() {
        let mut now = Instant::now();
        let mut schedule = PollSchedule::ready(now);

        for expected_seconds in [10, 20, 40, 60, 60, 60] {
            let delay = schedule.record_failure(now);
            assert_eq!(delay, Duration::from_secs(expected_seconds));
            assert!(!schedule.is_due(now + delay - Duration::from_millis(1)));
            now += delay;
            assert!(schedule.is_due(now));
        }
    }

    #[test]
    fn success_resets_retry_backoff() {
        let now = Instant::now();
        let mut schedule = PollSchedule::ready(now);

        schedule.record_failure(now);
        schedule.record_success(now);

        assert_eq!(
            schedule.record_failure(now + HEALTHY_POLL_INTERVAL),
            INITIAL_RETRY_BACKOFF
        );
    }

    #[test]
    fn requested_poll_advances_a_healthy_schedule() {
        let now = Instant::now();
        let mut schedule = PollSchedule::ready(now);
        schedule.record_success(now);

        schedule.request_poll(now + Duration::from_secs(1));

        assert!(!schedule.is_due(now + MIN_TRIGGERED_POLL_INTERVAL - Duration::from_millis(1)));
        assert!(schedule.is_due(now + MIN_TRIGGERED_POLL_INTERVAL));
    }

    #[test]
    fn requested_poll_does_not_bypass_error_backoff() {
        let now = Instant::now();
        let mut schedule = PollSchedule::ready(now);
        let retry_after = schedule.record_failure(now);

        schedule.request_poll(now + Duration::from_secs(1));

        assert!(!schedule.is_due(now + retry_after - Duration::from_millis(1)));
        assert!(schedule.is_due(now + retry_after));
    }
}
