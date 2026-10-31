import { reactive } from "vue";
import { api, type Notice } from "./api";
import { account } from "./account";
import { toast } from "./toast";

/**
 * The bell: friend requests, accepted requests and packs friends shared with you. One small
 * request every 30 seconds while the launcher is in front, every 2 minutes behind other
 * windows. New ones also show as a toast, and the friends list reloads when they change.
 */
export const notices = reactive<{ list: Notice[]; unread: number; loaded: boolean }>({ list: [], unread: 0, loaded: false });

/** Fired when friends or requests may have changed: the friends list reloads. */
export const FRIENDS_CHANGED = "tessera-friends-changed";

const keyOf = (n: Notice) => `${n.kind}:${n.user_id}:${n.code ?? ""}:${n.at}`;
let known: Set<string> | null = null;
let timer: ReturnType<typeof setTimeout> | undefined;
let busy = false;

export async function refreshNotices() {
  if (busy || !account.me?.member) return;
  busy = true;
  try {
    const r = await api.notifications.list();
    const keys = new Set(r.notifications.map(keyOf));
    const fresh = known ? r.notifications.filter((n) => !known!.has(keyOf(n))) : [];
    // the first load only fills the bell; later ones say what's new
    for (const n of fresh.slice(0, 3)) {
      if (n.kind === "friend_request") toast.info(`${n.username} sent you a friend request`);
      else if (n.kind === "friend_accepted") toast.success(`${n.username} accepted your friend request`);
      else toast.info(`${n.username} shared ${n.name} with you`);
    }
    const changed = !known || known.size !== keys.size || fresh.length > 0;
    known = keys;
    notices.list = r.notifications;
    notices.unread = r.unread;
    notices.loaded = true;
    if (changed) window.dispatchEvent(new Event(FRIENDS_CHANGED));
  } catch {
    // signed out or offline: the next round tries again
  } finally {
    busy = false;
  }
}

function schedule() {
  clearTimeout(timer);
  timer = setTimeout(
    async () => {
      await refreshNotices();
      schedule();
    },
    document.hasFocus() ? 30_000 : 120_000,
  );
}

let started = false;
export function startNotices() {
  if (started) return;
  started = true;
  void refreshNotices();
  schedule();
  // back in front: catch up right away
  window.addEventListener("focus", () => {
    void refreshNotices();
    schedule();
  });
}

/** Opening the bell: what's there counts as seen. */
export async function markSeen() {
  if (!notices.unread) return;
  notices.unread = 0;
  await api.notifications.seen().catch(() => {});
}

/** Signed out or another account: start over. */
export function resetNotices() {
  known = null;
  notices.list = [];
  notices.unread = 0;
  notices.loaded = false;
}
