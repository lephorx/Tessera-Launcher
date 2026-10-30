/**
 * Tessera's instance icons: an isometric 8×8 pixel block on a gradient. All drawn here in
 * code (no image assets), so they stay crisp at every size and cost nothing to ship.
 */

import type { Icon } from "./api";

export interface Background {
  id: string;
  from: string;
  to: string;
}

export const BACKGROUNDS: Background[] = [
  { id: "rose", from: "#e5577f", to: "#f08a6c" },
  { id: "orange", from: "#f08a3c", to: "#f6c25b" },
  { id: "yellow", from: "#f2c94c", to: "#fbe68a" },
  { id: "lime", from: "#8fd14f", to: "#d4ef7a" },
  { id: "green", from: "#2f9e5b", to: "#6fcf7f" },
  { id: "teal", from: "#1f9e9a", to: "#5fd3c4" },
  { id: "sky", from: "#3a8ee8", to: "#7cc3f5" },
  { id: "indigo", from: "#4b4be0", to: "#8a8aff" },
  { id: "violet", from: "#8b5cf6", to: "#c49bff" },
  { id: "pink", from: "#d6489c", to: "#f59ac7" },
  { id: "slate", from: "#3a3f4b", to: "#6b7280" },
  { id: "night", from: "#151826", to: "#3b3f6b" },
];

type Rand = () => number;
/** Colour of pixel (x, y) on a face, 0..7 each. */
type Paint = (x: number, y: number, r: Rand) => string;

export interface Symbol {
  id: string;
  name: string;
  top: Paint;
  left: Paint;
  right?: Paint; // defaults to `left`
}

const pick = (colors: string[]): Paint => (_x, _y, r) => colors[Math.floor(r() * colors.length)]!;
const speckle = (base: string[], dots: string[], amount: number): Paint => (x, y, r) =>
  r() < amount ? dots[Math.floor(r() * dots.length)]! : pick(base)(x, y, r);

const DIRT = ["#8b5a2b", "#7a4e25", "#96653a", "#6e4520"];
const GRASS = ["#5fa83a", "#6cb845", "#56993a", "#4f8f30"];
const STONE = ["#8a8a8a", "#7d7d7d", "#949494", "#737373"];
const PLANKS = ["#b8894f", "#a87b45", "#c2945a"];

function face(rows: string[], palette: Record<string, string>): Paint {
  return (x, y, r) => {
    const ch = rows[y]?.[x] ?? ".";
    const c = palette[ch];
    if (c) return c;
    return palette["."] ? palette["."] : pick(STONE)(x, y, r);
  };
}

