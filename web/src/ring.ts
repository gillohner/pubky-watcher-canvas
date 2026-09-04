import {
  AuthFlowKind,
  Pubky,
  type GrantAuthFlow,
  type Session,
} from "@synonymdev/pubky";
import type { Identity } from "./types";

const APP_CLIENT_ID = "pubky-watcher-canvas";
const CAPABILITIES = "/pub/pubky-watcher-canvas/:rw" as const;
const SESSION_KEY = `${APP_CLIENT_ID}:session`;
const pubky = new Pubky();

export type RingAttempt = {
  authorizationUrl: string;
  awaitApproval: Promise<Session>;
  cancel: () => void;
};

export async function startRingAuth(): Promise<RingAttempt> {
  const flow = await pubky.startGrantAuthFlow(CAPABILITIES, AuthFlowKind.signin(), {
    clientId: APP_CLIENT_ID,
  });
  const authorizationUrl = flow.authorizationUrl;
  if (!authorizationUrl.startsWith("pubkyauth://")) {
    flow.free();
    throw new Error("Ring authorization did not produce a pubkyauth:// request");
  }

  const approval = awaitRingApproval(flow);
  return {
    authorizationUrl,
    awaitApproval: approval.awaitApproval,
    cancel: approval.cancel,
  };
}

export async function restoreSavedIdentity(): Promise<Identity | undefined> {
  const savedId = localStorage.getItem(SESSION_KEY);
  if (!savedId) return undefined;

  const store = pubky.browserSessionStore;
  try {
    const session = await store.restore(savedId);
    return identityFromSession(session);
  } catch (error) {
    if (!isInvalidSavedSessionError(error)) throw error;
    localStorage.removeItem(SESSION_KEY);
    try {
      await store.remove(savedId);
    } catch {
      // IndexedDB may already have removed an invalid record.
    }
    return undefined;
  } finally {
    store.free();
  }
}

export async function signOut(identity: Identity): Promise<void> {
  const savedId = localStorage.getItem(SESSION_KEY);
  await identity.session.signout();
  localStorage.removeItem(SESSION_KEY);
  if (!savedId) return;

  const store = pubky.browserSessionStore;
  try {
    await store.remove(savedId);
  } finally {
    store.free();
  }
}

function awaitRingApproval(flow: GrantAuthFlow) {
  let cancelled = false;
  let freed = false;
  const free = () => {
    if (freed) return;
    freed = true;
    try {
      flow.free();
    } catch {
      // The SDK may already have consumed the WASM handle after approval.
    }
  };
  const cancel = () => {
    cancelled = true;
    free();
  };

  const awaitApproval = (async () => {
    try {
      const session = await flow.awaitApproval();
      if (cancelled) throw new Error("Pubky Ring sign-in cancelled");
      return session;
    } catch (error) {
      if (cancelled) throw new Error("Pubky Ring sign-in cancelled");
      throw error;
    } finally {
      free();
    }
  })();
  return { awaitApproval, cancel };
}

export async function saveSession(session: Session): Promise<Identity> {
  const store = pubky.browserSessionStore;
  try {
    const stored = await store.save(session);
    try {
      localStorage.setItem(SESSION_KEY, stored.id);
    } finally {
      stored.free();
    }
  } finally {
    store.free();
  }
  return identityFromSession(session);
}

function identityFromSession(session: Session): Identity {
  const info = session.info;
  const publicKey = info.publicKey;
  try {
    return { session, publicKey: publicKey.z32() };
  } finally {
    publicKey.free();
    info.free();
  }
}

function isInvalidSavedSessionError(error: unknown): boolean {
  return error instanceof Error
    && ["AuthenticationError", "InvalidInput", "ClientStateError"].includes(error.name);
}
