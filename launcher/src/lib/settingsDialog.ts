import { reactive } from "vue";
import { api, type Settings } from "./api";

export type SettingsSection = "appearance" | "profile" | "social" | "memory" | "resources" | "troubleshooting";

/** The Settings popup: whether it's open, and on which section. */
export const settingsDialog = reactive<{ open: boolean; section: SettingsSection }>({ open: false, section: "appearance" });

export function openSettings(section: SettingsSection = settingsDialog.section) {
  settingsDialog.section = section;
  settingsDialog.open = true;
}

/** "4.2 GiB" */
export function bytes(n: number) {
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(i && n < 100 ? 1 : 0)} ${units[i]}`;
}

/** Saves some settings on top of the latest ones on disk (each section saves on its own). */
export async function saveSettings(patch: Partial<Settings>) {
  return api.settings.save({ ...(await api.settings.get()), ...patch });
}
