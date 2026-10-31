import type { EasyKind } from "./api";

/** Servers on this computer: names and states the pages share. */
export const KIND: Record<EasyKind, string> = { vanilla: "Vanilla", paper: "Paper", fabric: "Fabric", forge: "Forge", neoforge: "NeoForge" };

export const STATE: Record<string, { key: string; label: string }> = {
  stopped: { key: "idle", label: "Stopped" },
  starting: { key: "preparing", label: "Starting…" },
  running: { key: "running", label: "Online" },
  stopping: { key: "preparing", label: "Stopping…" },
};

/** 3725 → "1 h 2 min" */
export function duration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${h} h ${m % 60} min`;
  return `${Math.floor(h / 24)} days ${h % 24} h`;
}

/** Never-seen dates from the server's own player cache come as year −262143. */
export const known = (iso: string | null | undefined) => !!iso && new Date(iso).getFullYear() > 2000;

export const head = (name: string, size = 24) => `https://mc-heads.net/avatar/${encodeURIComponent(name)}/${size}`;

export const NAME = /^[A-Za-z0-9_]{3,16}$/;
