import { reactive } from "vue";
import { api, type Me } from "./api";

/**
 * The signed-in Tessera account (null = signed out), shared by the top bar, Home, Settings
 * and Share. `dialog` opens the sign-in dialog from anywhere.
 */
export const account = reactive<{ me: Me | null; loaded: boolean; dialog: boolean }>({ me: null, loaded: false, dialog: false });

export async function loadAccount() {
  try {
    account.me = await api.account.status();
  } finally {
    account.loaded = true;
  }
}

export async function signOut() {
  await api.account.signOut();
  account.me = null;
}
