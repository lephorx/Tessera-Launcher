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
export const serverSync = reactive<Record<string, { state: "syncing" | "synced" | "failed"; message: string | null }>>({});

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
  await listen<{ instance: string; server_id: number; state: "syncing" | "synced" | "failed"; message: string | null }>("server-sync", ({ payload }) => {
    serverSync[`${payload.instance}:${payload.server_id}`] = { state: payload.state, message: payload.message };
    if (payload.state === "synced") toast.success(payload.message ?? "Server updated with the new mods");
    if (payload.state === "failed") toast.error(`Couldn't update the server: ${payload.message ?? "unknown error"}`);
  });
  await listen<{ instance: string; line: string }>("instance-log", ({ payload }) => {
    for (const fn of logListeners) fn(payload.instance, payload.line);
  });
}
