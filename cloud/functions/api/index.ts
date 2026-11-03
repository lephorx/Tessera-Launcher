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
 *   DELETE /v1/me        {password, confirm, code?}   delete my account, my shares and their packs
 *                                               (code: emailed for purpose delete_account, with two-step sign-in on)
 *   PUT    /v1/me/username        {username}    pick the name friends add you by
 *   POST   /v1/shares             .tapack body  share an instance (?name=&game_version=&loader=)
 *   PUT    /v1/shares/:code       .tapack body  upload a newer version (owner, or an editor: same
 *                                               Minecraft and loader versions only)
 *   GET    /v1/shares/mine                      what I share, with friends
 *   GET    /v1/shares/received                  packs friends share with me (invites I accepted)
 *   GET    /v1/shares/invites                   packs friends invited me to, not answered yet
 *   POST   /v1/shares/:code/accept              take the invite: the pack is mine to install now
 *   PATCH  /v1/shares/:code       {expires_in_days: 1|7|30|null}
 *                                               after expires_at friends can't download, sync or accept
 *                                               it any more; the owner still can, and extends it here
 *   DELETE /v1/shares/:code
 *   POST   /v1/shares/:code/members   {username, role?}   invite a friend (at most 10 per pack); they
 *                                               accept first, declining is DELETE …/members/<me>
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
 *   GET    /v1/me/email                         which notice emails I get
 *   PUT    /v1/me/email          {friends?, shares?}
 *   GET    /v1/email/unsubscribe?u=&k=&t=      public: the link in notice emails (POST turns them off)
 *   POST   /v1/webhooks/neon-authNeon Auth's send.otp webhook (signed): we email its codes ourselves
 *   POST   /v1/email/change      {email, password, new_email}
 *                                               public, password-checked: fixes the address of an
 *                                               account whose email never was verified (a typo, a throwaway)
 *   POST   /v1/site/limit        {bucket, key}  the website's own rate limits (signed by the website)
 *
 * Until the email is verified, an account only gets the calls that are open to non-members too
 * (who am I, export, delete, Terms, joining); the rest answer 403 email_unverified. The first
 * GET /v1/me after verifying emails "you're verified", once per address.
 *
 * Two-step sign-in by email. With it on, only GET /v1/me and the two sign-in calls work until
 * this device sends `X-Tessera-Device: <token>` from /me/2fa/verify (or /enable):
 *   POST   /v1/me/2fa/code       {purpose: sign_in|enable|disable|delete_account}   email me a code
 *   POST   /v1/me/2fa/verify     {code, label?}   sign-in step → {device_token}, good for 30 days
 *   POST   /v1/me/2fa/enable     {code, label?}   turn it on → {device_token} for this device
 *   POST   /v1/me/2fa/disable    {code}           turn it off (forgets every trusted device)
 *   POST   /v1/me/2fa/auto       {label?}         after verifying the email: turns it on unless
 *                                          you chose before → {device_token} for this device
 *   GET    /v1/me/2fa/devices                     trusted devices
 *
 * Where the account is signed in (Neon Auth sessions). The caller says which one is its own
 * with `X-Tessera-Session: <sha256 hex of its session token>`:
 *   GET    /v1/me/sessions                        every signed-in browser and launcher
 *   DELETE /v1/me/sessions/:id                    sign that one out
 *   DELETE /v1/me/sessions                        sign out everywhere but here
 *   DELETE /v1/me/2fa/devices                     forget every device but this one
 *
 * Closed beta: only accounts with a profile can use the API, and a profile is only made by
 * redeeming an invite (each works once):
 *   GET    /v1/invites/:code                    public: is this invite still good?
 *   POST   /v1/invites/:code/redeem  {username, terms_version, age, parental_consent?}
 *                                               join with an invite (right after sign-up)
 *   GET    /v1/invites                          mine, and how many I may still make
 *   POST   /v1/invites                          make one (3 for members, any number for admins)
 *   DELETE /v1/invites/:code                    revoke an unused one
 */
import { createHash, createHmac, createPublicKey, randomBytes, randomInt, scrypt, timingSafeEqual, verify } from "node:crypto";
import { attachDatabasePool } from "@neon/functions";
import { Inflate, strFromU8 } from "fflate";
import { Files } from "files-sdk";
import { neon } from "files-sdk/neon";
import { Hono, type Context } from "hono";
import { createRemoteJWKSet, jwtVerify } from "jose";
import pg from "pg";
import { z } from "zod";
import { checkUnsubscribe, deliverable, mailReady, sendMail, unsubscribeUrl, type NoticeKind } from "./mail.js";
import { usernameProblem } from "./names.js";

const pool = new pg.Pool({ connectionString: process.env.DATABASE_URL, max: 5 });
attachDatabasePool(pool);
const files = new Files({ adapter: neon({ bucket: "packs" }) });
const jwks = createRemoteJWKSet(new URL(process.env.NEON_AUTH_JWKS_URL!));
const issuer = new URL(process.env.NEON_AUTH_BASE_URL!).origin;

const MAX_PACK_BYTES = 50 * 1024 * 1024;
const MAX_SHARES_PER_USER = 20;
/** Friends per shared pack: each one's launcher checks the pack for changes while it's open. */
const MAX_SHARE_MEMBERS = 10;

// ── rate limits ──────────────────────────────────────────────────────────

// Per isolate, in memory: fine for throughput limits. Anything that guards a password or a code
// uses `dbLimit` instead, which every instance shares.

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

/**
 * Fixed-window limit kept in Postgres (rate_limits): shared by every instance and kept across
 * restarts. Keys are stored hashed (they hold addresses). True while under `limit`.
 */
async function dbLimit(key: string, limit: number, windowMs: number): Promise<boolean> {
  const { rows } = await pool.query(
    `insert into rate_limits (key, hits, reset_at) values ($1, 1, now() + make_interval(secs => $2))
     on conflict (key) do update set
       hits = case when rate_limits.reset_at <= now() then 1 else rate_limits.hits + 1 end,
       reset_at = case when rate_limits.reset_at <= now() then excluded.reset_at else rate_limits.reset_at end
     returning hits`,
    [sha256(key), windowMs / 1000],
  );
  if (Math.random() < 0.02) pool.query(`delete from rate_limits where reset_at < now()`).catch(() => {});
  return rows[0].hits <= limit;
}

// ── helpers ──────────────────────────────────────────────────────────────

type Env = { Variables: { userId: string; isAdmin: boolean; twoFactorPending: boolean; deviceId: string | null } };

function err(c: Context, status: 400 | 401 | 403 | 404 | 409 | 410 | 413 | 429 | 503, error: string, message?: string) {
  return c.json({ error, ...(message ? { message } : {}) }, status, status === 429 ? { "retry-after": "60" } : undefined);
}

/**
 * Launchers before 0.6.22 can't answer pack invites (no Accept button, and "Shared with me" hid them):
 * for them an invite works like it used to, listed as shared and taken when they install it.
 */
function answersInvites(c: Context): boolean {
  const v = /^tessera-launcher\/(\d+)\.(\d+)\.(\d+)/.exec(c.req.header("user-agent") ?? "");
  if (!v) return true;
  const [major, minor, patch] = [Number(v[1]), Number(v[2]), Number(v[3])];
  return major > 0 || minor > 6 || (minor === 6 && patch >= 22);
}

/**
 * The caller's address. Neon's function gateway replaces X-Forwarded-For with the address that
 * connected (one entry; checked 2026-11-01), so the last entry is the one to trust if a proxy
 * ever appends instead. X-Real-IP comes from the client unchanged: never used. Calls from the
 * website carry the visitor's address, signed (`siteVisitor`).
 */
function clientIp(c: Context): string {
  return siteVisitor(c) ?? ((BEHIND_CLOUDFLARE && c.req.header("cf-connecting-ip")) || c.req.header("x-forwarded-for")?.split(",").pop()?.trim() || "unknown");
}

/** On the home server the API only listens on localhost and Cloudflare Tunnel is the one way in,
 * so Cloudflare's CF-Connecting-IP is the visitor (see server.ts). */
const BEHIND_CLOUDFLARE = process.env.BEHIND_CLOUDFLARE === "1";

/** Shared with the website (TESSERA_SITE_SECRET): signs the visitor's address and the website's own calls. */
const SITE_SECRET = process.env.TESSERA_SITE_SECRET ?? "";

