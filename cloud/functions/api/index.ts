/**
 * Tessera API, a Neon Function next to the "tessera" database.
 *
 * Callers sign in with Neon Auth and send `Authorization: Bearer <JWT>` (15-minute token from
 * Neon Auth's /token). The JWT is checked against Neon Auth's JWKS before any work, and every
 * query is scoped to the caller. DATABASE_URL and the bucket credentials only exist here.
 *
 *   GET    /v1/me                               who am I (+ username, accepted Terms version)
 *   POST   /v1/me/consent  {terms_version, age, parental_consent?}   accept the current Terms
 *   GET    /v1/me/export                        everything stored about me, as a JSON download
 *   DELETE /v1/me                               delete my account, my shares and their packs
 *   PUT    /v1/me/username        {username}    pick the name friends add you by
 *   POST   /v1/shares             .tapack body  share an instance (?name=&game_version=&loader=)
 *   PUT    /v1/shares/:code       .tapack body  upload a newer version (owner, or an editor: same
 *                                               Minecraft and loader versions only)
 *   GET    /v1/shares/mine                      what I share, with friends
 *   GET    /v1/shares/received                  what friends share with me
 *   PATCH  /v1/shares/:code       {expires_in_days: 1|7|30|null}
 *   DELETE /v1/shares/:code
 *   POST   /v1/shares/:code/members   {username, role?}   friends only, at most 10 per pack
 *   PATCH  /v1/shares/:code/members/:userId   {role: viewer|editor}
 *   DELETE /v1/shares/:code/members/:userId   owner removes a friend, or a friend leaves
 *   POST   /v1/shares/:code/transfer  {user_id}  hand the share to a friend; you stay as editor
 *   GET    /v1/shares/:code                     public: pack info for an invite link
 *   GET    /v1/shares/:code/download            owner or a friend on it: short-lived download URL
 *   GET    /v1/shares/sync?codes=a,b            live sync: revision of each share I'm on
 *   GET    /v1/friends                          friends + incoming and outgoing requests
 *   POST   /v1/friends           {username}    send a request (accepts theirs if they asked first)
 *   POST   /v1/friends/:userId/accept
 *   DELETE /v1/friends/:userId                  unfriend (also off each other's packs), decline or cancel
 *   GET    /v1/notifications                    friend requests, accepted requests, packs shared with me
 *   POST   /v1/notifications/seen               the bell's unread count starts again from now
 *   GET    /v1/me/privacy                       who may send me friend requests / add me to shares
 *   PUT    /v1/me/privacy        {friend_requests?, share_invites?}
 *   GET    /v1/blocks                           people I blocked
 *   POST   /v1/blocks            {username}    block: also unfriends and takes them off my shares
 *   DELETE /v1/blocks/:userId
 *   GET    /v1/news                             public: launcher news and releases, newest first
 *
 * Closed beta: only accounts with a profile can use the API, and a profile is only made by
 * redeeming an invite (each works once):
 *   GET    /v1/invites/:code                    public: is this invite still good?
 *   POST   /v1/invites/:code/redeem  {username, terms_version?, age?, parental_consent?}
 *                                               join with an invite (right after sign-up)
 *   GET    /v1/invites                          mine, and how many I may still make
 *   POST   /v1/invites                          make one (3 for members, any number for admins)
 *   DELETE /v1/invites/:code                    revoke an unused one
 */
import { randomInt } from "node:crypto";
import { attachDatabasePool } from "@neon/functions";
import { unzipSync, strFromU8 } from "fflate";
import { Files } from "files-sdk";
import { neon } from "files-sdk/neon";
import { Hono, type Context } from "hono";
import { createRemoteJWKSet, jwtVerify } from "jose";
import pg from "pg";
import { z } from "zod";

const pool = new pg.Pool({ connectionString: process.env.DATABASE_URL, max: 5 });
attachDatabasePool(pool);
const files = new Files({ adapter: neon({ bucket: "packs" }) });
const jwks = createRemoteJWKSet(new URL(process.env.NEON_AUTH_JWKS_URL!));
const issuer = new URL(process.env.NEON_AUTH_BASE_URL!).origin;

const MAX_PACK_BYTES = 50 * 1024 * 1024;
const MAX_SHARES_PER_USER = 20;
/** Friends per shared pack: each one's launcher checks the pack for changes while it's open. */
const MAX_SHARE_MEMBERS = 10;

// ── rate limits (per isolate, in memory) ─────────────────────────────────

class RateLimiter {
  private hits = new Map<string, { count: number; resetAt: number }>();
  constructor(
    private limit: number,
    private windowMs: number,
  ) {}
  take(key: string, now = Date.now()): boolean {
    const e = this.hits.get(key);
    if (!e || e.resetAt <= now) {
      if (this.hits.size > 10_000) for (const [k, v] of this.hits) if (v.resetAt <= now) this.hits.delete(k);
      this.hits.set(key, { count: 1, resetAt: now + this.windowMs });
      return true;
    }
    return ++e.count <= this.limit;
  }
}
const perUser = new RateLimiter(120, 60_000);
// live sync uploads after every change, so this is generous; packs link mods instead of carrying them
const uploads = new RateLimiter(240, 60 * 60_000);
const publicLookups = new RateLimiter(60, 60_000);

// ── helpers ──────────────────────────────────────────────────────────────

type Env = { Variables: { userId: string; isAdmin: boolean } };

function err(c: Context, status: 400 | 401 | 403 | 404 | 409 | 410 | 413 | 429, error: string, message?: string) {
  return c.json({ error, ...(message ? { message } : {}) }, status, status === 429 ? { "retry-after": "60" } : undefined);
}

function clientIp(c: Context): string {
  return c.req.header("x-forwarded-for")?.split(",")[0]?.trim() || "unknown";
}

// No 0/O, 1/I/L: easy to read out loud.
const ALPHABET = "abcdefghjkmnpqrstuvwxyzABCDEFGHJKMNPQRSTUVWXYZ23456789";
function newCode(): string {
  let s = "";
  for (let i = 0; i < 8; i++) s += ALPHABET[randomInt(ALPHABET.length)];
  return s;
}

