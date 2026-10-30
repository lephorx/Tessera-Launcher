import { createHash, randomBytes, randomInt, timingSafeEqual } from "node:crypto";

/** 256-bit random token, URL-safe. Used for sessions, pairing secrets and machine credentials. */
export function randomToken(): string {
  return randomBytes(32).toString("base64url");
}

/** Tokens are high-entropy, so a plain SHA-256 is enough to store them. */
export function hashToken(token: string): string {
  return createHash("sha256").update(token).digest("hex");
}

export function tokenMatches(token: string, hash: string): boolean {
  const a = Buffer.from(hashToken(token), "hex");
  const b = Buffer.from(hash, "hex");
  return a.length === b.length && timingSafeEqual(a, b);
}

// No 0/O, 1/I/L, so codes read back unambiguously.
const CODE_ALPHABET = "ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/** Pairing code, stored without the dash: "KX742P", shown as "KX7-42P". */
export function pairingCode(): string {
  let code = "";
  for (let i = 0; i < 6; i++) code += CODE_ALPHABET[randomInt(CODE_ALPHABET.length)];
  return code;
}

export function formatPairingCode(code: string): string {
  return `${code.slice(0, 3)}-${code.slice(3)}`;
}

export function normalisePairingCode(input: string): string {
  return input.toUpperCase().replace(/[^A-Z0-9]/g, "");
}