export const SYMBOLS: Symbol[] = [
  {
    id: "grass",
    name: "Grass block",
    top: pick(GRASS),
    left: (x, y, r) => (y < 2 || (y === 2 && r() < 0.5) ? pick(GRASS)(x, y, r) : pick(DIRT)(x, y, r)),
  },
  { id: "dirt", name: "Dirt", top: pick(DIRT), left: pick(DIRT) },
  { id: "stone", name: "Stone", top: pick(STONE), left: pick(STONE) },
  {
    id: "cobble",
    name: "Cobblestone",
    top: speckle(STONE, ["#5a5a5a", "#a3a3a3"], 0.35),
    left: speckle(STONE, ["#5a5a5a", "#a3a3a3"], 0.35),
  },
  {
    id: "log",
    name: "Oak log",
    top: (x, y) => {
      const d = Math.max(Math.abs(x - 3.5), Math.abs(y - 3.5));
      return d > 3 ? "#6b4f2a" : d > 2 ? "#b8894f" : d > 1 ? "#a87b45" : "#c2945a";
    },
    left: (x, _y, r) => (x % 3 === 0 ? "#4f3a1f" : r() < 0.3 ? "#5c4524" : "#6b4f2a"),
  },
  { id: "planks", name: "Planks", top: pick(PLANKS), left: (x, y, r) => (y % 4 === 3 ? "#8a6236" : pick(PLANKS)(x, y, r)) },
  { id: "sand", name: "Sand", top: pick(["#e3d49a", "#dccb8a", "#ebdca8"]), left: pick(["#d8c888", "#dccb8a", "#e3d49a"]) },
  { id: "snow", name: "Snow", top: pick(["#f4f8fb", "#e9f0f5", "#ffffff"]), left: (x, y, r) => (y < 2 ? "#eef4f8" : pick(DIRT)(x, y, r)) },
  { id: "ice", name: "Ice", top: pick(["#9cc6f5", "#a9d0fa", "#8db8ea"]), left: (x, y, r) => (x === y || x === y + 4 ? "#d6eaff" : pick(["#8db8ea", "#9cc6f5"])(x, y, r)) },
  { id: "diamond", name: "Diamond ore", top: speckle(STONE, ["#5ee6e0", "#3fc9c4"], 0.18), left: speckle(STONE, ["#5ee6e0", "#3fc9c4"], 0.18) },
  { id: "gold", name: "Gold ore", top: speckle(STONE, ["#f6d63b", "#e2b828"], 0.18), left: speckle(STONE, ["#f6d63b", "#e2b828"], 0.18) },
  { id: "emerald", name: "Emerald ore", top: speckle(STONE, ["#2fd36b", "#16a34a"], 0.16), left: speckle(STONE, ["#2fd36b", "#16a34a"], 0.16) },
  { id: "redstone", name: "Redstone ore", top: speckle(STONE, ["#e23b3b", "#b91c1c"], 0.18), left: speckle(STONE, ["#e23b3b", "#b91c1c"], 0.18) },
  {
    id: "tnt",
    name: "TNT",
    top: (x, y) => ((x === 3 || x === 4) && (y === 3 || y === 4) ? "#2b2b2b" : "#c9c9c9"),
    left: face(["rrrrrrrr", "rrrrrrrr", "wwwwwwww", "wkwkkwkw", "wkwkwkkw", "wwwwwwww", "rrrrrrrr", "rrrrrrrr"], {
      r: "#d23b2b", w: "#eeeeee", k: "#2b2b2b",
    }),
  },
  {
    id: "pumpkin",
    name: "Carved pumpkin",
    top: (x, y) => (x === 3 && y === 3 ? "#5a7a2a" : x % 2 === 0 ? "#d9822b" : "#e3932f"),
    left: (x) => (x % 2 === 0 ? "#d9822b" : "#e3932f"),
    right: face(["oooooooo", "oooooooo", "okkooKko", "okkooKko", "oooooooo", "okkkkkko", "ooKkkKoo", "oooooooo"], {
      o: "#e08a2c", k: "#3a1d07", K: "#f7d24a",
    }),
  },
  {
    id: "melon",
    name: "Melon",
    top: (x, y) => (x === 3 && y === 3 ? "#7a5a2a" : (x + y) % 3 === 0 ? "#6db33f" : "#4f9a2f"),
    left: (x) => (x % 3 === 0 ? "#86c94f" : "#4f9a2f"),
  },
  {
    id: "crafting",
    name: "Crafting table",
    top: (x, y) => (x === 0 || y === 0 || x === 7 || y === 7 ? "#6b4f2a" : x === 4 || y === 4 ? "#8a6236" : "#b8894f"),
    left: face(["pppppppp", "kkkkkkkk", "p.kpp.kp", "p.kpp.kp", "pkkppkkp", "pppppppp", "pppppppp", "pppppppp"], {
      p: "#a87b45", k: "#6b4f2a", ".": "#8a8a8a",
    }),
  },
  {
    id: "bookshelf",
    name: "Bookshelf",
    top: pick(PLANKS),
    left: (x, y, r) => (y === 0 || y === 7 || y === 3 ? "#a87b45" : ["#c0392b", "#2e6fcf", "#2f9e5b", "#d9a33a", "#7d4fc9"][Math.floor(r() * 5)]!),
  },
  { id: "obsidian", name: "Obsidian", top: speckle(["#1d1530", "#160f26"], ["#4a2f7a", "#2f1f4f"], 0.25), left: speckle(["#1d1530", "#160f26"], ["#4a2f7a", "#2f1f4f"], 0.25) },
  { id: "netherrack", name: "Netherrack", top: pick(["#7a2e2e", "#6b2626", "#8a3636", "#5c2020"]), left: pick(["#7a2e2e", "#6b2626", "#8a3636", "#5c2020"]) },
  { id: "glowstone", name: "Glowstone", top: pick(["#f7d27a", "#e8b84f", "#fbe6a2", "#c9963a"]), left: pick(["#f7d27a", "#e8b84f", "#fbe6a2", "#c9963a"]) },
  {
    id: "slime",
    name: "Slime block",
    top: (x, y) => (x > 1 && x < 6 && y > 1 && y < 6 ? "#5fbf4a" : "#8ad66f"),
    left: (x, y) => (x > 1 && x < 6 && y > 1 && y < 6 ? "#4fa83c" : "#79c45f"),
  },
  { id: "amethyst", name: "Amethyst", top: pick(["#9a6fe0", "#b48af2", "#7d55c7", "#c9a8ff"]), left: pick(["#8a5fd0", "#a77ce8", "#6f48b5"]) },
  {
    id: "creeper",
    name: "Creeper",
    top: pick(["#5fbf4a", "#4fa83c", "#6fcf5a"]),
    left: pick(["#5fbf4a", "#4fa83c", "#6fcf5a"]),
    right: face(["gggggggg", "gkkggkkg", "gkkggkkg", "gggkkggg", "ggkkkkgg", "ggkkkkgg", "ggkggkgg", "gggggggg"], {
      g: "#5fbf4a", k: "#1d2b18",
    }),
  },
  { id: "magma", name: "Magma", top: speckle(["#4a1f12", "#3a170d"], ["#ff7a1a", "#ffb02e"], 0.3), left: speckle(["#4a1f12", "#3a170d"], ["#ff7a1a", "#ffb02e"], 0.3) },
  { id: "endstone", name: "End stone", top: speckle(["#e8e6a8", "#dcd99a"], ["#c9c582"], 0.2), left: speckle(["#e0dd9f", "#d4d090"], ["#bfba78"], 0.2) },
];