const packMeta = z.object({
  name: z.string().trim().min(1).max(80),
  game_version: z.string().trim().min(1).max(40),
  loader: z.enum(["vanilla", "fabric", "quilt", "forge", "neoforge"]),
});

/** Reads a .tapack request body, refusing anything too big or not a zip. */
async function readPack(c: Context): Promise<Uint8Array | Response> {
  const declared = Number(c.req.header("content-length") ?? 0);
  if (declared > MAX_PACK_BYTES) return err(c, 413, "too_large", "Packs can be up to 50 MB. Leave worlds out of the pack.");
  const body = new Uint8Array(await c.req.arrayBuffer());
  if (body.length > MAX_PACK_BYTES) return err(c, 413, "too_large", "Packs can be up to 50 MB. Leave worlds out of the pack.");
  if (body.length < 22 || body[0] !== 0x50 || body[1] !== 0x4b) return err(c, 400, "not_a_pack", "That isn't a .tapack file.");
  return body;
}

/** Minecraft and loader versions from the pack's tessera.pack.json: what the share really is. */
function packVersions(body: Uint8Array): { game_version: string; loader: string; loader_version: string | null } | null {
  try {
    const files = unzipSync(body, { filter: (f) => f.name === "tessera.pack.json" });
    const raw = files["tessera.pack.json"];
    if (!raw) return null;
    const m = JSON.parse(strFromU8(raw));
    const loader = z.enum(["vanilla", "fabric", "quilt", "forge", "neoforge"]).safeParse(m?.loader?.type);
    if (typeof m?.minecraft !== "string" || !loader.success) return null;
    return { game_version: m.minecraft, loader: loader.data, loader_version: typeof m.loader.version === "string" ? m.loader.version : null };
  } catch {
    return null;
  }
}

// ── Terms and age ────────────────────────────────────────────────────────

/** Bump together with the website's Terms and Privacy Policy; launchers then ask again. */
const CURRENT_TERMS_VERSION = "2026-10-31";

/**
 * Consent sent at sign-up or later: the current Terms version and an age band. We keep the
 * band, never a birthdate; 13–15 needs a parent's or guardian's agreement, under 13 can't
 * have an account (the launcher stops them before they get here).
 */
const consentBody = z.object({
  terms_version: z.string().max(40),
  age: z.enum(["16+", "13-15"]),
  parental_consent: z.boolean().optional(),
});

function checkConsent(c: Context, raw: unknown): { terms_version: string; age: "16+" | "13-15" } | Response {
  const body = consentBody.safeParse(raw);
  if (!body.success) return err(c, 400, "invalid_consent", "Choose your age and accept the Terms and Privacy Policy.");
  if (body.data.terms_version !== CURRENT_TERMS_VERSION) return err(c, 400, "outdated_terms", "Those Terms are out of date. Update the launcher and try again.");
  if (body.data.age === "13-15" && body.data.parental_consent !== true) {
    return err(c, 400, "parental_consent_required", "Between 13 and 15, a parent or guardian has to agree to your account.");
  }
  return { terms_version: body.data.terms_version, age: body.data.age };
}

const SHARE_COLUMNS = `s.code, s.name, s.game_version, s.loader, s.size_bytes, s.created_at, s.updated_at, s.expires_at, s.revision, p.username as owner`;

// ── app ──────────────────────────────────────────────────────────────────

const app = new Hono<Env>().basePath("/v1");

app.use("*", async (c, next) => {
  await next();
  c.header("cache-control", "no-store");
});

// Public: invite-link lookups, limited per address.
app.get("/shares/:code", async (c, next) => {
  if (c.req.header("authorization")) return next();
  if (!publicLookups.take(clientIp(c))) return err(c, 429, "rate_limited");
  const { rows } = await pool.query(
    `select ${SHARE_COLUMNS} from shares s left join profiles p on p.user_id = s.owner_id where s.code = $1`,
    [c.req.param("code")],
  );
  const share = rows[0];
  if (!share) return err(c, 404, "not_found", "That shared pack doesn't exist.");
  // the link only shows what it is: the pack itself is for the friends the owner added
  return c.json({ share });
});

// Public: launcher news, with each launcher release as a post of its own.
const RELEASES_URL = "https://api.github.com/repos/lephorx/Tessera-Launcher/releases?per_page=5";
let releasesCache: { at: number; items: unknown[] } = { at: 0, items: [] };