/**
 * The visitor's address the website vouches for. Its pages look up invites and shares from
 * Vercel's servers, so without this every visitor would share Vercel's limit. The headers are
 * `X-Tessera-Visitor: <ip>` and `X-Tessera-Visitor-Sig: <unix ms>.<HMAC-SHA256 of "visitor:<ms>:<ip>">`,
 * good for two minutes.
 */
function siteVisitor(c: Context): string | null {
  if (!SITE_SECRET) return null;
  const ip = c.req.header("x-tessera-visitor") ?? "";
  const [ts, mac] = (c.req.header("x-tessera-visitor-sig") ?? "").split(".");
  if (!ip || ip.length > 64 || !ts || !/^\d{10,16}$/.test(ts) || !mac) return null;
  if (Math.abs(Date.now() - Number(ts)) > 2 * 60_000) return null;
  const want = Buffer.from(createHmac("sha256", SITE_SECRET).update(`visitor:${ts}:${ip}`).digest("base64url"));
  const got = Buffer.from(mac);
  return want.length === got.length && timingSafeEqual(want, got) ? ip : null;
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

/** More files than any real pack has (configs and all); packs link their mods instead of carrying them. */
const MAX_PACK_ENTRIES = 20_000;
const MAX_MANIFEST_BYTES = 4 * 1024 * 1024;

/**
 * tessera.pack.json out of a .tapack, read from the zip's central directory. The sizes a zip
 * states aren't trusted: a zip bomb states a small one and inflates to gigabytes, so inflating
 * stops once the real output passes 4 MB. "too_many" for more than 20 000 entries, null for
 * anything that isn't a plain zip with the manifest in it.
 */
function packManifest(zip: Uint8Array): Uint8Array | "too_many" | null {
  const v = new DataView(zip.buffer, zip.byteOffset, zip.byteLength);
  // the end-of-central-directory record: the last 22 bytes, or before a comment of up to 64 KB
  let end = -1;
  for (let i = zip.length - 22; i >= Math.max(0, zip.length - 22 - 0xffff); i--) {
    if (v.getUint32(i, true) === 0x06054b50) {
      end = i;
      break;
    }
  }
  if (end < 0) return null;
  const entries = v.getUint16(end + 10, true);
  const dirStart = v.getUint32(end + 16, true);
  // 0xffff / 0xffffffff mean Zip64: only for 65 535+ entries or 4 GB, neither of which a pack has
  if (entries === 0xffff || entries > MAX_PACK_ENTRIES) return "too_many";
  if (dirStart === 0xffffffff || dirStart > end) return null;
  let p = dirStart;
  for (let n = 0; n < entries; n++) {
    if (p + 46 > end || v.getUint32(p, true) !== 0x02014b50) return null;
    const flags = v.getUint16(p + 8, true);
    const method = v.getUint16(p + 10, true);
    const compressed = v.getUint32(p + 20, true);
    const nameLen = v.getUint16(p + 28, true);
    const local = v.getUint32(p + 42, true);
    const name = strFromU8(zip.subarray(p + 46, p + 46 + nameLen));
    p += 46 + nameLen + v.getUint16(p + 30, true) + v.getUint16(p + 32, true);
    if (name !== "tessera.pack.json") continue;
    if (flags & 1 || local + 30 > zip.length || v.getUint32(local, true) !== 0x04034b50) return null; // encrypted, or no file there
    const start = local + 30 + v.getUint16(local + 26, true) + v.getUint16(local + 28, true);
    if (start + compressed > zip.length) return null;
    const data = zip.subarray(start, start + compressed);
    if (method === 0) return data.length <= MAX_MANIFEST_BYTES ? data : null;
    return method === 8 && data.length ? inflateCapped(data, MAX_MANIFEST_BYTES) : null;
  }
  return null;
}

/** Inflates raw deflate data in small steps, giving up (null) once the output passes `max` bytes. */
function inflateCapped(data: Uint8Array, max: number): Uint8Array | null {
  const parts: Uint8Array[] = [];
  let size = 0;
  // 4 KB in inflates to 4 MB out at most (deflate tops out near 1:1032), so one step can't run away
  const inflate = new Inflate((chunk) => {
    size += chunk.length;
    if (size <= max) parts.push(chunk.slice());
  });
  try {
    for (let i = 0; i < data.length && size <= max; i += 4096) inflate.push(data.subarray(i, i + 4096), i + 4096 >= data.length);
  } catch {
    return null;
  }
  if (size > max) return null;
  const out = new Uint8Array(size);
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

/** Minecraft and loader versions from the pack's tessera.pack.json: what the share really is. */
function packVersions(body: Uint8Array): { game_version: string; loader: string; loader_version: string | null } | "too_many" | null {
  try {
    const raw = packManifest(body);
    if (!raw || raw === "too_many") return raw;
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

const SHARE_COLUMNS = `s.code, s.name, s.game_version, s.loader, s.size_bytes, s.created_at, s.updated_at, s.expires_at,
  coalesce(s.expires_at <= now(), false) as expired, s.revision, p.username as owner`;

/** A share past its expires_at: only its owner can still get at it (and extend it). */
const NOT_EXPIRED = `(s.expires_at is null or s.expires_at > now())`;
const EXPIRED_MESSAGE = "This pack expired. Ask its owner to extend it in the launcher.";

/**
 * Runs `f` in a transaction holding advisory locks on `keys` ("shares:<user>", "members:<code>",
 * "invites:<user>"), so a limit counted inside it holds when requests come in at once. Always
 * take "members:" before "shares:" so two calls never wait on each other. Returning an error
 * Response (a refusal) rolls back whatever `f` did.
 */
async function locked<T>(keys: string[], f: (db: pg.PoolClient) => Promise<T>): Promise<T> {
  const db = await pool.connect();
  try {
    await db.query("begin");
    for (const key of keys) await db.query(`select pg_advisory_xact_lock(hashtextextended($1, 0))`, [key]);
    const result = await f(db);
    await db.query(result instanceof Response && !result.ok ? "rollback" : "commit");
    return result;
  } catch (e) {
    await db.query("rollback").catch(() => {});
    throw e;
  } finally {
    db.release();
  }
}

/** The answer when a .tapack's manifest can't be read. */
function packRefused(c: Context, versions: "too_many" | null) {
  return versions === "too_many"
    ? err(c, 413, "too_many_files", "That pack has too many files in it. Leave worlds and caches out of the pack.")
    : err(c, 400, "not_a_pack", "That isn't a .tapack file.");
}

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

// Public: launcher news, with each launcher release as a post of its own (not the Nightly builds
// and their update feed: those are "nightly" and "nightly-<run>").
const RELEASES_URL = "https://api.github.com/repos/lephorx/Tessera-Launcher/releases?per_page=30";
let releasesCache: { at: number; items: unknown[] } = { at: 0, items: [] };

/**
 * A release's notes as one short paragraph: the text without its title line ("Tessera Launcher
 * 0.6.20", older tags start with it), pictures and Markdown marks. Lists keep only their bold
 * lead-ins ("Verify your email · Two-step sign-in"); cut at a word near 280 characters.
 */
function summary(markdown: string): string {
  const plain = (s: string) => s.replace(/\[([^\]]+)\]\([^)]+\)/g, "$1").replace(/[#*`>]/g, "").replace(/\s+/g, " ").trim();
  const parts = markdown
    .split(/\r?\n\s*\r?\n/)
    .map((p) => p.trim())
    .filter((p) => p && !p.startsWith("#") && !/^!\[[^\]]*\]\([^)]+\)$/.test(p) && !/^Tessera Launcher v?\d[\w.-]*$/i.test(p))
    .map((p) =>
      /^\s*[-*]\s/.test(p)
        ? p.split(/\r?\n/).map((l) => (l.match(/^\s*[-*]\s+\*\*(.+?)\*\*/)?.[1] ?? plain(l.replace(/^\s*[-*]\s+/, ""))).replace(/[.:]$/, "")).join(" · ")
        : plain(p),
    );
  let text = "";
  for (const p of parts) {
    text = text ? `${text} ${p}` : p;
    if (text.length >= 120) break;
  }
  return text.length > 280 ? `${text.slice(0, 280).replace(/\s+\S*$/, "")}…` : text;
}

/** The first picture in a release's notes, for the news card. */
const firstImage = (markdown: string) => markdown.match(/!\[[^\]]*\]\((https:\/\/[^)\s]+)\)/)?.[1] ?? null;

