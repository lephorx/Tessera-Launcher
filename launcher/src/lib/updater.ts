import { reactive } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { invoke } from "@tauri-apps/api/core";
import { runs } from "./store";
import { errorText, toast } from "./toast";
import { APP_NAME } from "./channel";

/**
 * Launcher updates from GitHub releases (signed; the updater refuses anything else).
 *
 * On start a new version installs right away (no game can be running yet). While the launcher
 * is open it checks every 30 minutes and when the window comes back into focus, downloads a
 * new version in the background, and the top bar shows "Reload to update": installing then
 * restarts the launcher, which would close a running game, so that waits for a click.
 *
 * The Arch package updates through pacman instead (install_pacman_update): that asks for the
 * password, so it waits for the click on start too, and downloads only then.
 */
export const updater = reactive({
  current: "",
  available: null as null | { version: string; notes: string },
  /** The new version is downloaded; installing it is instant. */
  ready: false,
  checking: false,
  installing: false,
  /** 0–1 while downloading */
  progress: 0,
});

let pending: Update | null = null;
let downloading: Promise<void> | null = null;
let lastCheck = 0;
/** "pacman" for the Arch package, else "builtin" */
let kind: Promise<string> | null = null;
const updateKind = () => (kind ??= invoke<string>("update_kind").catch(() => "builtin"));

export async function checkForUpdate(manual = false) {
  if (updater.checking || updater.installing) return;
  updater.checking = true;
  lastCheck = Date.now();
  try {
    updater.current ||= await getVersion();
    const pacman = (await updateKind()) === "pacman";
    const update = await check(pacman ? { target: "linux-x86_64-pacman" } : undefined);
    if (update && update.version !== pending?.version) {
      pending = update;
      updater.ready = false;
      downloading = null;
    }
    if (!update) pending = null;
    updater.available = update ? { version: update.version, notes: update.body ?? "" } : null;
    if (manual && !update) toast.success(`${APP_NAME} is up to date (${updater.current})`);
  } catch (e) {
    if (manual) toast.error(`Couldn't check for updates: ${errorText(e)}`);
  } finally {
    updater.checking = false;
  }
}

/** Downloads the pending update once; progress shows while installing waits on it. */
function download(): Promise<void> {
  if (!pending) return Promise.resolve();
  const update = pending;
  downloading ??= (async () => {
    let total = 0;
    let done = 0;
    await update.download((e) => {
      if (e.event === "Started") total = e.data.contentLength ?? 0;
      else if (e.event === "Progress") {
        done += e.data.chunkLength;
        updater.progress = total ? done / total : 0;
      }
    });
    if (pending === update) updater.ready = true;
  })().catch((e) => {
    downloading = null;
    throw e;
  });
  return downloading;
}

/** Installs the update and restarts the launcher. */
export async function installUpdate() {
  if (!pending || updater.installing) return;
  updater.installing = true;
  try {
    if ((await updateKind()) === "pacman") {
      await invoke("install_pacman_update");
      await relaunch();
      return;
    }
    await download();
    await pending.install();
    await relaunch();
  } catch (e) {
    toast.error(`The update didn't install: ${errorText(e)}`);
    updater.installing = false;
  }
}

/** Checks, and gets a new version ready in the background. */
async function checkAndPrepare() {
  await checkForUpdate();
  if (pending && !updater.ready && (await updateKind()) !== "pacman") await download().catch(() => {});
}

let timer: ReturnType<typeof setInterval> | undefined;
export async function startUpdateChecks() {
  clearInterval(timer);
  timer = setInterval(() => void checkAndPrepare(), 30 * 60 * 1000);
  window.addEventListener("focus", () => {
    if (Date.now() - lastCheck > 10 * 60 * 1000) void checkAndPrepare();
  });

  await checkForUpdate();
  // on start nothing is running yet: update straight away
  if ((await updateKind()) === "pacman") return;
  if (pending && !Object.keys(runs).length) {
    toast.info(`Updating ${APP_NAME} to ${pending.version}…`);
    await installUpdate();
  } else if (pending) {
    void download().catch(() => {});
  }
}
