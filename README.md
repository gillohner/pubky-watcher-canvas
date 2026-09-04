# Pubky Watcher Canvas

A deliberately small multiplayer pixel game for understanding [`pubky-watcher`](https://github.com/tipogi/pubky-nexus/pull/7). Players authenticate through Pubky Ring's cookie-auth flow, publish moves to their own homeserver, and watch a Rust service discover those moves through `/events-stream`.

**Live demo:** <https://eventky.app/watcher-canvas/>

The board begins at **1×1** and grows through exactly:

```text
1 → 2 → 4 → 8 → 16 → 24 → 32 → 48 → 64
```

Each stage unlocks when every visible cell has been painted. There is no different-user overwrite gate and no credit timer, so a single person can exercise the early resize stages. `64×64` is the hard limit.

## What this demonstrates

```text
Browser                         Player homeserver                Rust demo service
   │                                   │                                │
   ├─ pubkyauth://signin ► Ring QR                                       │
   │◄──────── SDK Session via relay ───┤                                │
   │                                   │                                │
   ├─ session.storage.putJson(move) ──►│                                │
   │                                   │◄── pubky-watcher events-stream ┤
   │                                   ├── move resource ──────────────►│
   │◄──────────────────────── SSE board update ─────────────────────────┤
```

Important boundaries:

- The canvas uses `startCookieAuthFlow()` so it works before Ring supports grant auth.
- Only `AuthFlow.awaitApproval()` returning a Pubky SDK `Session` authenticates the player.
- `session.export()` persists non-secret cookie-session metadata in `localStorage`; the actual
  session cookie remains HTTP-only and `restoreSession()` restores the SDK session after reload.
- The client requests only `/pub/pubky-watcher-canvas/:rw`.
- The service constructs one `WatcherClient` and injects clones into `Watcher::key_stream` and the move handler.
- Every user key has its own event stream and cursor. Users are grouped by their resolved homeserver
  key only because `key_stream` targets one server endpoint and polls each hosted user separately.
- The watcher owns Pubky transport. The demo owns cursors, polling, move validation, board rules, and SSE.
- State and cursors are intentionally in memory. Restarting the server resets the demo.

The watcher dependency is pinned to the exact commit from the draft PR so the example stays reproducible while the API is under review.

## Run it

Requirements: Rust stable, Node.js 24+, a Pubky identity, and Pubky Ring.

```bash
cd web
npm install
cd ..
./start.sh
```

Open <http://localhost:5173>. Vite proxies `/api` to the Rust service at `127.0.0.1:3001`.

The Ring authorization link is a sensitive, short-lived `pubkyauth://` request. The UI can render,
open, or copy it but never logs it. Set `VITE_API_URL` only when the frontend and API are hosted on
different origins.

## Read the important parts

- `web/src/ring.ts` — Ring cookie-auth lifecycle, approval, metadata persistence, reload
  restoration, and sign out.
- `web/src/moves.ts` — writes one tiny JSON move through the authenticated session.
- `src/watcher.rs` — groups registered keys by homeserver, injects `WatcherClient`, advances cursors only after successful handling, reads resources, and applies moves.
- `src/game.rs` — validation and the requested resize sequence.
- `src/api.rs` — key registration, board snapshot, and live SSE notification endpoints.

## Verify it

```bash
cargo test
cd web
npm run typecheck
npm run build
```

This is an educational demo, not a durable indexer: it does not persist cursors or board state, implement retry backoff, or protect its public registration endpoint. Those policies deliberately remain visible as application responsibilities rather than being mistaken for watcher transport behavior.

## Deploy it

The production image builds the React UI and serves it from the same Rust process as the API and
watcher. `compose.yaml` binds the container to `127.0.0.1:3002`, leaving public exposure to an
existing reverse proxy. The included `deploy/nginx-location.conf` mounts it at
`https://eventky.app/watcher-canvas/` without claiming another public port.

GitHub Pages cannot run this application by itself: Pages serves static files, while the watcher
must remain alive to poll homeservers and hold an SSE connection. The GitHub repository is the
source of truth; the runnable demo is deployed as the isolated container described above.