async function releases(): Promise<unknown[]> {
  if (Date.now() - releasesCache.at < 10 * 60_000) return releasesCache.items;
  try {
    const res = await fetch(RELEASES_URL, { headers: { accept: "application/vnd.github+json", "user-agent": "tessera-api" }, signal: AbortSignal.timeout(5000) });
    if (res.ok) {
      const list = (await res.json()) as { id: number; name: string | null; tag_name: string; body: string | null; html_url: string; published_at: string; draft: boolean; prerelease: boolean }[];
      releasesCache = {
        at: Date.now(),
        items: list
          .filter((r) => !r.draft && !r.tag_name.startsWith("nightly"))
          .slice(0, 5)
          .map((r) => ({
            id: 1_000_000_000 + (r.id % 1_000_000_000),
            title: `Tessera Launcher ${r.tag_name.replace(/^v/, "")}${r.prerelease ? " (beta)" : ""}`,
            body: summary(r.body ?? "") || "A new version of the launcher is out.",
            url: r.html_url,
            image_url: firstImage(r.body ?? ""),
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

// Public: the "turn these emails off" link in notices. GET only asks (mail scanners open links),
// POST turns them off: the button on that page, or the mail app's one-click unsubscribe.
const unsubscribeQuery = (c: Context) => ({ u: c.req.query("u") ?? "", k: c.req.query("k") ?? "", t: c.req.query("t") ?? "" });

function unsubscribePage(c: Context, body: string, status: 200 | 400 = 200) {
  return c.html(
    `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Tessera emails</title></head>
<body style="margin:0;background:#f4f4f8;font-family:Inter,-apple-system,Segoe UI,sans-serif;color:#17171f">
<div style="max-width:440px;margin:12vh auto;background:#fff;border-radius:20px;padding:28px">
<p style="margin:0 0 14px;font-weight:800;font-size:18px">Tessera</p>${body}</div></body></html>`,
    status,
  );
}

const NOTICE_NAMES: Record<NoticeKind, string> = { friends: "friend request", shares: "shared pack" };

app.get("/email/unsubscribe", (c) => {
  const { u, k, t } = unsubscribeQuery(c);
  if (!UUID.test(u) || !checkUnsubscribe(u, k, t)) return unsubscribePage(c, `<p>That link doesn't work. Change your email settings in the launcher instead.</p>`, 400);
  const q = new URLSearchParams({ u, k, t });
  return unsubscribePage(
    c,
    `<p>Stop getting ${NOTICE_NAMES[k]} emails? You can turn them back on in the launcher's settings.</p>
<form method="post" action="?${q}"><button style="background:#5b5bf0;color:#fff;border:0;padding:11px 18px;border-radius:12px;font-weight:600;font-size:15px;cursor:pointer">Turn them off</button></form>`,
  );
});

app.post("/email/unsubscribe", async (c) => {
  const { u, k, t } = unsubscribeQuery(c);
  if (!UUID.test(u) || !checkUnsubscribe(u, k, t)) return unsubscribePage(c, `<p>That link doesn't work.</p>`, 400);
  await pool.query(`update profiles set ${k === "friends" ? "email_friends" : "email_shares"} = false where user_id = $1`, [u]);
  return unsubscribePage(c, `<p>Done: no more ${NOTICE_NAMES[k]} emails.</p>`);
});

// Public but signed: Neon Auth hands us its one-time codes (verify email, reset password) through
// the send.otp webhook and skips its own email, so every email comes from Tessera. The body is
// signed with Ed25519 as a detached JWS, keyed from Neon Auth's JWKS. A non-2xx answer makes Neon
// Auth retry (3 tries, 15 s in all) and then fail the sign-up, so only send failures return 5xx.
let webhookKeys: { at: number; keys: (JsonWebKey & { kid?: string })[] } = { at: 0, keys: [] };

async function webhookKey(kid: string) {
  const find = () => webhookKeys.keys.find((k) => k.kid === kid);
  // refresh for a new kid (rotation), at most once a minute
  if (!find() && Date.now() - webhookKeys.at > 60_000) {
    const res = await fetch(process.env.NEON_AUTH_JWKS_URL!, { signal: AbortSignal.timeout(4000) });
    if (res.ok) webhookKeys = { at: Date.now(), keys: ((await res.json()) as { keys: (JsonWebKey & { kid?: string })[] }).keys };
  }
  const jwk = find();
  return jwk ? createPublicKey({ key: jwk, format: "jwk" }) : null;
}

async function webhookSigned(c: Context, body: string): Promise<boolean> {
  // the website relay renames them: Neon's function gateway drops x-neon-* headers
  const sig = c.req.header("x-webhook-signature") ?? "";
  const kid = c.req.header("x-webhook-signature-kid") ?? "";
  const ts = c.req.header("x-webhook-timestamp") ?? "";
  const [header, middle, signature] = sig.split(".");
  if (!header || middle !== "" || !signature || !kid || !/^\d+$/.test(ts)) return false;
  if (Math.abs(Date.now() - Number(ts)) > 5 * 60_000) return false;
  const key = await webhookKey(kid).catch(() => null);
  if (!key) return false;
  const payload = Buffer.from(`${ts}.${Buffer.from(body).toString("base64url")}`).toString("base64url");
  return verify(null, Buffer.from(`${header}.${payload}`), key, Buffer.from(signature, "base64url"));
}

type OtpMail = { subject: string; title: string; line: string };
const OTP_MAIL: Record<string, OtpMail | undefined> = {
  "email-verification": { subject: "Verify your Tessera email", title: "Verify your email", line: "Enter this code in Tessera to verify your email address:" },
  "sign-in": { subject: "Your Tessera sign-in code", title: "Sign in to Tessera", line: "Enter this code in Tessera to sign in:" },
  "forget-password": { subject: "Reset your Tessera password", title: "Reset your password", line: "Enter this code in Tessera to choose a new password:" },
};
// retries reuse the event id: answer them without a second email
const webhookEvents = new Map<string, number>();

app.post("/webhooks/neon-auth", async (c) => {
  const body = await c.req.text();
  if (!(await webhookSigned(c, body))) return err(c, 401, "bad_signature");
  const event = z
    .object({
      event_id: z.string(),
      event_type: z.string(),
      user: z.object({ email: z.string().email().optional() }).passthrough().optional(),
      event_data: z.object({ otp_code: z.string().optional(), otp_type: z.string().optional() }).passthrough().optional(),
    })
    .safeParse((() => { try { return JSON.parse(body); } catch { return null; } })());
  if (!event.success) return err(c, 400, "invalid_event");
  const { event_id, event_type, user, event_data } = event.data;
  if (event_type !== "send.otp") return c.json({ ok: true });
  const to = user?.email;
  const code = event_data?.otp_code;
  const text = OTP_MAIL[event_data?.otp_type ?? ""] ?? (OTP_MAIL["email-verification"] as OtpMail);
  if (!to || !code) return err(c, 400, "invalid_event");
  if (webhookEvents.has(event_id)) return c.json({ ok: true });
  for (const [id, at] of webhookEvents) if (Date.now() - at > 30 * 60_000) webhookEvents.delete(id);
  const sent = await sendMail({
    to,
    subject: text.subject,
    title: text.title,
    lines: [text.line],
    code,
    after: ["It works for 10 minutes. If you didn't ask for it, you can ignore this email."],
  });
  // test addresses (example.com) never get mail: that's fine, not a failure
  if (!sent && deliverable(to)) return err(c, 503, "email_unavailable");
  webhookEvents.set(event_id, Date.now());
  return c.json({ ok: true });
});

// Public, but it needs the account's password: fixes the address of an account whose email was
// never verified, so a typo or a throwaway address doesn't lock anyone out (Neon Auth won't sign
// an unverified account in, so there is no session to ask for, and it only emails the address on
// file). An address that is or ever was verified stays put: whoever knows the password would
// otherwise take the account. Wrong password, unknown account and verified account all get the
// same answer, after the same scrypt work, so the call doesn't tell which one it was. The caller
// then asks Neon Auth for a code to the new address.
const EMAIL_CHANGE_MESSAGE = "Wrong email or password. (An email that was verified can't be changed here.)";
/** Checked when there's no account, so an unknown address takes as long as a wrong password. */
const DUMMY_PASSWORD_HASH = `${randomBytes(16).toString("hex")}:${randomBytes(64).toString("hex")}`;

/** Checks a password against Neon Auth's hash (Better Auth: scrypt N=16384 r=16 p=1, "salt:key" hex). */
function passwordMatches(password: string, stored: string): Promise<boolean> {
  const [salt, key] = stored.split(":");
  if (!salt || !key) return Promise.resolve(false);
  return new Promise((resolve) =>
    scrypt(password.normalize("NFKC"), salt, 64, { N: 16384, r: 16, p: 1, maxmem: 64 * 1024 * 1024 }, (e, derived) => {
      const want = Buffer.from(key, "hex");
      resolve(!e && want.length === derived.length && timingSafeEqual(want, derived));
    }),
  );
}

app.post("/email/change", async (c) => {
  const body = z
    .object({ email: z.string().trim().email().max(254), password: z.string().min(1).max(200), new_email: z.string().trim().email().max(254) })
    .safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "invalid_email", "That email address doesn't look right.");
  const email = body.data.email.toLowerCase();
  const next = body.data.new_email.toLowerCase();
  const [byIp, byEmail] = [await dbLimit(`email-change:ip:${clientIp(c)}`, 5, 15 * 60_000), await dbLimit(`email-change:${email}`, 5, 15 * 60_000)];
  if (!byIp || !byEmail) return err(c, 429, "rate_limited", "Too many attempts. Wait a few minutes.");
  const { rows } = await pool.query(
    `select u.id, u."emailVerified" or exists (select 1 from verified_users v where v.user_id = u.id) as ever_verified, a.password
     from neon_auth."user" u
     join neon_auth.account a on a."userId" = u.id and a."providerId" = 'credential' where lower(u.email) = $1`,
    [email],
  );
  const user = rows[0];
  const matches = await passwordMatches(body.data.password, user?.password || DUMMY_PASSWORD_HASH);
  if (!user?.password || !matches || user.ever_verified) return err(c, 401, "wrong_password", EMAIL_CHANGE_MESSAGE);
  if (next === email) return c.json({ ok: true, email: next });
  const taken = await pool.query(`select 1 from neon_auth."user" where lower(email) = $1`, [next]);
  if (taken.rows[0]) return err(c, 409, "email_taken", "There is already an account with that email.");
  await pool.query(`update neon_auth."user" set email = $2, "emailVerified" = false, "updatedAt" = now() where id = $1`, [user.id, next]);
  // codes sent to the old address stop working
  await pool.query(`delete from neon_auth.verification where right(identifier, length($1) + 1) = '-' || $1`, [email]);
  return c.json({ ok: true, email: next });
});

/** sha256 hex, for codes and device tokens: only hashes are stored. */
const sha256 = (s: string) => createHash("sha256").update(s).digest("hex");

// The website's own limits, kept here in Postgres so every Vercel instance counts together: form
// posts (sign-in tries) per visitor, and "send a new code" per address. Only the website can ask
// (it signs the call with the visitor's address); the limits themselves are set here.
const SITE_LIMITS = { action: [10, 60_000], resend_minute: [1, 60_000], resend_hour: [5, 60 * 60_000] } as const;

app.post("/site/limit", async (c) => {
  if (!siteVisitor(c)) return err(c, 401, "bad_signature");
  const body = z
    .object({ bucket: z.enum(["action", "resend_minute", "resend_hour"]), key: z.string().min(1).max(300) })
    .safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "invalid_limit");
  const [limit, windowMs] = SITE_LIMITS[body.data.bucket];
  return c.json({ ok: await dbLimit(`site:${body.data.bucket}:${body.data.key.toLowerCase()}`, limit, windowMs) });
});

// Everything else needs a signed-in user.
app.use("*", async (c, next) => {
  const auth = c.req.header("authorization");
  if (!auth?.toLowerCase().startsWith("bearer ")) return err(c, 401, "unauthenticated");
  try {
    // Neon Auth signs with Ed25519 and sets aud to its own origin (checked 2026-11-01)
    const { payload } = await jwtVerify(auth.slice(7), jwks, { issuer, audience: issuer, algorithms: ["EdDSA"] });
    if (!payload.sub) return err(c, 401, "unauthenticated");
    c.set("userId", payload.sub);
  } catch {
    return err(c, 401, "unauthenticated");
  }
  if (!perUser.take(c.var.userId)) return err(c, 429, "rate_limited", "Too many requests. Slow down a little.");
  const path = new URL(c.req.url).pathname.replace(/^\/v1/, "");
  const device = c.req.header("x-tessera-device")?.slice(0, 200);
  const { rows } = await pool.query(
    `select p.is_admin, p.two_factor_email, u."emailVerified" as verified,
       (select d.id from trusted_devices d where d.user_id = p.user_id and d.token_hash = $2 and d.expires_at > now()) as device_id
     from profiles p join neon_auth."user" u on u.id = p.user_id where p.user_id = $1`,
    [c.var.userId, device ? sha256(device) : ""],
  );
  const profile = rows[0];
  c.set("deviceId", profile?.device_id ?? null);
  // two-step sign-in: until this device entered a code, only "who am I" and the code itself
  const pending = !!profile?.two_factor_email && !profile.device_id;
  c.set("twoFactorPending", pending);
  if (pending && !((c.req.method === "GET" && path === "/me") || (c.req.method === "POST" && (path === "/me/2fa/code" || path === "/me/2fa/verify")))) {
    return err(c, 403, "two_factor_required", "Enter the code we emailed you to finish signing in.");
  }
  // closed beta: everything but "who am I" and joining needs a redeemed invite
  // also open: your own data (export, delete) and accepting the Terms, member or not
  const open =
    (c.req.method === "GET" && (path === "/me" || path === "/me/export")) ||
    (c.req.method === "DELETE" && path === "/me") ||
    // seeing and ending your sign-ins works before joining and before verifying too
    path === "/me/sessions" || path.startsWith("/me/sessions/") ||
    (c.req.method === "POST" && (path === "/me/consent" || /^\/invites\/[^/]+\/redeem$/.test(path)));
  if (!open && !profile) return err(c, 403, "invite_required", "Tessera is invite-only for now. Join with an invite link from a friend.");
  if (!open && !profile.verified) return err(c, 403, "email_unverified", "Verify your email first.");
  c.set("isAdmin", !!profile?.is_admin);
  await next();
});

app.get("/me", async (c) => {
  const { rows } = await pool.query(
    `select u.id, u.email, u.name, p.username, coalesce(p.is_admin, false) as is_admin, p.user_id is not null as member,
       p.terms_version, p.age_declaration, u."emailVerified" as email_verified,
       coalesce(p.two_factor_email, false) as two_factor
     from neon_auth."user" u left join profiles p on p.user_id = u.id where u.id = $1`,
    [c.var.userId],
  );
  if (!rows[0]) return err(c, 401, "unauthenticated");
  if (rows[0].email_verified) {
    verifiedMail(c.var.userId, rows[0].email);
    rememberVerified(c.var.userId);
  }
  // waiting for the emailed code: nothing but who it is
  const user = c.var.twoFactorPending
    ? { id: rows[0].id, email: rows[0].email, name: rows[0].name, username: rows[0].username, member: rows[0].member, two_factor: true }
    : rows[0];
  return c.json({ user: { ...user, two_factor_required: c.var.twoFactorPending } });
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
  const [account, profile, sharesOwned, memberships, friendships, invitesCreated, inviteUsed, blocks, devices] = await Promise.all([
    // Better Auth's columns are camelCase; read created_at either way
    q(`select u.email, u.name, coalesce(to_jsonb(u)->>'createdAt', to_jsonb(u)->>'created_at') as created_at
       from neon_auth."user" u where u.id = $1`),
    q(`select username, created_at, is_admin, friend_requests, share_invites, terms_version, terms_accepted_at, age_declaration,
         email_friends, email_shares, two_factor_email
       from profiles where user_id = $1`),
    q(`select s.code, s.name, s.game_version, s.loader, s.loader_version, s.size_bytes, s.created_at, s.updated_at, s.expires_at, s.revision,
         coalesce((select json_agg(json_build_object('username', mp.username, 'role', m.role, 'added_at', m.added_at, 'accepted_at', m.accepted_at) order by mp.username)
                   from share_members m join profiles mp on mp.user_id = m.user_id where m.code = s.code), '[]') as members
       from shares s where s.owner_id = $1 order by s.created_at`),
    q(`select s.code, s.name, p.username as owner, m.role, m.added_at, m.accepted_at
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
    q(`select label, created_at, last_used_at, expires_at from trusted_devices where user_id = $1 order by created_at`),
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
      trusted_devices: devices,
      exported_at: new Date().toISOString(),
    },
    200,
    { "content-disposition": 'attachment; filename="tessera-data.json"' },
  );
});

// Deletes the account (GDPR art. 17): the packs of every share I own from the bucket, then the
// Neon Auth user; profiles, shares, memberships, friendships, invites I made and blocks go with
// it by foreign-key cascade (invites I used and shares I last updated keep a null in my place).
// Deleting takes the password and the username (or the email, without one) typed out: a
// signed-in computer someone else sits at can't do it in one click. With two-step sign-in on it
// also takes a code emailed for it (POST /me/2fa/code {purpose: "delete_account"}).
app.delete("/me", async (c) => {
  const id = c.var.userId;
  const body = z
    .object({ password: z.string().min(1).max(200), confirm: z.string().trim(), code: z.string().trim().optional() })
    .safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "confirm_required", "Enter your password and type your username to delete the account.");
  const { rows: who } = await pool.query(
    `select u.email, p.username, a.password, coalesce(p.two_factor_email, false) as two_factor
     from neon_auth."user" u left join profiles p on p.user_id = u.id
     left join neon_auth.account a on a."userId" = u.id and a."providerId" = 'credential' where u.id = $1`,
    [id],
  );
  const me = who[0];
  if (!me) return err(c, 401, "unauthenticated");
  if (body.data.confirm.toLowerCase() !== String(me.username ?? me.email).toLowerCase()) {
    return err(c, 400, "wrong_confirm", `Type ${me.username ?? me.email} exactly to confirm.`);
  }
  // counted from here: password and code guesses, not typos in the username
  if (!(await dbLimit(`delete:${id}`, 5, 15 * 60_000))) return err(c, 429, "rate_limited", "Too many attempts. Wait a few minutes.");
  if (!me.password || !(await passwordMatches(body.data.password, me.password))) return err(c, 401, "wrong_password", "That password isn't right.");
  if (me.two_factor) {
    const code = body.data.code ?? "";
    if (!/^\d{6}$/.test(code)) return err(c, 400, "code_required", "Two-step sign-in is on: enter the code we emailed you for deleting the account.");
    const ok = await takeCode(id, "delete_account", code);
    if (ok !== true) return codeError(c, ok);
  }
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
  const body = z.object({ username: z.string().trim() }).safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "invalid_username", "3–24 letters, digits or underscores.");
  const problem = usernameProblem(body.data.username);
  if (problem) return err(c, 400, "invalid_username", problem);
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
  if (!versions || versions === "too_many") return packRefused(c, versions);

  const code = newCode();
  const key = `shares/${code}.tapack`;
  await files.upload(key, body, { contentType: "application/zip" });
  // counted again under the lock: parallel uploads can't pass the limit together
  const share = await locked([`shares:${c.var.userId}`], async (db) => {
    const { rows: mine } = await db.query(`select count(*)::int as n from shares where owner_id = $1`, [c.var.userId]);
    if (mine[0].n >= MAX_SHARES_PER_USER) return null;
    const { rows } = await db.query(
      `insert into shares (code, owner_id, name, game_version, loader, loader_version, size_bytes, object_key, expires_at)
       values ($1, $2, $3, $4, $5, $6, $7, $8, now() + interval '7 days') returning code, expires_at, revision`,
      [code, c.var.userId, meta.data.name, versions.game_version, versions.loader, versions.loader_version, body.length, key],
    );
    return rows[0];
  });
  if (!share) {
    await files.delete(key).catch(() => {});
    return err(c, 409, "too_many_shares", "You share 20 packs already. Stop sharing one first.");
  }
  return c.json({ share }, 201);
});

/** The share if the caller owns it. */
async function owned(c: Context<Env>) {
  const { rows } = await pool.query(`select * from shares where code = $1 and owner_id = $2`, [c.req.param("code"), c.var.userId]);
  return rows[0] ?? null;
}

/** The share and the caller's role in it: owner, editor, viewer, or null. */
async function access(c: Context<Env>) {
  const { rows } = await pool.query(
    `select s.*, case when s.owner_id = $2 then 'owner' else m.role end as role, not ${NOT_EXPIRED} as expired
     from shares s left join share_members m on m.code = s.code and m.user_id = $2 and m.accepted_at is not null where s.code = $1`,
    [c.req.param("code"), c.var.userId],
  );
  return rows[0] ?? null;
}

app.put("/shares/:code", async (c) => {
  if (!uploads.take(c.var.userId)) return err(c, 429, "rate_limited", "That's a lot of uploads. Try again in an hour.");
  const share = await access(c);
  if (!share || !share.role) return err(c, 404, "not_found");
  if (share.role === "viewer") return err(c, 403, "read_only", "You can install this pack but not change it. Ask the owner to make you an editor.");
  if (share.expired && share.role !== "owner") return err(c, 410, "expired", EXPIRED_MESSAGE);
  const meta = packMeta.safeParse(c.req.query());
  if (!meta.success) return err(c, 400, "invalid_pack");
  const body = await readPack(c);
  if (body instanceof Response) return body;
  const versions = packVersions(body);
  if (!versions || versions === "too_many") return packRefused(c, versions);
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
     left join share_members m on m.code = s.code and m.user_id = $2 and m.accepted_at is not null
     left join profiles up on up.user_id = s.updated_by
     where s.code = any($1) and (s.owner_id = $2 or (m.user_id is not null and ${NOT_EXPIRED}))`,
    [codes, c.var.userId],
  );
  return c.json({ shares: rows });
});

app.get("/shares/mine", async (c) => {
  const { rows } = await pool.query(
    `select ${SHARE_COLUMNS},
       coalesce((select json_agg(json_build_object('user_id', m.user_id, 'username', mp.username, 'role', m.role, 'pending', m.accepted_at is null) order by mp.username)
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
     left join profiles p on p.user_id = s.owner_id where m.user_id = $1 and (m.accepted_at is not null or not $2) order by s.updated_at desc`,
    [c.var.userId, answersInvites(c)],
  );
  return c.json({ shares: rows });
});

app.get("/shares/invites", async (c) => {
  const { rows } = await pool.query(
    `select ${SHARE_COLUMNS}, m.role, m.added_at as invited_at from share_members m join shares s on s.code = m.code
     left join profiles p on p.user_id = s.owner_id where m.user_id = $1 and m.accepted_at is null order by m.added_at desc`,
    [c.var.userId],
  );
  return c.json({ invites: rows });
});

app.post("/shares/:code/accept", async (c) => {
  const { rows } = await pool.query(
    `update share_members m set accepted_at = coalesce(m.accepted_at, now()) from shares s
     where s.code = m.code and m.code = $1 and m.user_id = $2 and ${NOT_EXPIRED} returning m.role`,
    [c.req.param("code"), c.var.userId],
  );
  if (rows[0]) return c.json({ ok: true, role: rows[0].role });
  const { rowCount } = await pool.query(`select 1 from share_members where code = $1 and user_id = $2`, [c.req.param("code"), c.var.userId]);
  return rowCount ? err(c, 410, "expired", EXPIRED_MESSAGE) : err(c, 404, "not_found", "That invite is gone. Its owner may have taken it back.");
});

app.get("/shares/:code", async (c) => {
  const { rows } = await pool.query(
    `select ${SHARE_COLUMNS}, s.loader_version,
       case when s.owner_id = $2 then 'owner' else (select role from share_members where code = s.code and user_id = $2 and accepted_at is not null) end as role
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
  if (share.expires_at && new Date(share.expires_at) <= new Date()) return err(c, 410, "expired", "This pack expired. Extend it first, then invite friends.");
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
  if (!(await mayInviteToShare(c.var.userId, friend.user_id))) {
    return err(c, 403, "not_accepting", `${friend.username} doesn't accept shared packs from you.`);
  }
  // count and add under the share's lock: parallel invites can't pass the limit together
  const added = await locked([`members:${share.code}`], async (db) => {
    const { rows: count } = await db.query(`select count(*)::int as n from share_members where code = $1 and user_id <> $2`, [share.code, friend.user_id]);
    if (count[0].n >= MAX_SHARE_MEMBERS) return null;
    return db.query(`insert into share_members (code, user_id, role) values ($1, $2, $3) on conflict do nothing`, [share.code, friend.user_id, body.data.role]);
  });
  if (!added) return err(c, 409, "too_many_members", `A pack can be shared with ${MAX_SHARE_MEMBERS} friends.`);
  if (added.rowCount) notify(friend.user_id, { kind: "share_added", from: await usernameOf(c.var.userId), code: share.code, pack: share.name });
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

// The new owner must already be on the share; the old owner stays on it as an editor. Everyone
// left on it has to be someone the new owner could add: friends with them, nobody blocked either
// way, and not closed to shared packs, the same rules as inviting a friend.
app.post("/shares/:code/transfer", async (c) => {
  const share = await owned(c);
  if (!share) return err(c, 404, "not_found");
  const body = z.object({ user_id: z.string().regex(UUID_RE) }).safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "invalid_user");
  const to = body.data.user_id;
  return locked([`members:${share.code}`, `shares:${to}`], async (db) => {
    const { rowCount: still } = await db.query(`select 1 from shares where code = $1 and owner_id = $2 for update`, [share.code, c.var.userId]);
    if (!still) return err(c, 404, "not_found");
    const { rowCount } = await db.query(`delete from share_members where code = $1 and user_id = $2 and accepted_at is not null`, [share.code, to]);
    if (!rowCount) return err(c, 404, "not_member", "They have to accept the invite first, then you can make them the owner.");
    const { rows: count } = await db.query(`select count(*)::int as n from shares where owner_id = $1`, [to]);
    if (count[0].n >= MAX_SHARES_PER_USER) return err(c, 409, "too_many_shares", "They already own 20 shares.");
    const { rows: refused } = await db.query(
      `with people as (
         select user_id, false as old_owner from share_members where code = $1
         union all select $3::uuid, true
       )
       select p.username from people x join profiles p on p.user_id = x.user_id
       where not exists (select 1 from friendships f where f.status = 'accepted'
                           and ((f.requester_id = $2 and f.addressee_id = x.user_id) or (f.requester_id = x.user_id and f.addressee_id = $2)))
          or exists (select 1 from blocks b where (b.blocker_id = $2 and b.blocked_id = x.user_id) or (b.blocker_id = x.user_id and b.blocked_id = $2))
          or (not x.old_owner and p.share_invites = 'no_one')
       order by p.username`,
      [share.code, to, c.var.userId],
    );
    if (refused.length) {
      const names = refused.map((r) => r.username).join(", ");
      return err(c, 409, "members_not_allowed", `Everyone on the pack has to be friends with the new owner, and nobody blocked. Not so for: ${names}. Take them off the pack first.`);
    }
    await db.query(`update shares set owner_id = $2, updated_at = updated_at where code = $1`, [share.code, to]);
    await db.query(
      `insert into share_members (code, user_id, role, accepted_at) values ($1, $2, 'editor', now())
       on conflict (code, user_id) do update set role = 'editor', accepted_at = coalesce(share_members.accepted_at, now())`,
      [share.code, c.var.userId],
    );
    return c.json({ ok: true });
  });
});

app.get("/shares/:code/download", async (c) => {
  // an older launcher installing a pack it was invited to: that's its accept
  if (!answersInvites(c)) {
    await pool.query(
      `update share_members m set accepted_at = now() from shares s
       where s.code = m.code and m.code = $1 and m.user_id = $2 and m.accepted_at is null and ${NOT_EXPIRED}`,
      [c.req.param("code"), c.var.userId],
    );
  }
  const { rows } = await pool.query(
    `select s.*, (s.owner_id = $2 or exists (select 1 from share_members m where m.code = s.code and m.user_id = $2 and m.accepted_at is not null)) as invited,
       s.owner_id = $2 as mine, not ${NOT_EXPIRED} as expired
     from shares s where s.code = $1`,
    [c.req.param("code"), c.var.userId],
  );
  const share = rows[0];
  if (!share) return err(c, 404, "not_found", "That shared pack doesn't exist.");
  // shared packs are for the owner's friends they added: a link alone doesn't open it
  if (!share.invited) return err(c, 403, "friends_only", "This pack is shared with friends only. Ask its owner to invite you, then accept the invite.");
  if (share.expired && !share.mine) return err(c, 410, "expired", EXPIRED_MESSAGE);
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
  const body = z.object({ username: z.string().trim() }).safeParse(raw);
  if (!body.success) return err(c, 400, "invalid_username", "Usernames are 3–24 letters, digits or underscores.");
  const problem = usernameProblem(body.data.username);
  if (problem) return err(c, 400, "invalid_username", problem);
  // joining needs the age and the Terms (launchers from before the Terms sent none: they update first)
  const sent = raw && typeof raw === "object" && ["terms_version", "age"].some((k) => k in raw);
  if (!sent) return err(c, 400, "consent_required", "Update Tessera Launcher, then join again: it asks for your age and the Terms first.");
  const consent = checkConsent(c, raw);
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
         values ($1, $2, $3, now(), $4, case when $4::text = '13-15' then 'friends_of_friends' else 'everyone' end)`,
        [c.var.userId, body.data.username, consent.terms_version, consent.age],
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
  const code = newInviteCode();
  // counted and made under the member's lock: parallel requests can't make a fourth
  return locked([`invites:${c.var.userId}`], async (db) => {
    if (!c.var.isAdmin) {
      const { rows } = await db.query(`select count(*)::int as n from invites where created_by = $1 and revoked_at is null`, [c.var.userId]);
      if (rows[0].n >= MEMBER_INVITES) return err(c, 409, "invite_limit", "You've made your 3 invites. Revoke an unused one to make another.");
    }
    await db.query(`insert into invites (code, created_by) values ($1, $2)`, [code, c.var.userId]);
    return c.json({ invite: { code, link: INVITE_LINK + code } }, 201);
  });
});

app.delete("/invites/:code", async (c) => {
  const { rowCount } = await pool.query(
    `update invites set revoked_at = now() where code = $1 and created_by = $2 and used_by is null and revoked_at is null`,
    [c.req.param("code"), c.var.userId],
  );
  return rowCount ? c.json({ ok: true }) : err(c, 404, "not_found", "That invite is gone or was already used.");
});

// ── email notices ────────────────────────────────────────────────────────

const SITE = "https://tesseralauncher.com";

type Notice =
  | { kind: "friend_request"; from: string; fromId: string }
  | { kind: "friend_accepted"; from: string; fromId: string }
  | { kind: "share_added"; from: string; code: string; pack: string };

/**
 * Emails `to` about something a friend did, unless they turned that kind off, their address
 * isn't verified, or they got the same notice in the last day. Runs after the response is
 * sent; failures only get logged.
 */
function notify(to: string, n: Notice) {
  if (!mailReady) return;
  const setting: NoticeKind = n.kind === "share_added" ? "shares" : "friends";
  const ref = n.kind === "share_added" ? n.code : n.fromId;
  void (async () => {
    const { rows } = await pool.query(
      `select u.email from neon_auth."user" u join profiles p on p.user_id = u.id
       where u.id = $1 and u."emailVerified" and ${setting === "friends" ? "p.email_friends" : "p.email_shares"}`,
      [to],
    );
    if (!rows[0]) return;
    // one per person and thing a day: asking, cancelling and asking again sends one email
    const { rowCount } = await pool.query(
      `insert into email_log (user_id, kind, ref) values ($1, $2, $3)
       on conflict (user_id, kind, ref) do update set sent_at = now() where email_log.sent_at < now() - interval '1 day'`,
      [to, n.kind, ref],
    );
    if (!rowCount) return;
    const unsubscribe = unsubscribeUrl(to, setting);
    const mail =
      n.kind === "friend_request"
        ? { subject: `${n.from} wants to be friends on Tessera`, title: `${n.from} wants to be friends`, lines: [`${n.from} sent you a friend request on Tessera.`, "Open the launcher to accept or decline it. Friends can share modpacks with each other."] }
        : n.kind === "friend_accepted"
          ? { subject: `${n.from} accepted your friend request`, title: `You and ${n.from} are friends now`, lines: [`${n.from} accepted your friend request on Tessera.`, "You can share packs with each other from the launcher."] }
          : { subject: `${n.from} invited you to "${n.pack}"`, title: `${n.from} invited you to a pack`, lines: [`${n.from} wants to share their pack "${n.pack}" with you on Tessera.`, "Open the launcher to accept or decline. Once you accept, you can install it, and it stays in sync when they change it."] };
    await sendMail({ to: rows[0].email, ...mail, action: { label: "Open Tessera", url: `${SITE}/account` }, unsubscribe });
  })().catch((e) => console.error("notify", n.kind, e));
}

/** Accounts already written to verified_users by this instance. */
const knownVerified = new Set<string>();

/** Notes that this account's email was verified (once is enough): /email/change won't touch it again. */
function rememberVerified(userId: string) {
  if (knownVerified.has(userId)) return;
  pool
    .query(`insert into verified_users (user_id) values ($1) on conflict do nothing`, [userId])
    .then(() => knownVerified.size < 100_000 && knownVerified.add(userId))
    .catch((e) => console.error("rememberVerified", e));
}

/** "You're verified": once per address, the first time /me sees it verified. Runs in the background. */
function verifiedMail(userId: string, email: string) {
  if (!mailReady) return;
  void (async () => {
    const { rowCount } = await pool.query(
      `insert into email_log (user_id, kind, ref) values ($1, 'email_verified', $2) on conflict do nothing`,
      [userId, email.toLowerCase()],
    );
    if (!rowCount) return;
    const { rows } = await pool.query(`select two_factor_email as on from profiles where user_id = $1`, [userId]);
    await sendMail({
      to: email,
      subject: "Your Tessera email is verified",
      title: "You're verified",
      lines: [
        `${email} is now the verified email of your Tessera account.`,
        rows[0]?.on
          ? "Two-step sign-in is on: when you sign in on a new computer, we email you a code here first."
          : "Sign-in codes and password resets come to this address.",
      ],
      action: { label: "Open Tessera", url: `${SITE}/account` },
      after: ["If you didn't make a Tessera account, write to privacy@tesseralauncher.com and we'll look into it."],
    });
  })().catch((e) => console.error("verifiedMail", e));
}

app.get("/me/email", async (c) => {
  const { rows } = await pool.query(
    `select p.email_friends as friends, p.email_shares as shares, u."emailVerified" as verified
     from profiles p join neon_auth."user" u on u.id = p.user_id where p.user_id = $1`,
    [c.var.userId],
  );
  return rows[0] ? c.json({ email: { ...rows[0], available: mailReady } }) : err(c, 404, "not_found");
});

app.put("/me/email", async (c) => {
  const body = z.object({ friends: z.boolean().nullish(), shares: z.boolean().nullish() }).safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "invalid_settings");
  const { rows } = await pool.query(
    `update profiles set email_friends = coalesce($2, email_friends), email_shares = coalesce($3, email_shares)
     where user_id = $1 returning email_friends as friends, email_shares as shares`,
    [c.var.userId, body.data.friends ?? null, body.data.shares ?? null],
  );
  return rows[0] ? c.json({ email: rows[0] }) : err(c, 404, "not_found");
});

// ── two-step sign-in by email ────────────────────────────────────────────

const CODE_MINUTES = 10;
const CODE_ATTEMPTS = 5;
const DEVICE_DAYS = 30;
const MAX_DEVICES = 20;
const codePurpose = z.enum(["sign_in", "enable", "disable", "delete_account"]);
type CodePurpose = z.infer<typeof codePurpose>;
const codeHash = (userId: string, purpose: CodePurpose, code: string) => sha256(`${userId}:${purpose}:${code}`);

const CODE_MAIL: Record<CodePurpose, { subject: string; title: string; line: string }> = {
  sign_in: { subject: "Your Tessera sign-in code", title: "Finish signing in", line: "Someone (hopefully you) is signing in to your Tessera account. Enter this code to finish:" },
  enable: { subject: "Turn on two-step sign-in", title: "Turn on two-step sign-in", line: "Enter this code in Tessera to turn on two-step sign-in by email:" },
  disable: { subject: "Turn off two-step sign-in", title: "Turn off two-step sign-in", line: "Enter this code in Tessera to turn off two-step sign-in:" },
  delete_account: {
    subject: "Delete your Tessera account",
    title: "Delete your account",
    line: "Someone (hopefully you) is deleting your Tessera account, with its shared packs, friends and invites. Enter this code to confirm:",
  },
};

app.post("/me/2fa/code", async (c) => {
  const body = z.object({ purpose: codePurpose }).safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "invalid_purpose");
  const purpose = body.data.purpose;
  if (c.var.twoFactorPending && purpose !== "sign_in") return err(c, 403, "two_factor_required", "Enter the code we emailed you to finish signing in.");
  if (!mailReady) return err(c, 503, "email_unavailable", "Tessera can't send email right now. Try again later.");
  const { rows } = await pool.query(
    `select u.email, p.two_factor_email as on,
       (select sent_at > now() - interval '1 minute' from email_codes where user_id = u.id and purpose = $2) as recent
     from neon_auth."user" u join profiles p on p.user_id = u.id where u.id = $1`,
    [c.var.userId, purpose],
  );
  const me = rows[0];
  if (!me) return err(c, 403, "invite_required", "Join with an invite first.");
  if (purpose === "enable" && me.on) return err(c, 409, "already_on", "Two-step sign-in is already on.");
  if (purpose !== "enable" && !me.on) return err(c, 409, "not_on", "Two-step sign-in is off.");
  if (me.recent || !(await dbLimit(`codes:${c.var.userId}`, 5, 60 * 60_000))) return err(c, 429, "rate_limited", "We just sent you a code. Wait a minute before asking for another.");
  const code = String(randomInt(1_000_000)).padStart(6, "0");
  await pool.query(
    `insert into email_codes (user_id, purpose, code_hash, expires_at)
     values ($1, $2, $3, now() + make_interval(mins => $4))
     on conflict (user_id, purpose) do update set code_hash = excluded.code_hash, sent_at = now(), expires_at = excluded.expires_at, attempts = 0`,
    [c.var.userId, purpose, codeHash(c.var.userId, purpose, code), CODE_MINUTES],
  );
  const text = CODE_MAIL[purpose];
  const sent = await sendMail({
    to: me.email,
    subject: text.subject,
    title: text.title,
    lines: [text.line],
    code,
    after: [`It works for ${CODE_MINUTES} minutes. If you didn't ask for it, change your password: someone knows it.`],
  });
  if (!sent) {
    await pool.query(`delete from email_codes where user_id = $1 and purpose = $2`, [c.var.userId, purpose]);
    return err(c, 503, "email_unavailable", "We couldn't send the email. Try again in a moment.");
  }
  return c.json({ ok: true, expires_in: CODE_MINUTES * 60 });
});

/** Uses up the caller's code for `purpose`: true if it matched. */
async function takeCode(userId: string, purpose: CodePurpose, code: string): Promise<boolean | "expired" | "locked"> {
  const { rows } = await pool.query(
    `update email_codes set attempts = attempts + 1 where user_id = $1 and purpose = $2
     returning code_hash, attempts, expires_at > now() as fresh`,
    [userId, purpose],
  );
  const row = rows[0];
  if (!row || !row.fresh) return "expired";
  if (row.attempts > CODE_ATTEMPTS) return "locked";
  if (row.code_hash !== codeHash(userId, purpose, code)) return false;
  await pool.query(`delete from email_codes where user_id = $1 and purpose = $2`, [userId, purpose]);
  return true;
}

function codeError(c: Context, result: false | "expired" | "locked") {
  if (result === "expired") return err(c, 400, "code_expired", "That code ran out. Ask for a new one.");
  if (result === "locked") return err(c, 429, "too_many_attempts", "Too many wrong codes. Ask for a new one.");
  return err(c, 400, "wrong_code", "That code isn't right.");
}

const codeBody = z.object({ code: z.string().trim().regex(/^\d{6}$/), label: z.string().trim().max(60).optional() });

/** Trusts this device for 30 days; the token goes back to it once, only its hash stays here. */
async function trustDevice(userId: string, label: string | undefined): Promise<string> {
  const token = randomBytes(32).toString("base64url");
  await pool.query(
    `insert into trusted_devices (user_id, token_hash, label, expires_at) values ($1, $2, $3, now() + make_interval(days => $4))`,
    [userId, sha256(token), label || null, DEVICE_DAYS],
  );
  // the oldest ones go once there are too many
  await pool.query(
    `delete from trusted_devices where user_id = $1 and (expires_at <= now() or id not in
       (select id from trusted_devices where user_id = $1 order by created_at desc limit ${MAX_DEVICES}))`,
    [userId],
  );
  return token;
}

app.post("/me/2fa/verify", async (c) => {
  const body = codeBody.safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "wrong_code", "The code is 6 digits.");
  const ok = await takeCode(c.var.userId, "sign_in", body.data.code);
  if (ok !== true) return codeError(c, ok);
  return c.json({ device_token: await trustDevice(c.var.userId, body.data.label), expires_in_days: DEVICE_DAYS });
});

app.post("/me/2fa/enable", async (c) => {
  const body = codeBody.safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "wrong_code", "The code is 6 digits.");
  const ok = await takeCode(c.var.userId, "enable", body.data.code);
  if (ok !== true) return codeError(c, ok);
  await pool.query(`update profiles set two_factor_email = true, two_factor_set_at = now() where user_id = $1`, [c.var.userId]);
  // this device stays signed in
  return c.json({ device_token: await trustDevice(c.var.userId, body.data.label), expires_in_days: DEVICE_DAYS });
});

app.post("/me/2fa/disable", async (c) => {
  const body = codeBody.safeParse(await c.req.json().catch(() => null));
  if (!body.success) return err(c, 400, "wrong_code", "The code is 6 digits.");
  const ok = await takeCode(c.var.userId, "disable", body.data.code);
  if (ok !== true) return codeError(c, ok);
  await pool.query(`update profiles set two_factor_email = false, two_factor_set_at = now() where user_id = $1`, [c.var.userId]);
  await pool.query(`delete from trusted_devices where user_id = $1`, [c.var.userId]);
  return c.json({ ok: true });
});

// A verified email turns two-step sign-in on, unless the member already chose. Off means the
// password alone gets in anyway, so trusting the caller's device gives nothing away.
app.post("/me/2fa/auto", async (c) => {
  const body = z.object({ label: z.string().trim().max(60).optional() }).safeParse(await c.req.json().catch(() => ({})));
  const { rows } = await pool.query(
    `update profiles p set two_factor_email = true, two_factor_set_at = now()
     from neon_auth."user" u
     where p.user_id = $1 and u.id = p.user_id and u."emailVerified" and p.two_factor_set_at is null and not p.two_factor_email
     returning 1`,
    [c.var.userId],
  );
  if (!rows[0]) return c.json({ ok: true, turned_on: false });
  const token = await trustDevice(c.var.userId, body.success ? body.data.label : undefined);
  return c.json({ ok: true, turned_on: true, device_token: token, expires_in_days: DEVICE_DAYS });
});

/** "Tessera Launcher 0.6.22 on Windows", "Chrome on macOS", "tesseralauncher.com". */
function deviceName(ua: string | null): string {
  if (!ua) return "Unknown device";
  const launcher = ua.match(/^tessera-launcher\/([\w.-]+)(?: \((macOS|Windows|Linux)[;)])?/);
  if (launcher) return `Tessera Launcher ${launcher[1]}${launcher[2] ? ` on ${launcher[2]}` : ""}`;
  if (/^node$|undici|^tessera-website/i.test(ua)) return "tesseralauncher.com";
  const os = /Windows/.test(ua) ? "Windows" : /iPhone|iPad/.test(ua) ? "iOS" : /Mac OS X|Macintosh/.test(ua) ? "macOS" : /Android/.test(ua) ? "Android" : /Linux/.test(ua) ? "Linux" : null;
  const browser = /Edg\//.test(ua) ? "Edge" : /OPR\//.test(ua) ? "Opera" : /Firefox\//.test(ua) ? "Firefox" : /Chrome\//.test(ua) ? "Chrome" : /Safari\//.test(ua) ? "Safari" : null;
  if (browser) return `${browser}${os ? ` on ${os}` : ""} (tesseralauncher.com)`;
  return ua.slice(0, 60);
}

/** The caller's own session, from X-Tessera-Session (sha256 hex of the session token). */
const sessionHash = (c: Context) => (c.req.header("x-tessera-session") ?? "").toLowerCase().replace(/[^0-9a-f]/g, "").slice(0, 64);

app.get("/me/sessions", async (c) => {
  const { rows } = await pool.query(
    `select id, "userAgent" as ua, "createdAt" as created_at, "updatedAt" as last_active_at, "expiresAt" as expires_at,
       encode(sha256(convert_to(token, 'UTF8')), 'hex') = $2 as current
     from neon_auth.session where "userId" = $1 and "expiresAt" > now() order by "updatedAt" desc`,
    [c.var.userId, sessionHash(c)],
  );
  return c.json({
    sessions: rows.map(({ ua, ...s }) => ({ ...s, device: deviceName(ua) })),
  });
});

app.delete("/me/sessions/:id", async (c) => {
  const id = c.req.param("id");
  if (!UUID_RE.test(id)) return err(c, 404, "not_found");
  const { rowCount } = await pool.query(`delete from neon_auth.session where id = $1 and "userId" = $2`, [id, c.var.userId]);
  return rowCount ? c.json({ ok: true }) : err(c, 404, "not_found", "That device is already signed out.");
});

app.delete("/me/sessions", async (c) => {
  const mine = sessionHash(c);
  // without knowing which one is the caller's, this would sign it out too
  if (mine.length !== 64) return err(c, 400, "session_unknown", "Update Tessera to sign out other devices.");
  const { rowCount } = await pool.query(
    `delete from neon_auth.session where "userId" = $1 and encode(sha256(convert_to(token, 'UTF8')), 'hex') <> $2`,
    [c.var.userId, mine],
  );
  return c.json({ ok: true, removed: rowCount ?? 0 });
});

app.get("/me/2fa/devices", async (c) => {
  const { rows } = await pool.query(
    `select id, label, created_at, expires_at, id = $2 as current from trusted_devices
     where user_id = $1 and expires_at > now() order by created_at desc`,
    [c.var.userId, c.var.deviceId],
  );
  return c.json({ devices: rows });
});

app.delete("/me/2fa/devices", async (c) => {
  const { rowCount } = await pool.query(`delete from trusted_devices where user_id = $1 and id is distinct from $2`, [c.var.userId, c.var.deviceId]);
  return c.json({ ok: true, removed: rowCount });
});

// ── friends ──────────────────────────────────────────────────────────────

const MAX_FRIENDS = 200;

async function usernameOf(userId: string): Promise<string> {
  const { rows } = await pool.query(`select username from profiles where user_id = $1`, [userId]);
  return rows[0]?.username ?? "Someone";
}
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
  if (accepted.rowCount) {
    notify(other.user_id, { kind: "friend_accepted", from: await usernameOf(c.var.userId), fromId: c.var.userId });
    return c.json({ friend: { ...other, status: "friend" } });
  }
  if (!(await mayRequestFriendship(c.var.userId, other.user_id))) {
    return err(c, 403, "not_accepting", `${other.username} doesn't accept friend requests from you.`);
  }
  const { rows: made } = await pool.query(
    `insert into friendships (requester_id, addressee_id) values ($1, $2)
     on conflict (requester_id, addressee_id) do update set requester_id = excluded.requester_id
     returning case when status = 'accepted' then 'friend' else 'outgoing' end as status`,
    [c.var.userId, other.user_id],
  );
  if (made[0].status === "outgoing") notify(other.user_id, { kind: "friend_request", from: await usernameOf(c.var.userId), fromId: c.var.userId });
  return c.json({ friend: { ...other, status: made[0].status } }, 201);
});

app.post("/friends/:userId/accept", async (c) => {
  if (!UUID.test(c.req.param("userId"))) return err(c, 404, "not_found");
  const { rowCount } = await pool.query(
    `update friendships set status = 'accepted', accepted_at = now() where requester_id = $2 and addressee_id = $1 and status = 'pending'`,
    [c.var.userId, c.req.param("userId")],
  );
  if (!rowCount) return err(c, 404, "not_found", "That friend request is gone.");
  notify(c.req.param("userId"), { kind: "friend_accepted", from: await usernameOf(c.var.userId), fromId: c.var.userId });
  return c.json({ ok: true });
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
         select case when $2 then 'share_invite' else 'share_added' end, s.owner_id, p.username, s.code, s.name, m.added_at
         from share_members m join shares s on s.code = m.code join profiles p on p.user_id = s.owner_id
         where m.user_id = $1 and m.accepted_at is null
       ) n order by at desc limit 30`,
      [c.var.userId, answersInvites(c)],
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
