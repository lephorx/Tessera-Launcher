import type { Router } from "vue-router";
import { api } from "./api";
import { refreshNotices, FRIENDS_CHANGED } from "./notifications";
import { errorText, toast } from "./toast";

/**
 * Invites to friends' packs: accepting takes the invite and installs the pack right away
 * (if installing fails, it waits under "Shared with me" in the Library); declining says no.
 * Both refresh the bell and the Library.
 */
export async function acceptInvite(router: Router, code: string, name: string | null) {
  try {
    await api.share.accept(code);
  } catch (e) {
    toast.error(errorText(e));
    return;
  } finally {
    void refreshNotices();
    window.dispatchEvent(new Event(FRIENDS_CHANGED));
  }
  try {
    const inst = await api.share.join(code);
    toast.success(`${inst.name} is ready`);
    void router.push(`/instance/${inst.id}`);
  } catch (e) {
    toast.error(`You accepted ${name ?? "the pack"}, but installing it failed: ${errorText(e)} Try again from the Library.`);
  }
}

export async function declineInvite(code: string, name: string | null) {
  try {
    await api.share.decline(code);
    toast.info(`Declined ${name ?? "the pack"}`);
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    void refreshNotices();
    window.dispatchEvent(new Event(FRIENDS_CHANGED));
  }
}
