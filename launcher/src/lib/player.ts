import { reactive } from "vue";
import { api, type MicrosoftAccount, type Settings } from "./api";
import { errorText, toast } from "./toast";

/**
 * Minecraft accounts (Microsoft accounts only), shared by the top bar, Home, Skins and
 * Settings. `microsoft` is the id of the account playing; `name` is empty without one.
 */
export const player = reactive<{ name: string; accounts: MicrosoftAccount[]; microsoft: string | null; signingIn: boolean }>({
  name: "",
  accounts: [],
  microsoft: null,
  signingIn: false,
});

function show(s: Settings) {
  player.microsoft = s.microsoft;
  player.name = s.microsoft ? s.player_name : "";
}

export async function loadPlayers() {
  const [s, accounts] = await Promise.all([api.settings.get(), api.microsoft.list()]);
  player.accounts = accounts;
  show(s);
}

export async function useMicrosoft(a: MicrosoftAccount) {
  show(await api.settings.save({ ...(await api.settings.get()), player_name: a.name, microsoft: a.id }));
}

/** Microsoft's sign-in in its own window; the account plays afterwards. False when closed. */
export async function signInMicrosoft(): Promise<boolean> {
  if (player.signingIn) return false;
  player.signingIn = true;
  try {
    const a = await api.microsoft.signIn();
    if (!a) return false;
    await loadPlayers();
    toast.success(`Signed in as ${a.name}`);
    return true;
  } catch (e) {
    toast.error(errorText(e));
    return false;
  } finally {
    player.signingIn = false;
  }
}

export async function removeMicrosoft(a: MicrosoftAccount) {
  show(await api.microsoft.remove(a.id));
  player.accounts = await api.microsoft.list();
}

/** Before playing: an account is needed, so without one the sign-in opens. */
export async function ensureAccount(): Promise<boolean> {
  if (player.microsoft) return true;
  return signInMicrosoft();
}

/** The player's face from their public skin. */
export const headUrl = (name: string) => `https://mc-heads.net/avatar/${encodeURIComponent(name)}/64`;
