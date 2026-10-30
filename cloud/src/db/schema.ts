import { sql } from "drizzle-orm";
import {
  boolean,
  index,
  jsonb,
  pgEnum,
  pgTable,
  primaryKey,
  text,
  timestamp,
  uuid,
} from "drizzle-orm/pg-core";

const createdAt = () => timestamp("created_at", { withTimezone: true }).notNull().defaultNow();

export const users = pgTable("users", {
  id: uuid("id").primaryKey().defaultRandom(),
  email: text("email").notNull().unique(), // stored lowercased
  username: text("username").notNull().unique(),
  passwordHash: text("password_hash").notNull(),
  totpSecret: text("totp_secret"),
  totpEnabled: boolean("totp_enabled").notNull().default(false),
  createdAt: createdAt(),
});

export const sessions = pgTable(
  "sessions",
  {
    id: uuid("id").primaryKey().defaultRandom(),
    userId: uuid("user_id")
      .notNull()
      .references(() => users.id, { onDelete: "cascade" }),
    tokenHash: text("token_hash").notNull().unique(),
    userAgent: text("user_agent"),
    createdAt: createdAt(),
    lastUsedAt: timestamp("last_used_at", { withTimezone: true }).notNull().defaultNow(),
    expiresAt: timestamp("expires_at", { withTimezone: true }).notNull(),
  },
  (t) => [index("sessions_user_idx").on(t.userId)],
);

export const machineKind = pgEnum("machine_kind", ["self_hosted", "hosted"]);
export const machineRole = pgEnum("machine_role", ["admin", "member"]);

export const machines = pgTable("machines", {
  id: uuid("id").primaryKey().defaultRandom(),
  ownerId: uuid("owner_id")
    .notNull()
    .references(() => users.id, { onDelete: "cascade" }),
  name: text("name").notNull(),
  kind: machineKind("kind").notNull().default("self_hosted"),
  // Set when the agent fetches its credential; null until then. Only the hash is stored.
  credentialHash: text("credential_hash").unique(),
  version: text("version"),
  capabilities: jsonb("capabilities").$type<string[]>().notNull().default(sql`'[]'::jsonb`),
  lastSeen: timestamp("last_seen", { withTimezone: true }),
  createdAt: createdAt(),
});

export const machineMembers = pgTable(
  "machine_members",
  {
    machineId: uuid("machine_id")
      .notNull()
      .references(() => machines.id, { onDelete: "cascade" }),
    userId: uuid("user_id")
      .notNull()
      .references(() => users.id, { onDelete: "cascade" }),
    role: machineRole("role").notNull(),
    createdAt: createdAt(),
  },
  (t) => [primaryKey({ columns: [t.machineId, t.userId] }), index("members_user_idx").on(t.userId)],
);

export const pairingCodes = pgTable("pairing_codes", {
  code: text("code").primaryKey(), // normalised, without the dash
  secretHash: text("secret_hash").notNull(),
  machineHint: text("machine_hint"),
  version: text("version"),
  expiresAt: timestamp("expires_at", { withTimezone: true }).notNull(),
  machineId: uuid("machine_id").references(() => machines.id, { onDelete: "cascade" }),
  exchangedAt: timestamp("exchanged_at", { withTimezone: true }),
  createdAt: createdAt(),
});

export type User = typeof users.$inferSelect;
export type Machine = typeof machines.$inferSelect;
export type MachineRole = (typeof machineRole.enumValues)[number];
