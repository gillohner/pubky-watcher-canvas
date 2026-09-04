import { AuthFlowKind, Pubky, type GrantAuthFlow, type Session } from "@synonymdev/pubky";
import type { Identity } from "./types";

const PASSPORT_ORIGIN = import.meta.env.VITE_PASSPORT_ORIGIN ?? "https://passport.pubky.app";
const PUBLIC_ORIGIN = import.meta.env.VITE_PUBLIC_ORIGIN ?? window.location.origin;
const CALLBACK_MESSAGE = "pubky-watcher-canvas.passport-return";
const TIMEOUT_MS = 5 * 60_000;
const CAPABILITIES = "/pub/pubky-watcher-canvas/:rw" as const;
const pubky = new Pubky();

type Outcome = "success" | "error" | "cancel";

export async function signInWithPassport(): Promise<Identity> {
  const attemptId = crypto.randomUUID();
  const popup = window.open(
    "about:blank",
    `pubky-passport-${attemptId}`,
    "popup,width=520,height=760",
  );
  if (!popup) throw new Error("Passport popup was blocked");

  let outcome: Outcome | undefined;
  let flow: GrantAuthFlow | undefined;
  const onMessage = (event: MessageEvent<unknown>) => {
    if (event.source !== popup || !isRecord(event.data)) return;
    if (
      event.origin === PASSPORT_ORIGIN &&
      event.data.type === "pubky-passport.authorization-outcome" &&
      event.data.version === 1 &&
      isOutcome(event.data.outcome) &&
      typeof event.data.messageId === "string"
    ) {
      popup.postMessage(
        {
          type: "pubky-passport.authorization-outcome-ack",
          version: 1,
          messageId: event.data.messageId,
        },
        PASSPORT_ORIGIN,
      );
      outcome = event.data.outcome;
      return;
    }
    if (
      event.origin === window.location.origin &&
      event.data.type === CALLBACK_MESSAGE &&
      event.data.attemptId === attemptId &&
      isOutcome(event.data.outcome)
    ) {
      outcome = event.data.outcome;
    }
  };
  window.addEventListener("message", onMessage);

  try {
    const callback = (nextOutcome: Outcome) => {
      const url = new URL(import.meta.env.BASE_URL, PUBLIC_ORIGIN);
      url.searchParams.set("passport-attempt", attemptId);
      url.searchParams.set("passport-outcome", nextOutcome);
      return url.href;
    };
    const secureCallbacks = new URL(PUBLIC_ORIGIN).protocol === "https:";
    flow = await pubky.startGrantAuthFlow(CAPABILITIES, AuthFlowKind.signin(), {
      clientId: "pubky-watcher-canvas",
      ...(secureCallbacks
        ? {
            xCallback: {
              xSource: "Pubky Watcher Canvas",
              xSuccess: callback("success"),
              xError: callback("error"),
              xCancel: callback("cancel"),
            },
          }
        : {}),
    });

    const passportUrl = new URL("/authorize", PASSPORT_ORIGIN);
    passportUrl.hash = `d=${encodeURIComponent(flow.authorizationUrl)}`;
    popup.location.replace(passportUrl.href);

    const deadline = Date.now() + TIMEOUT_MS;
    while (Date.now() < deadline) {
      if (outcome === "cancel") throw new Error("Passport authorization was cancelled");
      if (outcome === "error") throw new Error("Passport could not approve the request");
      if (popup.closed && outcome !== "success") throw new Error("Passport popup was closed");

      // The relay result is authoritative. A popup message never authenticates the player.
      const session = await flow.tryPollOnce();
      if (session) {
        const store = pubky.browserSessionStore;
        try {
          await store.save(session);
        } finally {
          store.free();
        }
        return { session, publicKey: sessionPublicKey(session) };
      }
      await new Promise((resolve) => window.setTimeout(resolve, 250));
    }
    throw new Error("Passport authorization timed out");
  } finally {
    window.removeEventListener("message", onMessage);
    flow?.free();
    try {
      if (!popup.closed) popup.close();
    } catch {
      // Cross-origin popup closing is best effort.
    }
  }
}

export function completePassportCallback(): boolean {
  const params = new URLSearchParams(window.location.search);
  const attemptId = params.get("passport-attempt");
  const value = params.get("passport-outcome");
  const outcome = isOutcome(value) ? value : null;
  if (!attemptId || !outcome) return false;

  if (window.opener && !window.opener.closed) {
    window.opener.postMessage(
      { type: CALLBACK_MESSAGE, attemptId, outcome },
      window.location.origin,
    );
    window.close();
  }
  return true;
}

function sessionPublicKey(session: Session): string {
  const info = session.info;
  const publicKey = info.publicKey;
  try {
    return publicKey.z32();
  } finally {
    publicKey.free();
    info.free();
  }
}

function isOutcome(value: unknown): value is Outcome {
  return value === "success" || value === "error" || value === "cancel";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
