/**
 * Tessera API, a Neon Function next to the "tessera" database.
 *
 * Callers sign in with Neon Auth and send `Authorization: Bearer <JWT>` (15-minute token from
 * Neon Auth's /token). The JWT is checked against Neon Auth's JWKS before any work, and every
 * query is scoped to the caller. DATABASE_URL and the bucket credentials only exist here.
 *
 *   GET    /v1/me                               who am I (+ username)
 *   PUT    /v1/me/username        {username}    pick the name friends add you by
 *   POST   /v1/shares             .tapack body  share an instance (?name=&game_version=&loader=)
 *   PUT    /v1/shares/:code       .tapack body  upload a newer version (owner, or an editor: same
 *                                               Minecraft and loader versions only)
 *   GET    /v1/shares/mine                      what I share, with friends
 *   GET    /v1/shares/received                  what friends share with me
 *   PATCH  /v1/shares/:code       {expires_in_days: 1|7|30|null}
 *   DELETE /v1/shares/:code
 *   POST   /v1/shares/:code/members   {username, role?}
 *   PATCH  /v1/shares/:code/members/:userId   {role: viewer|editor}
 *   DELETE /v1/shares/:code/members/:userId   owner removes a friend, or a friend leaves
 *   POST   /v1/shares/:code/transfer  {user_id}  hand the share to a friend; you stay as editor
 *   GET    /v1/shares/:code                     public: pack info for an invite link
 *   GET    /v1/shares/:code/download            invite link or friend: short-lived download URL
 *   GET    /v1/shares/sync?codes=a,b            live sync: revision of each share I'm on
 *   GET    /v1/friends                          friends + incoming and outgoing requests
 *   POST   /v1/friends           {username}    send a request (accepts theirs if they asked first)
 *   POST   /v1/friends/:userId/accept
 *   DELETE /v1/friends/:userId                  unfriend, decline or cancel a request
 *   GET    /v1/news                             public: launcher news and releases, newest first
 *
 * Closed beta: only accounts with a profile can use the API, and a profile is only made by
 * redeeming an invite (each works once):
 *   GET    /v1/invites/:code                    public: is this invite still good?
 *   POST   /v1/invites/:code/redeem  {username}  join with an invite (right after sign-up)
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
  if (!share) return err(c, 404, "not_found", "That invite link doesn't exist.");
  if (share.expires_at && new Date(share.expires_at) < new Date()) return err(c, 410, "expired", "That invite link has expired.");
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
  const open = (c.req.method === "GET" && path === "/me") || (c.req.method === "POST" && /^\/invites\/[^/]+\/redeem$/.test(path));
  if (!open) {
    const { rows } = await pool.query(`select is_admin from profiles where user_id = $1`, [c.var.userId]);
    if (!rows[0]) return err(c, 403, "invite_required", "Tessera is invite-only for now. Join with an invite link from a friend.");
    c.set("isAdmin", rows[0].is_admin);
  }
  await next();
});

app.get("/me", async (c) => {
  const { rows } = await pool.query(
    `select u.id, u.email, u.name, p.username, coalesce(p.is_admin, false) as is_admin, p.user_id is not null as member
     from neon_auth."user" u left join profiles p on p.user_id = u.id where u.id = $1`,
    [c.var.userId],
  );
  return rows[0] ? c.json({ user: rows[0] }) : err(c, 401, "unauthenticated");
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
  if (!share) return err(c, 404, "not_found", "That invite link doesn't exist.");
  // invited friends always; everyone else only while the invite link is valid
  if (!share.invited && share.expires_at && new Date(share.expires_at) < new Date()) return err(c, 410, "expired", "That invite link has expired.");
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
  const body = z.object({ username: z.string().regex(/^[A-Za-z0-9_]{3,24}$/) }).safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "invalid_username", "Usernames are 3–24 letters, digits or underscores.");
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
      await db.query(`insert into profiles (user_id, username) values ($1, $2)`, [c.var.userId, body.data.username]);
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
  const { rows: count } = await pool.query(
    `select count(*)::int as n from friendships where requester_id = $1 or addressee_id = $1`,
    [c.var.userId],
  );
  if (count[0].n >= MAX_FRIENDS) return err(c, 409, "too_many_friends", "You have 200 friends and requests already.");
  // They asked first: this accepts their request.
  const accepted = await pool.query(
    `update friendships set status = 'accepted' where requester_id = $2 and addressee_id = $1 returning status`,
    [c.var.userId, other.user_id],
  );
  if (accepted.rowCount) return c.json({ friend: { ...other, status: "friend" } });
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
    `update friendships set status = 'accepted' where requester_id = $2 and addressee_id = $1 and status = 'pending'`,
    [c.var.userId, c.req.param("userId")],
  );
  return rowCount ? c.json({ ok: true }) : err(c, 404, "not_found", "That friend request is gone.");
});

app.delete("/friends/:userId", async (c) => {
  if (!UUID.test(c.req.param("userId"))) return err(c, 404, "not_found");
  await pool.query(
    `delete from friendships where (requester_id = $1 and addressee_id = $2) or (requester_id = $2 and addressee_id = $1)`,
    [c.var.userId, c.req.param("userId")],
  );
  return c.json({ ok: true });
});

app.notFound((c) => err(c, 404, "not_found"));
app.onError((e, c) => {
  console.error(e);
  return c.json({ error: "internal_error" }, 500);
});

export default app;