/** The first paragraph of a release's notes, without Markdown marks. */
function summary(markdown: string): string {
  const para = markdown.split(/\r?\n\s*\r?\n/).map((p) => p.trim()).find((p) => p && !p.startsWith("#")) ?? "";
  return para.replace(/[#*_`>]/g, "").replace(/\[([^\]]+)\]\([^)]+\)/g, "$1").replace(/^\s*-\s+/gm, "• ").slice(0, 280);
}

async function releases(): Promise<unknown[]> {
  if (Date.now() - releasesCache.at < 10 * 60_000) return releasesCache.items;
  try {
    const res = await fetch(RELEASES_URL, { headers: { accept: "application/vnd.github+json", "user-agent": "tessera-api" }, signal: AbortSignal.timeout(5000) });
    if (res.ok) {
      const list = (await res.json()) as { id: number; name: string | null; tag_name: string; body: string | null; html_url: string; published_at: string; draft: boolean; prerelease: boolean }[];
      releasesCache = {
        at: Date.now(),
        items: list
          .filter((r) => !r.draft)
          .map((r) => ({
            id: 1_000_000_000 + (r.id % 1_000_000_000),
            title: `Tessera Launcher ${r.tag_name.replace(/^v/, "")}${r.prerelease ? " (beta)" : ""}`,
            body: summary(r.body ?? "") || "A new version of the launcher is out.",
            url: r.html_url,
            image_url: null,
            published_at: r.published_at,
            release: r.tag_name,
          })),
      };
    } else {
      releasesCache = { ...releasesCache, at: Date.now() };
    }
  } catch {
    releasesCache = { ...releasesCache, at: Date.now() };
  }
  return releasesCache.items;
}

app.get("/news", async (c) => {
  if (!publicLookups.take(clientIp(c))) return err(c, 429, "rate_limited");
  const [{ rows }, rel] = await Promise.all([
    pool.query(`select id, title, body, url, image_url, published_at from news order by published_at desc limit 10`),
    releases(),
  ]);
  const all = [...rows, ...rel] as { published_at: string }[];
  all.sort((a, b) => new Date(b.published_at).getTime() - new Date(a.published_at).getTime());
  return c.json({ news: all.slice(0, 10) });
});

app.get("/invites/:code", async (c, next) => {
  if (c.req.header("authorization")) return next();
  if (!publicLookups.take(clientIp(c))) return err(c, 429, "rate_limited");
  const { rows } = await pool.query(
    `select i.used_by is null and i.revoked_at is null as valid, p.username as invited_by
     from invites i left join profiles p on p.user_id = i.created_by where i.code = $1`,
    [c.req.param("code")],
  );
  if (!rows[0]) return c.json({ valid: false, reason: "That invite doesn't exist." });
  if (!rows[0].valid) return c.json({ valid: false, reason: "That invite was already used. Ask for a new one." });
  return c.json({ valid: true, invited_by: rows[0].invited_by });
});

// Everything else needs a signed-in user.
app.use("*", async (c, next) => {
  const auth = c.req.header("authorization");
  if (!auth?.toLowerCase().startsWith("bearer ")) return err(c, 401, "unauthenticated");
  try {
    const { payload } = await jwtVerify(auth.slice(7), jwks, { issuer });
    if (!payload.sub) return err(c, 401, "unauthenticated");
    c.set("userId", payload.sub);
  } catch {
    return err(c, 401, "unauthenticated");
  }
  if (!perUser.take(c.var.userId)) return err(c, 429, "rate_limited", "Too many requests. Slow down a little.");
  // closed beta: everything but "who am I" and joining needs a redeemed invite
  const path = new URL(c.req.url).pathname.replace(/^\/v1/, "");
  // also open: your own data (export, delete) and accepting the Terms, member or not
  const open =
    (c.req.method === "GET" && (path === "/me" || path === "/me/export")) ||
    (c.req.method === "DELETE" && path === "/me") ||
    (c.req.method === "POST" && (path === "/me/consent" || /^\/invites\/[^/]+\/redeem$/.test(path)));
  if (!open) {
    const { rows } = await pool.query(`select is_admin from profiles where user_id = $1`, [c.var.userId]);
    if (!rows[0]) return err(c, 403, "invite_required", "Tessera is invite-only for now. Join with an invite link from a friend.");
    c.set("isAdmin", rows[0].is_admin);
  }
  await next();
});

app.get("/me", async (c) => {
  const { rows } = await pool.query(
    `select u.id, u.email, u.name, p.username, coalesce(p.is_admin, false) as is_admin, p.user_id is not null as member,
       p.terms_version, p.age_declaration
     from neon_auth."user" u left join profiles p on p.user_id = u.id where u.id = $1`,
    [c.var.userId],
  );
  return rows[0] ? c.json({ user: rows[0] }) : err(c, 401, "unauthenticated");
});

// Existing members accept new Terms (and say how old they are) here.
app.post("/me/consent", async (c) => {
  const consent = checkConsent(c, await c.req.json().catch(() => null));
  if (consent instanceof Response) return consent;
  const { rows } = await pool.query(
    `update profiles set terms_version = $2, terms_accepted_at = now(), age_declaration = $3 where user_id = $1
     returning terms_version, terms_accepted_at, age_declaration`,
    [c.var.userId, consent.terms_version, consent.age],
  );
  return rows[0] ? c.json({ consent: rows[0] }) : err(c, 403, "invite_required", "Join with an invite first.");
});

// Everything stored about the caller (GDPR art. 15 and 20): one JSON file.
app.get("/me/export", async (c) => {
  const id = c.var.userId;
  const q = async (sql: string) => (await pool.query(sql, [id])).rows;
  const [account, profile, sharesOwned, memberships, friendships, invitesCreated, inviteUsed, blocks] = await Promise.all([
    // Better Auth's columns are camelCase; read created_at either way
    q(`select u.email, u.name, coalesce(to_jsonb(u)->>'createdAt', to_jsonb(u)->>'created_at') as created_at
       from neon_auth."user" u where u.id = $1`),
    q(`select username, created_at, is_admin, friend_requests, share_invites, terms_version, terms_accepted_at, age_declaration
       from profiles where user_id = $1`),
    q(`select s.code, s.name, s.game_version, s.loader, s.loader_version, s.size_bytes, s.created_at, s.updated_at, s.expires_at, s.revision,
         coalesce((select json_agg(json_build_object('username', mp.username, 'role', m.role, 'added_at', m.added_at) order by mp.username)
                   from share_members m join profiles mp on mp.user_id = m.user_id where m.code = s.code), '[]') as members
       from shares s where s.owner_id = $1 order by s.created_at`),
    q(`select s.code, s.name, p.username as owner, m.role, m.added_at
       from share_members m join shares s on s.code = m.code left join profiles p on p.user_id = s.owner_id
       where m.user_id = $1 order by m.added_at`),
    q(`select p.username, f.status, case when f.requester_id = $1 then 'sent' else 'received' end as direction, f.created_at
       from friendships f left join profiles p on p.user_id = case when f.requester_id = $1 then f.addressee_id else f.requester_id end
       where f.requester_id = $1 or f.addressee_id = $1 order by f.created_at`),
    q(`select i.code, i.created_at, i.used_at, i.revoked_at, p.username as used_by
       from invites i left join profiles p on p.user_id = i.used_by where i.created_by = $1 order by i.created_at`),
    q(`select i.code, i.used_at, p.username as invited_by
       from invites i left join profiles p on p.user_id = i.created_by where i.used_by = $1`),
    q(`select p.username, b.created_at from blocks b left join profiles p on p.user_id = b.blocked_id where b.blocker_id = $1 order by b.created_at`),
  ]);
  return c.json(
    {
      account: account[0] ?? null,
      profile: profile[0] ?? null,
      shares_owned: sharesOwned,
      share_memberships: memberships,
      friendships,
      invites_created: invitesCreated,
      invite_used: inviteUsed[0] ?? null,
      blocks,
      exported_at: new Date().toISOString(),
    },
    200,
    { "content-disposition": 'attachment; filename="tessera-data.json"' },
  );
});

// Deletes the account (GDPR art. 17): the packs of every share I own from the bucket, then the
// Neon Auth user; profiles, shares, memberships, friendships, invites I made and blocks go with
// it by foreign-key cascade (invites I used and shares I last updated keep a null in my place).
app.delete("/me", async (c) => {
  const id = c.var.userId;
  const { rows: mine } = await pool.query(`select object_key from shares where owner_id = $1`, [id]);
  await Promise.all(mine.map((s) => files.delete(s.object_key).catch((e) => console.error("delete pack", s.object_key, e))));
  const db = await pool.connect();
  try {
    await db.query("begin");
    await db.query("savepoint auth_user");
    try {
      await db.query(`delete from neon_auth."user" where id = $1`, [id]);
    } catch (e) {
      // no rights on neon_auth: delete everything of ours; the bare sign-in stays at Neon Auth
      if ((e as { code?: string }).code !== "42501") throw e;
      console.error("delete /me: no permission on neon_auth.user, deleting app data only");
      await db.query("rollback to savepoint auth_user");
      await db.query(`delete from shares where owner_id = $1`, [id]);
      await db.query(`delete from share_members where user_id = $1`, [id]);
      await db.query(`delete from friendships where requester_id = $1 or addressee_id = $1`, [id]);
      await db.query(`delete from blocks where blocker_id = $1 or blocked_id = $1`, [id]);
      await db.query(`delete from invites where created_by = $1`, [id]);
      await db.query(`update invites set used_by = null where used_by = $1`, [id]);
      await db.query(`update shares set updated_by = null where updated_by = $1`, [id]);
      await db.query(`delete from profiles where user_id = $1`, [id]);
    }
    await db.query("commit");
  } catch (e) {
    await db.query("rollback").catch(() => {});
    throw e;
  } finally {
    db.release();
  }
  return c.body(null, 204);
});

app.put("/me/username", async (c) => {
  const body = z.object({ username: z.string().regex(/^[A-Za-z0-9_]{3,24}$/) }).safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "invalid_username", "3–24 letters, digits or underscores.");
  try {
    await pool.query(`update profiles set username = $2 where user_id = $1`, [c.var.userId, body.data.username]);
  } catch (e) {
    if ((e as { code?: string }).code === "23505") return err(c, 409, "username_taken", "That username is taken.");
    throw e;
  }
  return c.json({ username: body.data.username });
});

app.post("/shares", async (c) => {
  if (!uploads.take(c.var.userId)) return err(c, 429, "rate_limited", "That's a lot of uploads. Try again in an hour.");
  const meta = packMeta.safeParse(c.req.query());
  if (!meta.success) return err(c, 400, "invalid_pack", "Missing pack name, game version or loader.");
  const { rows: count } = await pool.query(`select count(*)::int as n from shares where owner_id = $1`, [c.var.userId]);
  if (count[0].n >= MAX_SHARES_PER_USER) return err(c, 409, "too_many_shares", "You share 20 packs already. Stop sharing one first.");
  const body = await readPack(c);
  if (body instanceof Response) return body;
  const versions = packVersions(body);
  if (!versions) return err(c, 400, "not_a_pack", "That isn't a .tapack file.");

  const code = newCode();
  const key = `shares/${code}.tapack`;
  await files.upload(key, body, { contentType: "application/zip" });
  const { rows } = await pool.query(
    `insert into shares (code, owner_id, name, game_version, loader, loader_version, size_bytes, object_key, expires_at)
     values ($1, $2, $3, $4, $5, $6, $7, $8, now() + interval '7 days') returning code, expires_at, revision`,
    [code, c.var.userId, meta.data.name, versions.game_version, versions.loader, versions.loader_version, body.length, key],
  );
  return c.json({ share: rows[0] }, 201);
});

/** The share if the caller owns it. */
async function owned(c: Context<Env>) {
  const { rows } = await pool.query(`select * from shares where code = $1 and owner_id = $2`, [c.req.param("code"), c.var.userId]);
  return rows[0] ?? null;
}

/** The share and the caller's role in it: owner, editor, viewer, or null. */
async function access(c: Context<Env>) {
  const { rows } = await pool.query(
    `select s.*, case when s.owner_id = $2 then 'owner' else m.role end as role
     from shares s left join share_members m on m.code = s.code and m.user_id = $2 where s.code = $1`,
    [c.req.param("code"), c.var.userId],
  );
  return rows[0] ?? null;
}

app.put("/shares/:code", async (c) => {
  if (!uploads.take(c.var.userId)) return err(c, 429, "rate_limited", "That's a lot of uploads. Try again in an hour.");
  const share = await access(c);
  if (!share || !share.role) return err(c, 404, "not_found");
  if (share.role === "viewer") return err(c, 403, "read_only", "You can install this pack but not change it. Ask the owner to make you an editor.");
  const meta = packMeta.safeParse(c.req.query());
  if (!meta.success) return err(c, 400, "invalid_pack");
  const body = await readPack(c);
  if (body instanceof Response) return body;
  const versions = packVersions(body);
  if (!versions) return err(c, 400, "not_a_pack", "That isn't a .tapack file.");
  if (
    share.role === "editor" &&
    (versions.game_version !== share.game_version || versions.loader !== share.loader || (share.loader_version && versions.loader_version !== share.loader_version))
  ) {
    return err(c, 403, "versions_locked", "Editors can add and remove content, but only the owner can change the Minecraft or loader version.");
  }
  // made from an older copy: the caller has to take the newer one first (and merge)
  const base = c.req.query("base_revision");
  if (base !== undefined && Number(base) !== share.revision) {
    return c.json({ error: "outdated", message: "Someone changed the pack meanwhile.", revision: share.revision }, 409);
  }
  await files.upload(share.object_key, body, { contentType: "application/zip" });
  // only the owner renames the pack
  const name = share.role === "owner" ? meta.data.name : share.name;
  const { rows } = await pool.query(
    `update shares set name = $2, game_version = $3, loader = $4, loader_version = $5, size_bytes = $6, updated_at = now(),
       revision = revision + 1, updated_by = $8
     where code = $1 and ($7::int is null or revision = $7::int) returning revision`,
    [share.code, name, versions.game_version, versions.loader, versions.loader_version, body.length, base === undefined ? null : Number(base), c.var.userId],
  );
  if (!rows[0]) return c.json({ error: "outdated", message: "Someone changed the pack meanwhile." }, 409);
  return c.json({ ok: true, revision: rows[0].revision });
});

app.get("/shares/sync", async (c) => {
  const codes = (c.req.query("codes") ?? "").split(",").map((x) => x.trim()).filter((x) => /^[A-Za-z0-9]{8}$/.test(x)).slice(0, 50);
  if (!codes.length) return c.json({ shares: [] });
  const { rows } = await pool.query(
    `select s.code, s.revision, s.updated_at, (s.updated_by = $2) as mine, up.username as updated_by,
       case when s.owner_id = $2 then 'owner' else m.role end as role
     from shares s
     left join share_members m on m.code = s.code and m.user_id = $2
     left join profiles up on up.user_id = s.updated_by
     where s.code = any($1) and (s.owner_id = $2 or m.user_id is not null)`,
    [codes, c.var.userId],
  );
  return c.json({ shares: rows });
});

app.get("/shares/mine", async (c) => {
  const { rows } = await pool.query(
    `select ${SHARE_COLUMNS},
       coalesce((select json_agg(json_build_object('user_id', m.user_id, 'username', mp.username, 'role', m.role) order by mp.username)
                 from share_members m join profiles mp on mp.user_id = m.user_id where m.code = s.code), '[]') as members
     from shares s left join profiles p on p.user_id = s.owner_id
     where s.owner_id = $1 order by s.updated_at desc`,
    [c.var.userId],
  );
  return c.json({ shares: rows });
});

app.get("/shares/received", async (c) => {
  const { rows } = await pool.query(
    `select ${SHARE_COLUMNS}, m.role from share_members m join shares s on s.code = m.code
     left join profiles p on p.user_id = s.owner_id where m.user_id = $1 order by s.updated_at desc`,
    [c.var.userId],
  );
  return c.json({ shares: rows });
});

app.get("/shares/:code", async (c) => {
  const { rows } = await pool.query(
    `select ${SHARE_COLUMNS}, s.loader_version,
       case when s.owner_id = $2 then 'owner' else (select role from share_members where code = s.code and user_id = $2) end as role
     from shares s left join profiles p on p.user_id = s.owner_id where s.code = $1`,
    [c.req.param("code"), c.var.userId],
  );
  return rows[0] ? c.json({ share: rows[0] }) : err(c, 404, "not_found");
});

app.patch("/shares/:code", async (c) => {
  const body = z.object({ expires_in_days: z.union([z.literal(1), z.literal(7), z.literal(30), z.null()]) }).safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "invalid_expiry", "Choose 1, 7 or 30 days, or never.");
  const days = body.data.expires_in_days;
  const { rows } = await pool.query(
    `update shares set expires_at = case when $3::int is null then null else now() + make_interval(days => $3::int) end
     where code = $1 and owner_id = $2 returning code, expires_at`,
    [c.req.param("code"), c.var.userId, days],
  );
  return rows[0] ? c.json({ share: rows[0] }) : err(c, 404, "not_found");
});

