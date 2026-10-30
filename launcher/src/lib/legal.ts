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

/** The age band for a birthdate ("YYYY-MM-DD" from a date input), or null if it isn't a real past date. */
export function ageBandOf(birthdate: string): AgeBand | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(birthdate);
  if (!m) return null;
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const now = new Date();
  if (y < 1900 || new Date(y, mo - 1, d) > now) return null;
  const birthdayPassed = now.getMonth() + 1 > mo || (now.getMonth() + 1 === mo && now.getDate() >= d);
  const age = now.getFullYear() - y - (birthdayPassed ? 0 : 1);
  return age >= 16 ? "16+" : age >= 13 ? "13-15" : "under13";
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
