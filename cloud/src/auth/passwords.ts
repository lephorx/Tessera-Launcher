import { hash, verify } from "@node-rs/argon2";
import { generateSecret, generateURI, verify as verifyTotp } from "otplib";

// argon2id with the OWASP-recommended minimum parameters.
const ARGON2 = { memoryCost: 19456, timeCost: 2, parallelism: 1 };

export function hashPassword(password: string): Promise<string> {
  return hash(password, ARGON2);
}

export async function verifyPassword(passwordHash: string, password: string): Promise<boolean> {
  try {
    return await verify(passwordHash, password);
  } catch {
    return false;
  }
}

/** A real hash to verify against when the user doesn't exist, so timing doesn't reveal it. */
let dummyHash: Promise<string> | undefined;
export function dummyPasswordHash(): Promise<string> {
  dummyHash ??= hashPassword("tessera-dummy-password");
  return dummyHash;
}

export function newTotpSecret(): string {
  return generateSecret();
}

export function totpUri(secret: string, email: string): string {
  return generateURI({ issuer: "Tessera", label: email, secret });
}

export async function checkTotp(secret: string, code: string): Promise<boolean> {
  const token = code.replace(/\s/g, "");
  if (!/^\d{6}$/.test(token)) return false;
  try {
    // Accept the previous and next 30 s step for clock drift.
    const result = await verifyTotp({ secret, token, epochTolerance: 30 });
    return result.valid;
  } catch {
    return false;
  }
}