app.delete("/shares/:code", async (c) => {
  const share = await owned(c);
  if (!share) return err(c, 404, "not_found");
  await files.delete(share.object_key).catch(() => {});
  await pool.query(`delete from shares where code = $1`, [share.code]);
  return c.json({ ok: true });
});

app.post("/shares/:code/members", async (c) => {
  const share = await owned(c);
  if (!share) return err(c, 404, "not_found");
  const body = z.object({ username: z.string().max(24), role: z.enum(["viewer", "editor"]).default("viewer") }).safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "invalid_username");
  const { rows } = await pool.query(`select user_id, username from profiles where username = $1`, [body.data.username.trim()]);
  const friend = rows[0];
  if (!friend) return err(c, 404, "user_not_found", "No Tessera account with that username.");
  if (friend.user_id === c.var.userId) return err(c, 400, "self", "That's you.");
  const { rowCount: friends } = await pool.query(
    `select 1 from friendships where status = 'accepted'
       and ((requester_id = $1 and addressee_id = $2) or (requester_id = $2 and addressee_id = $1))`,
    [c.var.userId, friend.user_id],
  );
  if (!friends) return err(c, 403, "not_friends", `You can only share with friends. Add ${friend.username} as a friend first.`);
  const { rows: count } = await pool.query(`select count(*)::int as n from share_members where code = $1 and user_id <> $2`, [share.code, friend.user_id]);
  if (count[0].n >= MAX_SHARE_MEMBERS) return err(c, 409, "too_many_members", `A pack can be shared with ${MAX_SHARE_MEMBERS} friends.`);
  if (!(await mayInviteToShare(c.var.userId, friend.user_id))) {
    return err(c, 403, "not_accepting", `${friend.username} doesn't accept shared packs from you.`);
  }
  await pool.query(`insert into share_members (code, user_id, role) values ($1, $2, $3) on conflict do nothing`, [share.code, friend.user_id, body.data.role]);
  return c.json({ member: { ...friend, role: body.data.role } }, 201);
});

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

