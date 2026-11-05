import { listen } from "@tauri-apps/api/event";
import { reactive } from "vue";
import { api, type ShareReview } from "./api";
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

type SyncState = "pulled" | "published" | "waiting" | "error" | "paused";
/** Live share sync per instance: last result, for the Share tab. */
export const shareSync = reactive<Record<string, { state: SyncState; at: number; by: string | null; message: string | null }>>({});
const lastNotice = new Map<string, string>();

/** Shared instances waiting for this member's answer (unknown jars, a new version): the popup. */
export const shareReviews = reactive<{ list: ShareReview[] }>({ list: [] });
export async function refreshReviews() {
  try {
    shareReviews.list = await api.share.reviews();
  } catch {
    // the next sync event tries again
  }
}

export const logListeners = new Set<(instance: string, line: string) => void>();

/** A mod Modrinth found only by its name (look-alikes exist): installed only after a yes. */
export interface NameMatch {
  mod_id: string;
  needed_by: string;
  project_id: string;
  slug: string;
  title: string;
  author: string | null;
  downloads: number | null;
}
/** Questions from the Rust side, answered with `deps_confirm_answer`; the first is shown. */
export const depsQuestions = reactive<{ id: number; instance: string | null; name: string | null; items: NameMatch[] }[]>([]);

export function runOf(id: string): Run {
  return runs[id] ?? { state: "idle", stage: "", done: 0, total: 0 };
}

let started = false;
export async function startEvents() {
  if (started) return;
  started = true;
  for (const id of await api.instances.running()) runs[id] = { state: "running", stage: "", done: 0, total: 0 };
  // answers still owed from before the launcher was closed
  void refreshReviews();

  await listen<{ instance: string; stage: string; done: number; total: number }>("launch-progress", ({ payload }) => {
    runs[payload.instance] = { state: "preparing", stage: payload.stage, done: payload.done, total: payload.total };
  });
  await listen<{ instance: string; state: string; message: string | null }>("instance-state", ({ payload }) => {
    const prev = runOf(payload.instance);
    if (payload.state === "repaired") {
      delete runs[payload.instance];
      toast.success("Repaired: every file was checked, and broken ones downloaded again");
      return;
    }
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
  // a server on this computer whose setup the launcher was closed in the middle of
  await listen<{ name: string; state: "resuming" | "done" | "failed"; message: string | null }>("server-setup", ({ payload }) => {
    if (payload.state === "resuming") toast.success(`Continuing to set up ${payload.name}`);
    else if (payload.state === "done") toast.success(`${payload.name} is set up`);
    else toast.error(`Setting up ${payload.name} failed: ${payload.message ?? "unknown error"}`);
  });
  await listen<{ instance: string; server_id: number; state: "syncing" | "synced" | "failed"; message: string | null; needs_restart: boolean }>("server-sync", ({ payload }) => {
    const key = `${payload.instance}:${payload.server_id}`;
    // still waiting for a restart while the next change is sent
    const waiting = payload.needs_restart || (payload.state === "syncing" && !!serverSync[key]?.needsRestart);
    serverSync[key] = { state: payload.state, message: payload.message, needsRestart: waiting };
    if (payload.state === "synced") toast.success(payload.message ?? "Server updated with the new mods");
    if (payload.state === "failed") toast.error(`Couldn't update the server: ${payload.message ?? "unknown error"}`);
  });
  await listen<{ instance: string; name: string; state: SyncState | "review" | "servers"; by: string | null; message: string | null }>(
    "share-sync",
    ({ payload: p }) => {
      // servers someone linked to the pack, just put in this copy's server list
      if (p.state === "servers") {
        window.dispatchEvent(new CustomEvent("tessera-servers-changed", { detail: p.instance }));
        toast.success(`${p.name}: ${p.message} added to your servers`);
        return;
      }
      // jars waiting for an answer: the popup asks, the Share tab lists them
      if (p.state === "review" || p.state === "paused") {
        void refreshReviews();
        window.dispatchEvent(new CustomEvent("tessera-share-review", { detail: p.instance }));
      }
      if (p.state === "review") return;
      shareSync[p.instance] = { state: p.state, at: Date.now(), by: p.by, message: p.message };
      // tell the instance page to reload what it shows
      if (p.state === "pulled") window.dispatchEvent(new CustomEvent("tessera-instance-changed", { detail: p.instance }));
      // say each kind of thing once, not every few seconds
      const key = `${p.state}:${p.message ?? ""}`;
      if (p.state !== "pulled" && lastNotice.get(p.instance) === key) return;
      lastNotice.set(p.instance, key);
      if (p.state === "pulled") toast.success(`${p.name} updated${p.by ? `: ${p.by} ${p.message ?? "changed it"}` : p.message ? `: ${p.message}` : ""}`);
      else if (p.state === "waiting") toast.info(`${p.name} has new changes. They're applied when you close the game.`);
      else if (p.state === "error") toast.error(`Couldn't sync ${p.name}: ${p.message ?? "unknown error"}`);
    },
  );
  await listen<{ id: number; instance: string | null; name: string | null; items: NameMatch[] }>("deps-confirm", ({ payload }) => {
    depsQuestions.push(payload);
  });
  await listen<{ instance: string; line: string }>("instance-log", ({ payload }) => {
    for (const fn of logListeners) fn(payload.instance, payload.line);
  });
}
