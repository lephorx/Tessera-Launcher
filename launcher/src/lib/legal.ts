import { openUrl } from "@tauri-apps/plugin-opener";

/**
 * Terms, privacy and the other legal pages, all on the website. Bump TERMS_VERSION together
 * with the API's CURRENT_TERMS_VERSION and the website's Terms; members are asked again.
 */
export const TERMS_VERSION = "2026-10-31";

export const LEGAL = {
  privacy: "https://tesseralauncher.com/privacy",
  terms: "https://tesseralauncher.com/terms",
  imprint: "https://tesseralauncher.com/imprint",
  licenses: "https://tesseralauncher.com/licenses",
  eula: "https://aka.ms/MinecraftEULA",
} as const;

/** "16+" or "13-15"; under 13 can't have a Tessera account. */
export type AgeBand = "16+" | "13-15" | "under13";

export interface Consent {
  terms_version: string;
  age: "16+" | "13-15";
  parental_consent: boolean;
}

/** Opens a legal page in the system browser. */
export function openLegal(page: keyof typeof LEGAL) {
  void openUrl(LEGAL[page]);
}

/** The consent to send, or null while the answers don't allow an account yet. */
export function consentOf(age: AgeBand | null, parental: boolean, agreed: boolean): Consent | null {
  if (!agreed || !age || age === "under13" || (age === "13-15" && !parental)) return null;
  return { terms_version: TERMS_VERSION, age, parental_consent: age === "13-15" && parental };
}