app.delete("/shares/:code/members/:userId", async (c) => {
  const userId = c.req.param("userId");
  if (!UUID_RE.test(userId)) return err(c, 404, "not_found");
  // a friend may leave on their own
  const share = userId === c.var.userId ? await access(c) : await owned(c);
  if (!share) return err(c, 404, "not_found");
  await pool.query(`delete from share_members where code = $1 and user_id = $2`, [share.code, userId]);
  return c.json({ ok: true });
});

app.patch("/shares/:code/members/:userId", async (c) => {
  const share = await owned(c);
  if (!share) return err(c, 404, "not_found");
  const userId = c.req.param("userId");
  if (!UUID_RE.test(userId)) return err(c, 404, "not_found");
  const body = z.object({ role: z.enum(["viewer", "editor"]) }).safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "invalid_role", "The role is viewer or editor.");
  const { rowCount } = await pool.query(`update share_members set role = $3 where code = $1 and user_id = $2`, [share.code, userId, body.data.role]);
  return rowCount ? c.json({ ok: true, role: body.data.role }) : err(c, 404, "not_found", "That friend isn't on this share.");
});

// The new owner must already be on the share; the old owner stays on it as an editor.
app.post("/shares/:code/transfer", async (c) => {
  const share = await owned(c);
  if (!share) return err(c, 404, "not_found");
  const body = z.object({ user_id: z.string().regex(UUID_RE) }).safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "invalid_user");
  const db = await pool.connect();
  try {
    await db.query("begin");
    const { rowCount } = await db.query(`delete from share_members where code = $1 and user_id = $2`, [share.code, body.data.user_id]);
    if (!rowCount) {
      await db.query("rollback");
      return err(c, 404, "not_member", "Add them to the share first, then make them the owner.");
    }
    const { rows: count } = await db.query(`select count(*)::int as n from shares where owner_id = $1`, [body.data.user_id]);
    if (count[0].n >= MAX_SHARES_PER_USER) {
      await db.query("rollback");
      return err(c, 409, "too_many_shares", "They already own 20 shares.");
    }
    await db.query(`update shares set owner_id = $2, updated_at = updated_at where code = $1`, [share.code, body.data.user_id]);
    await db.query(`insert into share_members (code, user_id, role) values ($1, $2, 'editor') on conflict (code, user_id) do update set role = 'editor'`, [share.code, c.var.userId]);
    await db.query("commit");
  } catch (e) {
    await db.query("rollback").catch(() => {});
    throw e;
  } finally {
    db.release();
  }
  return c.json({ ok: true });
});

