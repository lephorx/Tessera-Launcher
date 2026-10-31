import { listen } from "@tauri-apps/api/event";
import { reactive } from "vue";
import { api } from "./api";
import { toast } from "./toast";

export type RunState = "idle" | "preparing" | "running" | "error";

interface Run {
  state: RunState;
  stage: string;
  done: number;
  total: number;
}

/** Live state of every instance, fed by events from the Rust side. */
export const runs = reactive<Record<string, Run>>({});
/** Modpack download/import in progress (empty stage = nothing running). */
export const packProgress = reactive({ stage: "", done: 0, total: 0 });

/** Pushing an instance's server-side mods to its CSCM servers, by `${instance}:${serverId}`. */
/** `needsRestart`: the server got the mods while running; they're installed when it restarts. */
export const serverSync = reactive<Record<string, { state: "syncing" | "synced" | "failed"; message: string | null; needsRestart?: boolean }>>({});

/** Live share sync per instance: last result, for the Share tab. */
export const shareSync = reactive<Record<string, { state: "pulled" | "published" | "waiting" | "error"; at: number; by: string | null; message: string | null }>>({});
const lastNotice = new Map<string, string>();

export const logListeners = new Set<(instance: string, line: string) => void>();

export function runOf(id: string): Run {
  return runs[id] ?? { state: "idle", stage: "", done: 0, total: 0 };
}

let started = false;
export async function startEvents() {
  if (started) return;
  started = true;
  for (const id of await api.instances.running()) runs[id] = { state: "running", stage: "", done: 0, total: 0 };

  await listen<{ instance: string; stage: string; done: number; total: number }>("launch-progress", ({ payload }) => {
    runs[payload.instance] = { state: "preparing", stage: payload.stage, done: payload.done, total: payload.total };
  });
  await listen<{ instance: string; state: string; message: string | null }>("instance-state", ({ payload }) => {
    const prev = runOf(payload.instance);
    if (payload.state === "stopped") delete runs[payload.instance];
    else runs[payload.instance] = { ...prev, state: payload.state as RunState };
    if (payload.state === "error") {
      toast.error(payload.message ?? "The game couldn't start");
      delete runs[payload.instance];
    } else if (payload.state === "stopped" && payload.message) {
      toast.error(payload.message);
    }
  });
  await listen<{ stage: string; done: number; total: number }>("pack-progress", ({ payload }) => {
    Object.assign(packProgress, payload);
  });
  await listen<{ instance: string; server_id: number; state: "syncing" | "synced" | "failed"; message: string | null; needs_restart: boolean }>("server-sync", ({ payload }) => {
    const key = `${payload.instance}:${payload.server_id}`;
    // still waiting for a restart while the next change is sent
    const waiting = payload.needs_restart || (payload.state === "syncing" && !!serverSync[key]?.needsRestart);
    serverSync[key] = { state: payload.state, message: payload.message, needsRestart: waiting };
    if (payload.state === "synced") toast.success(payload.message ?? "Server updated with the new mods");
    if (payload.state === "failed") toast.error(`Couldn't update the server: ${payload.message ?? "unknown error"}`);
  });
  await listen<{ instance: string; name: string; state: "pulled" | "published" | "waiting" | "error"; by: string | null; message: string | null }>(
    "share-sync",
    ({ payload: p }) => {
      shareSync[p.instance] = { state: p.state, at: Date.now(), by: p.by, message: p.message };
      // tell the instance page to reload what it shows
      if (p.state === "pulled") window.dispatchEvent(new CustomEvent("tessera-instance-changed", { detail: p.instance }));
      // say each kind of thing once, not every few seconds
      const key = `${p.state}:${p.message ?? ""}`;
      if (p.state !== "pulled" && lastNotice.get(p.instance) === key) return;
      lastNotice.set(p.instance, key);
      if (p.state === "pulled") toast.success(`${p.name} updated${p.by ? ` with ${p.by}'s changes` : ""}`);
      else if (p.state === "waiting") toast.info(`${p.name} has new changes. They're applied when you close the game.`);
      else if (p.state === "error") toast.error(`Couldn't sync ${p.name}: ${p.message ?? "unknown error"}`);
    },
  );
  await listen<{ instance: string; line: string }>("instance-log", ({ payload }) => {
    for (const fn of logListeners) fn(payload.instance, payload.line);
  });
}
