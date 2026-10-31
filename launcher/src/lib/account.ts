import { reactive } from "vue";
import { api, type Me } from "./api";

/**
 * The signed-in Tessera account (null = signed out), shared by the top bar, Home, Settings
 * and Share. `dialog` opens the sign-in dialog from anywhere. `pending` is an account that
 * signed in with its password but still owes the two-step code: everywhere else it counts as
 * signed out (the API answers almost nothing yet), and the account card asks for the code.
 */
// consentUnsupported: the API is older than the Terms (no /me/consent yet), so don't hold members up
export const account = reactive<{ me: Me | null; pending: Me | null; loaded: boolean; dialog: boolean; consentUnsupported: boolean }>({
  me: null,
  pending: null,
  loaded: false,
  dialog: false,
  consentUnsupported: false,
});

/** Who is signed in, or who still owes a two-step code. */
export function setMe(me: Me | null) {
  account.pending = me?.two_factor_required ? me : null;
  account.me = me?.two_factor_required ? null : me;
}

/**
 * After a failed API call: if this computer's 30 days of two-step trust ran out, the account
 * card asks for a code again. Offline or anything else keeps you signed in.
 */
export async function recheckTwoStep() {
  if (!account.me?.two_factor) return;
  const me = await api.account.status().catch(() => null);
  if (me?.two_factor_required) setMe(me);
}

export async function loadAccount() {
  try {
    setMe(await api.account.status());
  } finally {
    account.loaded = true;
  }
}

export async function signOut() {
  await api.account.signOut();
  setMe(null);
}
