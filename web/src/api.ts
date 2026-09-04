import type { Registration, Snapshot } from "./types";

const API = import.meta.env.VITE_API_URL?.replace(/\/$/, "")
  ?? import.meta.env.BASE_URL.replace(/\/$/, "");

export async function getBoard(): Promise<Snapshot> {
  const response = await fetch(`${API}/api/board`);
  if (!response.ok) throw new Error(`Board API returned ${response.status}`);
  return response.json() as Promise<Snapshot>;
}

export async function registerWatcher(publicKey: string): Promise<Registration> {
  const response = await fetch(`${API}/api/watch/${encodeURIComponent(publicKey)}`, {
    method: "PUT",
  });
  if (!response.ok) throw new Error(await response.text());
  return response.json() as Promise<Registration>;
}

export function subscribeToBoard(onUpdate: () => void): () => void {
  const events = new EventSource(`${API}/api/events`);
  events.addEventListener("update", onUpdate);
  return () => events.close();
}