app.get("/shares/:code/download", async (c) => {
  const { rows } = await pool.query(
    `select s.*, (s.owner_id = $2 or exists (select 1 from share_members m where m.code = s.code and m.user_id = $2)) as invited
     from shares s where s.code = $1`,
    [c.req.param("code"), c.var.userId],
  );
  const share = rows[0];
  if (!share) return err(c, 404, "not_found", "That shared pack doesn't exist.");
  // shared packs are for the owner's friends they added: a link alone doesn't open it
  if (!share.invited) return err(c, 403, "friends_only", "This pack is shared with friends only. Ask its owner to add you.");
  const url = await files.url(share.object_key, { expiresIn: 600 });
  return c.json({ url, size_bytes: share.size_bytes });
});

// ── invites ──────────────────────────────────────────────────────────────

const MEMBER_INVITES = 3;
const INVITE_LINK = "https://tesseralauncher.com/invite/";

function newInviteCode(): string {
  let s = "";
  for (let i = 0; i < 10; i++) s += ALPHABET[randomInt(ALPHABET.length)];
  return s;
}

app.post("/invites/:code/redeem", async (c) => {
  const raw = await c.req.json().catch(() => null);
  const body = z.object({ username: z.string().regex(/^[A-Za-z0-9_]{3,24}$/) }).safeParse(raw);
  if (!body.success) return err(c, 400, "invalid_username", "Usernames are 3–24 letters, digits or underscores.");
  // launchers from before the Terms send no consent; they're asked via /me/consent afterwards
  const sent = raw && typeof raw === "object" && ["terms_version", "age", "parental_consent"].some((k) => k in raw);
  const consent = sent ? checkConsent(c, raw) : null;
  if (consent instanceof Response) return consent;
  const db = await pool.connect();
  try {
    await db.query("begin");
    const { rows: me } = await db.query(`select 1 from profiles where user_id = $1`, [c.var.userId]);
    if (me[0]) {
      await db.query("rollback");
      return err(c, 409, "already_member", "You're already in.");
    }
    const { rows } = await db.query(
      `select code from invites where code = $1 and used_by is null and revoked_at is null for update`,
      [c.req.param("code")],
    );
    if (!rows[0]) {
      await db.query("rollback");
      return err(c, 410, "invite_used", "That invite was already used or doesn't exist. Ask for a new one.");
    }
    try {
      await db.query(
        // friend requests from anyone, except for members under 16: friends of friends
        `insert into profiles (user_id, username, terms_version, terms_accepted_at, age_declaration, friend_requests)
         values ($1, $2, $3, case when $3::text is null then null else now() end, $4,
                 case when $4::text = '13-15' then 'friends_of_friends' else 'everyone' end)`,
        [c.var.userId, body.data.username, consent?.terms_version ?? null, consent?.age ?? null],
      );
    } catch (e) {
      await db.query("rollback");
      if ((e as { code?: string }).code === "23505") return err(c, 409, "username_taken", "That username is taken.");
      throw e;
    }
    await db.query(`update invites set used_by = $2, used_at = now() where code = $1`, [rows[0].code, c.var.userId]);
    await db.query("commit");
  } catch (e) {
    await db.query("rollback").catch(() => {});
    throw e;
  } finally {
    db.release();
  }
  return c.json({ username: body.data.username }, 201);
});

