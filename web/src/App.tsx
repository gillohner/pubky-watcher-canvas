import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import QRCode from "qrcode";
import { getBoard, registerWatcher, requestWatcherPoll, subscribeToBoard } from "./api";
import { publishMove } from "./moves";
import {
  restoreSavedIdentity,
  saveSession,
  signOut,
  startRingAuth,
  type RingAttempt,
} from "./ring";
import type { Identity, Snapshot } from "./types";

export function App() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [identity, setIdentity] = useState<Identity | null>(null);
  const [selectedColor, setSelectedColor] = useState(6);
  const [status, setStatus] = useState("Loading watcher…");
  const [busy, setBusy] = useState(false);
  const [ringQr, setRingQr] = useState<string | null>(null);
  const [ringUrl, setRingUrl] = useState<string | null>(null);
  const ringAttempt = useRef<RingAttempt | null>(null);

  const refresh = useCallback(async () => {
    try {
      setSnapshot(await getBoard());
    } catch (error) {
      setStatus(message(error));
    }
  }, []);

  useEffect(() => {
    void refresh();
    return subscribeToBoard(() => void refresh());
  }, [refresh]);

  const activateIdentity = useCallback(async (nextIdentity: Identity, restored = false) => {
    // The SDK session is already authenticated and persisted. Watcher
    // registration is separate and must not make the UI forget that session.
    setIdentity(nextIdentity);
    setStatus(restored ? "Session restored. Registering the watcher…" : "Signed in. Registering the watcher…");
    try {
      await registerWatcher(nextIdentity.publicKey);
      setStatus("Signed in. The watcher is following your homeserver event stream.");
      await refresh();
    } catch (error) {
      setStatus(`Signed in, but watcher registration failed: ${message(error)}`);
    }
  }, [refresh]);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const saved = await restoreSavedIdentity();
        if (!active) return;
        if (saved) await activateIdentity(saved, true);
        else setStatus("Sign in with Pubky Ring to paint a pixel.");
      } catch (error) {
        if (active) setStatus(message(error));
      }
    })();
    return () => {
      active = false;
    };
  }, [activateIdentity]);

  useEffect(() => () => ringAttempt.current?.cancel(), []);

  const pixels = useMemo(
    () => new Map(snapshot?.pixels.map((pixel) => [`${pixel.x}:${pixel.y}`, pixel])),
    [snapshot],
  );

  const showRingQr = async () => {
    let attempt: RingAttempt | undefined;
    let approvalAccepted = false;
    setBusy(true);
    setStatus("Creating a Pubky Ring cookie-auth request…");
    try {
      attempt = await startRingAuth();
      ringAttempt.current = attempt;
      setRingUrl(attempt.authorizationUrl);
      setRingQr(await QRCode.toDataURL(attempt.authorizationUrl, {
        width: 320,
        margin: 2,
        errorCorrectionLevel: "M",
        color: { dark: "#090b0f", light: "#ffffff" },
      }));
      setStatus("Scan the QR with Pubky Ring, review the capability, and approve.");
      setBusy(false);

      const session = await attempt.awaitApproval;
      if (ringAttempt.current !== attempt) {
        session.free();
        return;
      }
      approvalAccepted = true;
      ringAttempt.current = null;
      setRingQr(null);
      setRingUrl(null);
      setBusy(true);
      const nextIdentity = await saveSession(session);
      await activateIdentity(nextIdentity);
    } catch (error) {
      if (!approvalAccepted && attempt && ringAttempt.current !== attempt) return;
      ringAttempt.current = null;
      setStatus(message(error));
      setRingQr(null);
      setRingUrl(null);
    } finally {
      setBusy(false);
    }
  };

  const closeRingQr = () => {
    ringAttempt.current?.cancel();
    ringAttempt.current = null;
    setRingQr(null);
    setRingUrl(null);
    setBusy(false);
    setStatus("Ring sign-in cancelled.");
  };

  const disconnect = async () => {
    if (!identity || busy) return;
    setBusy(true);
    setStatus("Signing out…");
    try {
      await signOut(identity);
      setIdentity(null);
      setStatus("Signed out. Sign in with Pubky Ring to paint again.");
    } catch (error) {
      setStatus(message(error));
    } finally {
      setBusy(false);
    }
  };

  const copyRingUrl = async () => {
    if (!ringUrl) return;
    try {
      await navigator.clipboard.writeText(ringUrl);
      setStatus("Pubky Ring authorization link copied.");
    } catch (error) {
      setStatus(message(error));
    }
  };

  const paint = async (x: number, y: number) => {
    if (!identity || busy) return;
    setBusy(true);
    setStatus(`Publishing (${x}, ${y}) to your Pubky homeserver…`);
    try {
      await publishMove(identity.session, x, y, selectedColor);
      await requestWatcherPoll();
      setStatus("Published. Waiting for the watcher to observe and index it…");
    } catch (error) {
      setStatus(message(error));
    } finally {
      setBusy(false);
    }
  };

  if (!snapshot) {
    return <main className="loading">{status}</main>;
  }

  const progress = Math.round((snapshot.filled / snapshot.total) * 100);
  return (
    <main>
      <header>
        <div>
          <p className="eyebrow">PUBKY WATCHER / LIVE DEMO</p>
          <h1>Watch pixels travel.</h1>
          <p className="lede">
            Paint through your Pubky session. The server only learns about the move when the
            watcher sees it on your homeserver.
          </p>
        </div>
        {identity ? (
          <div className="signed-in">
            <div className="identity" title={identity.publicKey}>
              <span className="live-dot" /> {shortKey(identity.publicKey)}
            </div>
            <button className="ring-button" onClick={() => void disconnect()} disabled={busy}>
              Sign out
            </button>
          </div>
        ) : (
          <div className="auth-actions">
            <button className="connect" onClick={() => void showRingQr()} disabled={busy}>
              Sign in with Pubky Ring
            </button>
          </div>
        )}
      </header>

      <section className="pipeline" aria-label="Data flow">
        <Step number="1" title="Pubky Ring" detail="Cookie session" active={busy && !identity} />
        <Arrow />
        <Step number="2" title="Your homeserver" detail="Stores move JSON" active={busy && !!identity} />
        <Arrow />
        <Step number="3" title="Watcher" detail="Polls /events-stream" active={snapshot.watched_users.length > 0} />
        <Arrow />
        <Step number="4" title="This board" detail="Indexes + streams" active={snapshot.processed_moves > 0} />
      </section>

      <div className="workspace">
        <section className="game-card">
          <div className="game-head">
            <div>
              <span className="stage-label">CURRENT STAGE</span>
              <strong>{snapshot.size} × {snapshot.size}</strong>
            </div>
            <div className="progress-copy">
              <span>{snapshot.filled}/{snapshot.total} cells</span>
              <span>{progress}%</span>
            </div>
          </div>
          <div className="progress"><span style={{ width: `${progress}%` }} /></div>

          <div className="canvas-wrap">
            <div
              className="canvas"
              style={{
                gridTemplateColumns: `repeat(${snapshot.size}, 1fr)`,
                gridTemplateRows: `repeat(${snapshot.size}, 1fr)`,
              }}
            >
              {Array.from({ length: snapshot.total }, (_, index) => {
                const x = index % snapshot.size;
                const y = Math.floor(index / snapshot.size);
                const pixel = pixels.get(`${x}:${y}`);
                return (
                  <button
                    key={`${x}:${y}`}
                    className="pixel"
                    style={{ background: pixel ? snapshot.colors[pixel.color] : "#161920" }}
                    title={pixel ? `${x},${y} — ${shortKey(pixel.owner)}` : `${x},${y}`}
                    onClick={() => void paint(x, y)}
                    disabled={!identity || busy}
                    aria-label={`Paint cell ${x}, ${y}`}
                  />
                );
              })}
            </div>
          </div>

          <div className="palette" aria-label="Color palette">
            {snapshot.colors.map((color, index) => (
              <button
                key={color}
                className={index === selectedColor ? "swatch selected" : "swatch"}
                style={{ background: color }}
                onClick={() => setSelectedColor(index)}
                aria-label={`Select color ${index + 1}`}
              />
            ))}
          </div>
          <p className="status">{status}</p>
        </section>

        <aside>
          <section className="panel">
            <p className="panel-title">EXPANSION PATH</p>
            <div className="sizes">
              {snapshot.sizes.map((size) => (
                <span key={size} className={size === snapshot.size ? "current" : size < snapshot.size ? "done" : ""}>
                  {size}
                </span>
              ))}
            </div>
            <p className="muted">
              Filling every visible cell unlocks {snapshot.next_size ? `${snapshot.next_size}×${snapshot.next_size}` : "nothing else — 64×64 is the limit"}.
              Repainting is allowed, but does not count as a new cell.
            </p>
          </section>

          <section className="panel activity-panel">
            <div className="panel-row">
              <p className="panel-title">WATCHER ACTIVITY</p>
              <span className="watch-count">{snapshot.watched_users.length} key{snapshot.watched_users.length === 1 ? "" : "s"}</span>
            </div>
            <ol className="activity">
              {snapshot.activity.map((item) => (
                <li key={item.id}>
                  <span className={`event-icon ${item.kind}`} />
                  <div><strong>{item.kind}</strong><p>{item.message}</p></div>
                </li>
              ))}
            </ol>
          </section>
        </aside>
      </div>

      {ringQr && ringUrl && (
        <div className="modal-backdrop" role="dialog" aria-modal="true" aria-label="Pubky Ring sign in">
          <div className="qr-modal">
            <button className="modal-close" onClick={closeRingQr} aria-label="Close Ring QR">×</button>
            <p className="eyebrow">PUBKYAUTH:// COOKIE AUTH</p>
            <h2>Scan with Pubky Ring</h2>
            <p>Ring will independently show the requested canvas capability before you approve.</p>
            <img src={ringQr} alt="Pubky Ring authorization QR code" />
            <div className="ring-links">
              <a className="connect" href={ringUrl}>Authorize with Pubky Ring</a>
              <button className="ring-button" onClick={() => void copyRingUrl()}>Copy link</button>
            </div>
            <small>The SDK creates the cookie session only after Ring approves the request.</small>
          </div>
        </div>
      )}
    </main>
  );
}

function Step({ number, title, detail, active }: { number: string; title: string; detail: string; active: boolean }) {
  return <div className={active ? "step active" : "step"}><span>{number}</span><div><strong>{title}</strong><small>{detail}</small></div></div>;
}

function Arrow() {
  return <span className="arrow">→</span>;
}

function shortKey(key: string): string {
  return key.length > 15 ? `${key.slice(0, 8)}…${key.slice(-4)}` : key;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "Something went wrong";
}
