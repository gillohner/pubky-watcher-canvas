# Pubky Watcher Canvas

A deliberately small multiplayer pixel game for understanding [`pubky-watcher`](https://github.com/tipogi/pubky-nexus/pull/7). Players authenticate through a grant-based [Pubky Passport](https://passport.pubky.app) popup, publish moves to their own homeserver, and watch a Rust service discover those moves through `/events-stream`.

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
   ├─ start grant auth ──► Passport or Ring QR                           │
   │◄──────── SDK Session via relay ───┤                                │
   │                                   │                                │
   ├─ session.storage.putJson(move) ──►│                                │
   │                                   │◄── pubky-watcher events-stream ┤
   │                                   ├── move resource ──────────────►│
   │◄──────────────────────── SSE board update ─────────────────────────┤
```

Important boundaries:

- Only the Pubky SDK `Session` authenticates the player. Passport popup messages are UI signals.
- The canvas offers two grant-based paths: **Continue with Passport** opens Passport, while
  **Show Ring QR** renders the SDK authorization request directly in the canvas for Ring.
- The client requests only `/pub/pubky-watcher-canvas/:rw`.
- The service constructs one `WatcherClient` and injects clones into `Watcher::key_stream` and the move handler.
- The watcher owns Pubky transport. The demo owns cursors, polling, move validation, board rules, and SSE.
- State and cursors are intentionally in memory. Restarting the server resets the demo.

The watcher dependency is pinned to the exact commit from the draft PR so the example stays reproducible while the API is under review.

## Run it

Requirements: Rust stable, Node.js 24+, a Pubky identity, and Pubky Ring if you use the direct QR route.

```bash
cd web
npm install
cd ..
./start.sh
```

Open <http://localhost:5173>. Vite proxies `/api` to the Rust service at `127.0.0.1:3001`.

Passport requires HTTPS callbacks. For local HTTP development this demo omits callback URLs and keeps polling the SDK relay, which remains the authoritative result. For a deployed build, copy `web/.env.example`, set `VITE_PUBLIC_ORIGIN` to the frontend's HTTPS origin and `VITE_API_URL` to the HTTPS API origin, then rebuild.

## Read the important parts

- `web/src/passport.ts` — grant flow, popup, secure outcome acknowledgement, callback fallback, relay polling, and browser session store.
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