app.get("/invites", async (c) => {
  const { rows } = await pool.query(
    `select i.code, i.created_at, i.used_at, i.revoked_at, p.username as used_by
     from invites i left join profiles p on p.user_id = i.used_by
     where i.created_by = $1 and i.revoked_at is null order by i.created_at desc`,
    [c.var.userId],
  );
  const limit = c.var.isAdmin ? null : MEMBER_INVITES;
  return c.json({
    invites: rows.map((r) => ({ ...r, link: INVITE_LINK + r.code })),
    limit,
    remaining: limit === null ? null : Math.max(0, limit - rows.length),
  });
});

app.post("/invites", async (c) => {
  if (!c.var.isAdmin) {
    const { rows } = await pool.query(`select count(*)::int as n from invites where created_by = $1 and revoked_at is null`, [c.var.userId]);
    if (rows[0].n >= MEMBER_INVITES) return err(c, 409, "invite_limit", "You've made your 3 invites. Revoke an unused one to make another.");
  }
  const code = newInviteCode();
  await pool.query(`insert into invites (code, created_by) values ($1, $2)`, [code, c.var.userId]);
  return c.json({ invite: { code, link: INVITE_LINK + code } }, 201);
});

app.delete("/invites/:code", async (c) => {
  const { rowCount } = await pool.query(
    `update invites set revoked_at = now() where code = $1 and created_by = $2 and used_by is null and revoked_at is null`,
    [c.req.param("code"), c.var.userId],
  );
  return rowCount ? c.json({ ok: true }) : err(c, 404, "not_found", "That invite is gone or was already used.");
});

// ── friends ──────────────────────────────────────────────────────────────

const MAX_FRIENDS = 200;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

app.get("/friends", async (c) => {
  const { rows } = await pool.query(
    `select case when f.requester_id = $1 then f.addressee_id else f.requester_id end as user_id,
            p.username, f.created_at,
            case when f.status = 'accepted' then 'friend'
                 when f.requester_id = $1 then 'outgoing' else 'incoming' end as status
     from friendships f
     join profiles p on p.user_id = case when f.requester_id = $1 then f.addressee_id else f.requester_id end
     where f.requester_id = $1 or f.addressee_id = $1
     order by p.username`,
    [c.var.userId],
  );
  return c.json({ friends: rows });
});

app.post("/friends", async (c) => {
  const body = z.object({ username: z.string().max(24) }).safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "invalid_username");
  const { rows } = await pool.query(`select user_id, username from profiles where username = $1`, [body.data.username.trim()]);
  const other = rows[0];
  if (!other) return err(c, 404, "user_not_found", "No Tessera account with that username.");
  if (other.user_id === c.var.userId) return err(c, 400, "self", "That's you.");
  if (await blockedEitherWay(c.var.userId, other.user_id)) return err(c, 403, "not_accepting", `${other.username} doesn't accept friend requests from you.`);
  const { rows: count } = await pool.query(
    `select count(*)::int as n from friendships where requester_id = $1 or addressee_id = $1`,
    [c.var.userId],
  );
  if (count[0].n >= MAX_FRIENDS) return err(c, 409, "too_many_friends", "You have 200 friends and requests already.");
  // They asked first: this accepts their request.
  const accepted = await pool.query(
    `update friendships set status = 'accepted', accepted_at = now() where requester_id = $2 and addressee_id = $1 returning status`,
    [c.var.userId, other.user_id],
  );
  if (accepted.rowCount) return c.json({ friend: { ...other, status: "friend" } });
  if (!(await mayRequestFriendship(c.var.userId, other.user_id))) {
    return err(c, 403, "not_accepting", `${other.username} doesn't accept friend requests from you.`);
  }
  const { rows: made } = await pool.query(
    `insert into friendships (requester_id, addressee_id) values ($1, $2)
     on conflict (requester_id, addressee_id) do update set requester_id = excluded.requester_id
     returning case when status = 'accepted' then 'friend' else 'outgoing' end as status`,
    [c.var.userId, other.user_id],
  );
  return c.json({ friend: { ...other, status: made[0].status } }, 201);
});

app.post("/friends/:userId/accept", async (c) => {
  if (!UUID.test(c.req.param("userId"))) return err(c, 404, "not_found");
  const { rowCount } = await pool.query(
    `update friendships set status = 'accepted', accepted_at = now() where requester_id = $2 and addressee_id = $1 and status = 'pending'`,
    [c.var.userId, c.req.param("userId")],
  );
  return rowCount ? c.json({ ok: true }) : err(c, 404, "not_found", "That friend request is gone.");
});

app.delete("/friends/:userId", async (c) => {
  if (!UUID.test(c.req.param("userId"))) return err(c, 404, "not_found");
  const [me, other] = [c.var.userId, c.req.param("userId")];
  await pool.query(`delete from friendships where (requester_id = $1 and addressee_id = $2) or (requester_id = $2 and addressee_id = $1)`, [me, other]);
  // packs are shared with friends only: no longer friends, no longer on each other's packs
  await pool.query(
    `delete from share_members m using shares s
     where s.code = m.code and ((s.owner_id = $1 and m.user_id = $2) or (s.owner_id = $2 and m.user_id = $1))`,
    [me, other],
  );
  return c.json({ ok: true });
});

// ── notifications ────────────────────────────────────────────────────────

/**
 * What the bell shows: friend requests waiting for you, requests of yours that were accepted
 * and packs friends shared with you (both from the last 14 days), newest first. Built from
 * the tables they live in, so there's nothing extra to store or clean up. The launcher asks
 * every 30 seconds (less often in the background): one cheap query.
 */
app.get("/notifications", async (c) => {
  const [list, seen] = await Promise.all([
    pool.query(
      `select * from (
         select 'friend_request' as kind, f.requester_id as user_id, p.username, null::text as code, null::text as name, f.created_at as at
         from friendships f join profiles p on p.user_id = f.requester_id
         where f.addressee_id = $1 and f.status = 'pending'
         union all
         select 'friend_accepted', f.addressee_id, p.username, null, null, f.accepted_at
         from friendships f join profiles p on p.user_id = f.addressee_id
         where f.requester_id = $1 and f.status = 'accepted' and f.accepted_at > now() - interval '14 days'
         union all
         select 'share_added', s.owner_id, p.username, s.code, s.name, m.added_at
         from share_members m join shares s on s.code = m.code join profiles p on p.user_id = s.owner_id
         where m.user_id = $1 and m.added_at > now() - interval '14 days'
       ) n order by at desc limit 30`,
      [c.var.userId],
    ),
    pool.query(`select notifications_seen_at from profiles where user_id = $1`, [c.var.userId]),
  ]);
  const seenAt: Date | null = seen.rows[0]?.notifications_seen_at ?? null;
  const unread = list.rows.filter((n) => !seenAt || new Date(n.at) > seenAt).length;
  return c.json({ notifications: list.rows, seen_at: seenAt, unread });
});

