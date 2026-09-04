import {
  AuthFlowKind,
  Pubky,
  type AuthFlow,
  type Session,
} from "@synonymdev/pubky";
import type { Identity } from "./types";

const APP_CLIENT_ID = "pubky-watcher-canvas";
const CAPABILITIES = "/pub/pubky-watcher-canvas/:rw" as const;
const SESSION_KEY = `${APP_CLIENT_ID}:cookie-session`;
const LEGACY_GRANT_SESSION_KEY = `${APP_CLIENT_ID}:session`;
const pubky = new Pubky();

export type RingAttempt = {
  authorizationUrl: string;
  awaitApproval: Promise<Session>;
  cancel: () => void;
};

export function startRingAuth(): RingAttempt {
  const flow = pubky.startCookieAuthFlow(CAPABILITIES, AuthFlowKind.signin());
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
  localStorage.removeItem(LEGACY_GRANT_SESSION_KEY);
  const exported = localStorage.getItem(SESSION_KEY);
  if (!exported) return undefined;

  try {
    const session = await pubky.restoreSession(exported);
    return identityFromSession(session);
  } catch (error) {
    if (!isInvalidSavedSessionError(error)) throw error;
    localStorage.removeItem(SESSION_KEY);
    return undefined;
  }
}

export async function signOut(identity: Identity): Promise<void> {
  try {
    await identity.session.signout();
  } finally {
    localStorage.removeItem(SESSION_KEY);
  }
}

function awaitRingApproval(flow: AuthFlow) {
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
  localStorage.setItem(SESSION_KEY, session.export());
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
