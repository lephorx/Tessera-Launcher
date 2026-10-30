import { reactive } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { errorText, toast } from "./toast";

/**
 * Launcher updates from GitHub releases (signed; the updater refuses anything else). Checked
 * on start and every few hours; the top bar shows "Reload to update" once one is ready.
 */
export const updater = reactive({
  current: "",
  available: null as null | { version: string; notes: string },
  checking: false,
  installing: false,
  /** 0–1 while downloading */
  progress: 0,
});

let pending: Update | null = null;

export async function checkForUpdate(manual = false) {
  if (updater.checking || updater.installing) return;
  updater.checking = true;
  try {
    updater.current ||= await getVersion();
    const update = await check();
    pending = update;
    updater.available = update ? { version: update.version, notes: update.body ?? "" } : null;
    if (manual && !update) toast.success(`Tessera Launcher is up to date (${updater.current})`);
  } catch (e) {
    if (manual) toast.error(`Couldn't check for updates: ${errorText(e)}`);
  } finally {
    updater.checking = false;
  }
}

/** Downloads the update, installs it and restarts the launcher. */
export async function installUpdate() {
  if (!pending || updater.installing) return;
  updater.installing = true;
  updater.progress = 0;
  let total = 0;
  let done = 0;
  try {
    await pending.downloadAndInstall((e) => {
      if (e.event === "Started") total = e.data.contentLength ?? 0;
      else if (e.event === "Progress") {
        done += e.data.chunkLength;
        updater.progress = total ? done / total : 0;
      }
    });
    await relaunch();
  } catch (e) {
    toast.error(`The update didn't install: ${errorText(e)}`);
    updater.installing = false;
  }
}

let timer: ReturnType<typeof setInterval> | undefined;
export function startUpdateChecks() {
  void checkForUpdate();
  clearInterval(timer);
  timer = setInterval(() => void checkForUpdate(), 6 * 60 * 60 * 1000);
}
