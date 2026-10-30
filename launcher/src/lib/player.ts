import { reactive } from "vue";
import { api } from "./api";

/** Minecraft accounts (offline names until Microsoft sign-in), shared by the top bar, Home and Settings. */
export const player = reactive<{ name: string; players: string[] }>({ name: "Player", players: [] });

export async function loadPlayers() {
  const s = await api.settings.get();
  player.name = s.player_name;
  player.players = s.players;
}

/** Saves the active name and the list, starting from the latest settings on disk. */
async function save(name: string, players: string[]) {
  const s = await api.settings.save({ ...(await api.settings.get()), player_name: name, players });
  player.name = s.player_name;
  player.players = s.players;
}

export const usePlayer = (name: string) => save(name, player.players);

export const addPlayer = (name: string) => save(name, [...player.players, name]);

export async function removePlayer(name: string) {
  const rest = player.players.filter((p) => p !== name);
  if (!rest.length) return;
  await save(player.name === name ? rest[0]! : player.name, rest);
}

/** The player's face from their public skin; offline names that aren't real accounts get Steve. */
export const headUrl = (name: string) => `https://mc-heads.net/avatar/${encodeURIComponent(name)}/64`;
