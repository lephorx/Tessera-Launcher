/**
 * Tessera Launcher Nightly is the experimental build from main, installed next to the released
 * launcher with its own name, data, update feed and a midnight look (tokens.css). Set at
 * build time by TESSERA_NIGHTLY (vite.config.ts).
 */
export const NIGHTLY = __NIGHTLY__;
export const APP_NAME = NIGHTLY ? "Tessera Launcher Nightly" : "Tessera Launcher";