app.post("/notifications/seen", async (c) => {
  await pool.query(`update profiles set notifications_seen_at = now() where user_id = $1`, [c.var.userId]);
  return c.json({ ok: true });
});

// ── privacy and blocking ─────────────────────────────────────────────────

async function blockedEitherWay(a: string, b: string) {
  const { rowCount } = await pool.query(
    `select 1 from blocks where (blocker_id = $1 and blocked_id = $2) or (blocker_id = $2 and blocked_id = $1)`,
    [a, b],
  );
  return !!rowCount;
}

/** `from` may send `to` a friend request: not blocked, and `to`'s setting allows it. */
async function mayRequestFriendship(from: string, to: string) {
  const { rows } = await pool.query(`select friend_requests from profiles where user_id = $1`, [to]);
  const setting = rows[0]?.friend_requests ?? "everyone";
  if (setting === "everyone") return true;
  if (setting === "no_one") return false;
  // friends of friends: someone is friends with both
  const { rowCount } = await pool.query(
    `with mine as (
       select case when requester_id = $1 then addressee_id else requester_id end as id
       from friendships where status = 'accepted' and (requester_id = $1 or addressee_id = $1)
     )
     select 1 from friendships f join mine m on m.id = case when f.requester_id = $2 then f.addressee_id else f.requester_id end
     where f.status = 'accepted' and (f.requester_id = $2 or f.addressee_id = $2) limit 1`,
    [from, to],
  );
  return !!rowCount;
}

/** `from` may add `to` to a shared pack. */
async function mayInviteToShare(from: string, to: string) {
  if (await blockedEitherWay(from, to)) return false;
  const { rows } = await pool.query(`select share_invites from profiles where user_id = $1`, [to]);
  const setting = rows[0]?.share_invites ?? "everyone";
  if (setting === "everyone") return true;
  if (setting === "no_one") return false;
  const { rowCount } = await pool.query(
    `select 1 from friendships where status = 'accepted'
       and ((requester_id = $1 and addressee_id = $2) or (requester_id = $2 and addressee_id = $1))`,
    [from, to],
  );
  return !!rowCount;
}

// a setting left out or null stays as it is (launchers up to 0.6.12 send null for the other one)
const privacy = z.object({
  friend_requests: z.enum(["everyone", "friends_of_friends", "no_one"]).nullish(),
  share_invites: z.enum(["everyone", "friends", "no_one"]).nullish(),
});

app.get("/me/privacy", async (c) => {
  const { rows } = await pool.query(`select friend_requests, share_invites from profiles where user_id = $1`, [c.var.userId]);
  return rows[0] ? c.json({ privacy: rows[0] }) : err(c, 404, "not_found");
});

app.put("/me/privacy", async (c) => {
  const body = privacy.safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "invalid_privacy");
  const { rows } = await pool.query(
    `update profiles set friend_requests = coalesce($2, friend_requests), share_invites = coalesce($3, share_invites)
     where user_id = $1 returning friend_requests, share_invites`,
    [c.var.userId, body.data.friend_requests ?? null, body.data.share_invites ?? null],
  );
  return rows[0] ? c.json({ privacy: rows[0] }) : err(c, 404, "not_found");
});

app.get("/blocks", async (c) => {
  const { rows } = await pool.query(
    `select b.blocked_id as user_id, p.username, b.created_at
     from blocks b join profiles p on p.user_id = b.blocked_id
     where b.blocker_id = $1 order by p.username`,
    [c.var.userId],
  );
  return c.json({ blocks: rows });
});

app.post("/blocks", async (c) => {
  const body = z.object({ username: z.string().max(24) }).safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "invalid_username");
  const { rows } = await pool.query(`select user_id, username from profiles where username = $1`, [body.data.username.trim()]);
  const other = rows[0];
  if (!other) return err(c, 404, "user_not_found", "No Tessera account with that username.");
  if (other.user_id === c.var.userId) return err(c, 400, "self", "That's you.");
  const { rows: count } = await pool.query(`select count(*)::int as n from blocks where blocker_id = $1`, [c.var.userId]);
  if (count[0].n >= 500) return err(c, 409, "too_many_blocks", "You've blocked 500 people already.");
  await pool.query(`insert into blocks (blocker_id, blocked_id) values ($1, $2) on conflict do nothing`, [c.var.userId, other.user_id]);
  await pool.query(
    `delete from friendships where (requester_id = $1 and addressee_id = $2) or (requester_id = $2 and addressee_id = $1)`,
    [c.var.userId, other.user_id],
  );
  // off each other's shared packs
  await pool.query(
    `delete from share_members m using shares s
     where m.code = s.code and ((s.owner_id = $1 and m.user_id = $2) or (s.owner_id = $2 and m.user_id = $1))`,
    [c.var.userId, other.user_id],
  );
  return c.json({ block: { user_id: other.user_id, username: other.username } }, 201);
});

app.delete("/blocks/:userId", async (c) => {
  if (!UUID.test(c.req.param("userId"))) return err(c, 404, "not_found");
  await pool.query(`delete from blocks where blocker_id = $1 and blocked_id = $2`, [c.var.userId, c.req.param("userId")]);
  return c.json({ ok: true });
});

app.notFound((c) => err(c, 404, "not_found"));
app.onError((e, c) => {
  console.error(e);
  return c.json({ error: "internal_error" }, 500);
});

export default app;
