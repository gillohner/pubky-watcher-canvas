use std::convert::Infallible;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive},
        Json, Sse,
    },
    routing::{get, post, put},
    Router,
};
use pubky::PublicKey;
use pubky_watcher::HomeserverResolver;
use serde::Serialize;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use crate::{
    game::GameSnapshot,
    state::{AppState, Registration},
};

const MAX_WATCHED_USERS: usize = 64;

#[derive(Serialize)]
struct Snapshot {
    #[serde(flatten)]
    game: GameSnapshot,
    watched_users: Vec<Registration>,
}

#[derive(Serialize)]
struct Health {
    ok: bool,
}

pub fn router(state: AppState, static_dir: &str) -> Router {
    let index = format!("{static_dir}/index.html");
    let files = ServeDir::new(static_dir).not_found_service(ServeFile::new(index));
    Router::new()
        .route("/api/health", get(health))
        .route("/api/board", get(board))
        .route("/api/events", get(events))
        .route("/api/poll", post(request_watcher_poll))
        .route("/api/watch/{public_key}", put(watch_user))
        .layer(TraceLayer::new_for_http())
        .fallback_service(files)
        .with_state(state)
}

async fn health() -> Json<Health> {
    Json(Health { ok: true })
}

async fn board(State(state): State<AppState>) -> Json<Snapshot> {
    let game = state.game.read().await.snapshot();
    let mut watched_users: Vec<_> = state.registrations.read().await.values().cloned().collect();
    watched_users.sort_by(|left, right| left.public_key.cmp(&right.public_key));
    Json(Snapshot {
        game,
        watched_users,
    })
}

async fn request_watcher_poll(State(state): State<AppState>) -> StatusCode {
    state.request_watcher_poll();
    StatusCode::ACCEPTED
}

async fn watch_user(
    State(state): State<AppState>,
    Path(public_key): Path<String>,
) -> Result<Json<Registration>, (StatusCode, String)> {
    let user = public_key.parse::<PublicKey>().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "invalid Pubky public key".to_owned(),
        )
    })?;
    let homeserver = state
        .client
        .resolve_homeserver(&user)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "the user's homeserver could not be resolved".to_owned(),
            )
        })?;

    let registration = Registration {
        public_key: user.z32(),
        homeserver: homeserver.z32(),
        cursor: 0,
    };
    let inserted = {
        let mut registrations = state.registrations.write().await;
        if registrations.len() >= MAX_WATCHED_USERS
            && !registrations.contains_key(&registration.public_key)
        {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                format!("this demo is limited to {MAX_WATCHED_USERS} watched keys"),
            ));
        }
        registrations
            .entry(registration.public_key.clone())
            .or_insert_with(|| registration.clone())
            .clone()
    };
    state.game.write().await.note(
        "watcher",
        format!(
            "Watching {} on homeserver {}",
            short_key(&inserted.public_key),
            short_key(&inserted.homeserver)
        ),
    );
    state.notify("watcher");
    Ok(Json(inserted))
}

async fn events(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let stream =
        BroadcastStream::new(state.updates.subscribe()).filter_map(|message| match message {
            Ok(kind) => Some(Ok(Event::default().event("update").data(kind))),
            Err(_) => None,
        });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn internal_error(error: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::BAD_GATEWAY, error.to_string())
}

fn short_key(key: &str) -> String {
    if key.len() <= 12 {
        key.to_owned()
    } else {
        format!("{}…{}", &key[..7], &key[key.len() - 4..])
    }
}
