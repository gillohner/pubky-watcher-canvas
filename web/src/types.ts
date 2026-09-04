import type { Session } from "@synonymdev/pubky";

export type Pixel = {
  x: number;
  y: number;
  color: number;
  owner: string;
  received_at: number;
};

export type Activity = {
  id: number;
  kind: string;
  message: string;
  at: number;
};

export type Registration = {
  public_key: string;
  homeserver: string;
  cursor: number;
};

export type Snapshot = {
  size: number;
  next_size: number | null;
  filled: number;
  total: number;
  processed_moves: number;
  pixels: Pixel[];
  activity: Activity[];
  sizes: number[];
  colors: string[];
  watched_users: Registration[];
};

export type Identity = {
  publicKey: string;
  session: Session;
};
