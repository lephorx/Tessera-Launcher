import { reactive } from "vue";
import { api, type MicrosoftAccount, type Settings } from "./api";

/**
 * Minecraft accounts, shared by the top bar, Home, Skins and Settings: offline names and
 * Microsoft accounts. `microsoft` is the id of the Microsoft account playing, if one is.
 */
export const player = reactive<{ name: string; players: string[]; accounts: MicrosoftAccount[]; microsoft: string | null }>({
  name: "Player",
  players: [],
  accounts: [],
  microsoft: null,
});

function show(s: Settings) {
  player.name = s.player_name;
  player.players = s.players;
  player.microsoft = s.microsoft;
}

export async function loadPlayers() {
  const [s, accounts] = await Promise.all([api.settings.get(), api.microsoft.list()]);
  player.accounts = accounts;
  show(s);
}

/** Saves who plays and the offline names, starting from the latest settings on disk. */
async function save(name: string, players: string[], microsoft: string | null) {
  show(await api.settings.save({ ...(await api.settings.get()), player_name: name, players, microsoft }));
}

export const usePlayer = (name: string) => save(name, player.players, null);

export const useMicrosoft = (a: MicrosoftAccount) => save(a.name, player.players, a.id);

export const addPlayer = (name: string) => save(name, [...player.players, name], null);

export async function removePlayer(name: string) {
  const rest = player.players.filter((p) => p !== name);
  if (!rest.length && !player.accounts.length) return;
  if (player.microsoft || player.name !== name) return save(player.name, rest, player.microsoft);
  if (rest.length) return save(rest[0]!, rest, null);
  await save(player.accounts[0]!.name, rest, player.accounts[0]!.id);
}

/** Microsoft sign-in in its own window; the account plays afterwards. False when closed. */
export async function signInMicrosoft(): Promise<boolean> {
  const a = await api.microsoft.signIn();
  if (!a) return false;
  await loadPlayers();
  return true;
}

export async function removeMicrosoft(a: MicrosoftAccount) {
  show(await api.microsoft.remove(a.id));
  player.accounts = await api.microsoft.list();
}

/** The player's face from their public skin; offline names that aren't real accounts get Steve. */
export const headUrl = (name: string) => `https://mc-heads.net/avatar/${encodeURIComponent(name)}/64`;
