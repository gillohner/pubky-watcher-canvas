import { AuthFlowKind, Pubky, type Session } from "@synonymdev/pubky";
import type { Identity } from "./types";

const CAPABILITIES = "/pub/pubky-watcher-canvas/:rw" as const;
const TIMEOUT_MS = 5 * 60_000;
const pubky = new Pubky();

export type RingAttempt = {
  authorizationUrl: string;
  waitForSession: () => Promise<Identity>;
  cancel: () => void;
};

export async function createRingAttempt(): Promise<RingAttempt> {
  // Keep the direct Ring path compatible with the original canvas example:
  // the QR payload is a pubkyauth://signin request. Passport uses grant auth.
  const flow = pubky.startCookieAuthFlow(CAPABILITIES, AuthFlowKind.signin());
  const authorizationUrl = flow.authorizationUrl;
  if (!authorizationUrl.startsWith("pubkyauth://")) {
    flow.free();
    throw new Error("Ring authorization did not produce a pubkyauth:// request");
  }
  let freed = false;
  let cancelled = false;
  const free = () => {
    if (freed) return;
    freed = true;
    flow.free();
  };

  return {
    authorizationUrl,
    waitForSession: async () => {
      try {
        const deadline = Date.now() + TIMEOUT_MS;
        while (Date.now() < deadline) {
          if (cancelled) throw new Error("Ring sign-in cancelled");
          const session = await flow.tryPollOnce();
          if (session) {
            await saveSession(session);
            return { session, publicKey: sessionPublicKey(session) };
          }
          await new Promise((resolve) => window.setTimeout(resolve, 250));
        }
        throw new Error("Ring authorization timed out");
      } finally {
        free();
      }
    },
    cancel: () => {
      cancelled = true;
    },
  };
}

async function saveSession(session: Session): Promise<void> {
  const store = pubky.browserSessionStore;
  try {
    await store.save(session);
  } finally {
    store.free();
  }
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
