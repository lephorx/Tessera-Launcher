/**
 * Email from Tessera: Proton Mail SMTP submission with a token for the sending address
 * (Proton → Settings → IMAP/SMTP → SMTP tokens). Without SMTP_USER and SMTP_PASS nothing is
 * sent and `mailReady` is false; the API keeps working, only codes and notices can't go out.
 *
 *   SMTP_HOST    default smtp.protonmail.ch
 *   SMTP_PORT    default 587 (STARTTLS)
 *   SMTP_USER    the address the token belongs to, e.g. noreply@tesseralauncher.com
 *   SMTP_PASS    the SMTP token
 *   MAIL_FROM    default "Tessera <SMTP_USER>"
 *   MAIL_SECRET  signs unsubscribe links (any long random string)
 */
import { createHmac, timingSafeEqual } from "node:crypto";
import nodemailer from "nodemailer";

const user = process.env.SMTP_USER;
const pass = process.env.SMTP_PASS;
const secret = process.env.MAIL_SECRET;
const from = process.env.MAIL_FROM || (user ? `Tessera <${user}>` : "");

export const mailReady = !!(user && pass && secret);

const transport = mailReady
  ? nodemailer.createTransport({
      host: process.env.SMTP_HOST || "smtp.protonmail.ch",
      port: Number(process.env.SMTP_PORT || 587),
      secure: Number(process.env.SMTP_PORT || 587) === 465,
      requireTLS: true,
      auth: { user, pass },
      connectionTimeout: 10_000,
      greetingTimeout: 10_000,
      socketTimeout: 20_000,
    })
  : null;

/** Reserved test domains (RFC 2606/6761): test accounts never get mail, so nothing bounces. */
export function deliverable(to: string): boolean {
  const domain = to.split("@")[1]?.toLowerCase() ?? "";
  return !!domain && !/(^|\.)(example\.(com|net|org)|example|test|invalid|localhost)$/.test(domain);
}

export interface Mail {
  to: string;
  subject: string;
  /** The big line at the top of the card. */
  title: string;
  /** Plain text paragraphs; the HTML version is made from them. */
  lines: string[];
  /** A one-time code, shown large between the paragraphs and `after`. */
  code?: string;
  /** Small print under the code or button. */
  after?: string[];
  /** A button under the text. */
  action?: { label: string; url: string };
  /** One-click unsubscribe for notices (never for codes). */
  unsubscribe?: string;
}

const SITE = "https://tesseralauncher.com";
// Tessera's colours (the website's light theme)
const C = { page: "#f2f4f7", card: "#ffffff", ink: "#14171c", muted: "#5b6372", faint: "#8b93a1", primary: "#5b5bf0", tint: "#4646d6", tintBg: "#ececfd", line: "#e6e9ef" };
const FONT = `Inter,-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif`;

