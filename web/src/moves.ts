import type { Session } from "@synonymdev/pubky";

export async function publishMove(
  session: Session,
  x: number,
  y: number,
  color: number,
): Promise<void> {
  const id = `${Date.now()}-${crypto.randomUUID()}`;
  const path = `/pub/pubky-watcher-canvas/moves/${id}` as `/pub/${string}`;
  await session.storage.putJson(path, { x, y, color });
}