/** Deterministic random per symbol, so a block looks the same every time. */
function seeded(seed: string): Rand {
  let h = 2166136261;
  for (const ch of seed) h = Math.imul(h ^ ch.charCodeAt(0), 16777619);
  return () => {
    h = Math.imul(h ^ (h >>> 15), 2246822507);
    h = Math.imul(h ^ (h >>> 13), 3266489909);
    return ((h ^= h >>> 16) >>> 0) / 4294967296;
  };
}

function shade(hex: string, factor: number): string {
  const n = parseInt(hex.slice(1), 16);
  const ch = (v: number) => Math.round(Math.min(255, v * factor)).toString(16).padStart(2, "0");
  return `#${ch((n >> 16) & 255)}${ch((n >> 8) & 255)}${ch(n & 255)}`;
}

/** SVG markup (inner content, viewBox "-11 -11 22 22") of the isometric block. */
export function blockSvg(symbolId: string): string {
  const sym = SYMBOLS.find((s) => s.id === symbolId) ?? SYMBOLS[0]!;
  const w = 6.93;
  // corners: top, right, centre, left, bottom, right-bottom, left-bottom
  const T = [0, -8], R = [w, -4], C = [0, 0], L = [-w, -4], LB = [-w, 4];
  const m = (o: number[], u: number[], v: number[]) =>
    `matrix(${(u[0]! - o[0]!) / 8} ${(u[1]! - o[1]!) / 8} ${(v[0]! - o[0]!) / 8} ${(v[1]! - o[1]!) / 8} ${o[0]} ${o[1]})`;
  const pixels = (paint: Paint, factor: number, salt: string) => {
    const r = seeded(sym.id + salt);
    let out = "";
    for (let y = 0; y < 8; y++)
      for (let x = 0; x < 8; x++) out += `<rect x="${x}" y="${y}" width="1.04" height="1.04" fill="${shade(paint(x, y, r), factor)}"/>`;
    return out;
  };
  return (
    `<g transform="${m(L, T, C)}">${pixels(sym.top, 1.08, "t")}</g>` +
    `<g transform="${m(L, C, LB)}">${pixels(sym.left, 0.86, "l")}</g>` +
    `<g transform="${m(C, R, [0, 8])}">${pixels(sym.right ?? sym.left, 0.7, "r")}</g>`
  );
}

export function backgroundOf(id: string): Background {
  return BACKGROUNDS.find((b) => b.id === id) ?? BACKGROUNDS[7]!;
}

export function randomIcon(): Icon {
  const bg = BACKGROUNDS[Math.floor(Math.random() * BACKGROUNDS.length)]!;
  const sym = SYMBOLS[Math.floor(Math.random() * SYMBOLS.length)]!;
  return { kind: "generated", background: bg.id, symbol: sym.id };
}