const escape = (s: string) => s.replace(/[&<>"']/g, (ch) => `&#${ch.charCodeAt(0)};`);

/** Tables and inline styles only: that's what Gmail, Outlook and Apple Mail all render alike. */
function html(m: Mail): string {
  const p = (l: string, style = "") => `<p style="margin:0 0 14px;${style}">${escape(l)}</p>`;
  const code = m.code
    ? `<table role="presentation" cellpadding="0" cellspacing="0" width="100%" style="margin:8px 0 22px"><tr><td align="center" style="background:${C.tintBg};border-radius:16px;padding:20px 12px">
<span style="font-family:'SF Mono',Menlo,Consolas,monospace;font-size:34px;font-weight:700;letter-spacing:10px;color:${C.tint}">${escape(m.code)}</span></td></tr></table>`
    : "";
  const button = m.action
    ? `<table role="presentation" cellpadding="0" cellspacing="0" style="margin:8px 0 22px"><tr><td style="background:${C.primary};border-radius:12px">
<a href="${escape(m.action.url)}" style="display:inline-block;padding:13px 22px;color:#ffffff;text-decoration:none;font-weight:600;font-size:15px">${escape(m.action.label)}</a></td></tr></table>`
    : "";
  const after = (m.after ?? []).map((l) => p(l, `color:${C.muted};font-size:13px`)).join("");
  const foot = m.unsubscribe
    ? `You get this because of your Tessera email settings. <a href="${escape(m.unsubscribe)}" style="color:${C.faint};text-decoration:underline">Turn these emails off</a>.`
    : `You get this because someone used your address on Tessera. If it wasn't you, you can ignore it.`;
  const preview = m.code ? `${m.code} is your code` : m.lines[0] ?? "";
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="light"><meta name="supported-color-schemes" content="light"><title>${escape(m.subject)}</title></head>
<body style="margin:0;padding:0;background:${C.page};-webkit-text-size-adjust:100%">
<div style="display:none;max-height:0;overflow:hidden;opacity:0">${escape(preview)}&#8203;&nbsp;&#8203;&nbsp;&#8203;&nbsp;&#8203;&nbsp;</div>
<table role="presentation" cellpadding="0" cellspacing="0" width="100%" style="background:${C.page}"><tr><td align="center" style="padding:32px 16px">
<table role="presentation" cellpadding="0" cellspacing="0" width="100%" style="max-width:520px;font-family:${FONT};color:${C.ink};font-size:15px;line-height:1.55">
<tr><td style="padding:0 4px 18px"><a href="${SITE}" style="text-decoration:none;color:${C.ink}">
<img src="${SITE}/email/logo.png" width="36" height="36" alt="" style="vertical-align:middle;border:0;border-radius:9px">
<span style="vertical-align:middle;font-size:18px;font-weight:800;letter-spacing:-0.2px;padding-left:8px">Tessera</span></a></td></tr>
<tr><td style="background:${C.card};border-radius:20px;border:1px solid ${C.line};padding:32px 30px 18px">
<h1 style="margin:0 0 14px;font-size:22px;line-height:1.3;font-weight:800;letter-spacing:-0.3px;color:${C.ink}">${escape(m.title)}</h1>
${m.lines.map((l) => p(l)).join("")}${code}${button}${after}</td></tr>
<tr><td style="padding:18px 6px 0;color:${C.faint};font-size:12px;line-height:1.5">${foot}<br>
<a href="${SITE}" style="color:${C.faint};text-decoration:none">Tessera Launcher</a> · modded Minecraft, together</td></tr>
</table></td></tr></table></body></html>`;
}

function text(m: Mail): string {
  const parts = [m.title, ...m.lines];
  if (m.code) parts.push(m.code);
  if (m.after) parts.push(...m.after);
  if (m.action) parts.push(`${m.action.label}: ${m.action.url}`);
  if (m.unsubscribe) parts.push(`Turn these emails off: ${m.unsubscribe}`);
  return parts.join("\n\n");
}

/** Sends one email. False when mail isn't set up, the address is a test one, or SMTP failed. */
export async function sendMail(m: Mail): Promise<boolean> {
  if (!transport || !deliverable(m.to)) return false;
  try {
    await transport.sendMail({
      from,
      to: m.to,
      subject: m.subject,
      text: text(m),
      html: html(m),
      headers: m.unsubscribe
        ? { "List-Unsubscribe": `<${m.unsubscribe}>`, "List-Unsubscribe-Post": "List-Unsubscribe=One-Click" }
        : undefined,
    });
    return true;
  } catch (e) {
    console.error("mail: sending failed", (e as Error).message);
    return false;
  }
}

// ── unsubscribe links ────────────────────────────────────────────────────

export type NoticeKind = "friends" | "shares";

function sign(userId: string, kind: NoticeKind): string {
  return createHmac("sha256", secret ?? "").update(`unsubscribe:${userId}:${kind}`).digest("base64url");
}

export function unsubscribeUrl(apiBase: string, userId: string, kind: NoticeKind): string {
  const q = new URLSearchParams({ u: userId, k: kind, t: sign(userId, kind) });
  return `${apiBase}/v1/email/unsubscribe?${q}`;
}

export function checkUnsubscribe(userId: string, kind: string, token: string): kind is NoticeKind {
  if (!secret || (kind !== "friends" && kind !== "shares")) return false;
  const want = Buffer.from(sign(userId, kind));
  const got = Buffer.from(token);
  return want.length === got.length && timingSafeEqual(want, got);
}
