/**
 * What names may say: usernames (everyone sees them) and the names of shared packs (friends
 * see them). Usernames are 3–24 letters, digits and underscores, not only digits, with no
 * underscore at either end or two in a row. Neither may hold slurs, swearing, sexual words,
 * notorious people (Epstein, Diddy, Hitler…) or pretend to be Tessera staff.
 *
 * Names are checked after undoing look-alikes (d1ddy, 3pst3in, p_o_r_n): digits and symbols
 * that stand for letters become those letters and separators go. Stretched letters still
 * count (fuuuck), but a banned word's own double letters must stay double, so "boobs" doesn't
 * catch "bobsled". Words that hide inside harmless ones (ass in "class", rape in "grape",
 * pedo in "torpedo") only count as a whole word: split at underscores, digits, spaces and
 * CamelCase.
 */

const LEET: Record<string, string> = { "0": "o", "1": "i", "3": "e", "4": "a", "5": "s", "7": "t", "8": "b", "9": "g", "@": "a", $: "s", "!": "i", "|": "i", "+": "t", "€": "e" };
const unleet = (text: string) => text.replace(/[0-9@$!|+€]/g, (ch) => LEET[ch] ?? ch);

/** Banned anywhere in a name, even inside other words. */
const ANYWHERE = [
  // slurs
  "nigger", "nigga", "faggot", "fagot", "tranny", "retard", "kike", "wetback", "chingchong", "beaner",
  // swearing and sexual words
  "fuck", "fuk", "fck", "shit", "bitch", "biatch", "cunt", "whore", "slut", "pussy", "penis", "vagina", "porn", "hentai",
  "dildo", "jizz", "blowjob", "handjob", "rimjob", "boobs", "orgasm", "masturb", "wanker", "bollocks", "motherf",
  "asshole", "arsehole", "dickhead", "cumshot", "creampie", "onlyfans", "pornhub", "xvideos", "nudes",
  "pedophil", "paedophil", "molest", "incest", "bestiality", "zoophil", "necrophil",
  // hate and violence
  "hitler", "nazi", "siegheil", "swastika", "holocaust", "kukluxklan", "whitepower", "killyourself", "terrorist",
  "jihad", "genocide", "schoolshoot", "massshoot",
  // notorious people
  "epstein", "diddy", "puffdaddy", "seancombs", "goebbels", "himmler", "mussolini", "binladen", "osama",
  "charlesmanson", "tedbundy", "dahmer", "breivik", "tarrant", "unabomber", "rkelly", "ghislaine", "weinstein", "savile",
];

/** Banned as a whole word only: they hide in harmless words. */
const WHOLE_WORD = [
  "ass", "arse", "dick", "cock", "cum", "fag", "rape", "raped", "raping", "rapist", "sex", "anal", "anus", "tits", "tit",
  "spic", "chink", "coon", "gook", "hoe", "nig", "niga", "pedo", "pedos", "paedo", "kkk", "kys", "isis", "heil",
];

/** Usernames that pretend to be Tessera: these exactly, or starting with the first four. */
const RESERVED_PREFIX = ["tessera", "admin", "moderator", "official"];
const RESERVED = [...RESERVED_PREFIX, "administrator", "mod", "mods", "staff", "support", "system", "root", "owner", "developer", "dev", "help", "security"];

/** A banned word as a pattern: each letter may repeat, double letters stay at least double. */
function pattern(word: string): string {
  return (word.match(/(.)\1*/g) ?? []).map((run) => (run.length > 1 ? `${run[0]}{${run.length},}` : `${run}+`)).join("");
}

const anywhere = new RegExp(ANYWHERE.map(pattern).join("|"));
const wholeWord = new RegExp(`^(?:${WHOLE_WORD.map(pattern).join("|")})$`);

/** Lowercase letters only: look-alikes undone, accents and separators gone. */
function flat(text: string): string {
  return unleet(text.toLowerCase().normalize("NFKD").replace(/[̀-ͯ]/g, "")).replace(/[^a-z]/g, "");
}

/** The words a name is made of: split at separators, digits and CamelCase. */
function words(text: string): string[] {
  return text
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .toLowerCase()
    .split(/[^a-z]+/)
    .filter(Boolean);
}

/** True if the text has a banned word in it. */
export function offensive(text: string): boolean {
  if (anywhere.test(flat(text))) return true;
  // whole words as written (Big_Ass) and with look-alikes undone (d1ck)
  return [...words(text), ...words(unleet(text))].some((w) => wholeWord.test(w));
}

/** Why a username can't be used, or null when it's fine. */
export function usernameProblem(name: string): string | null {
  if (!/^[A-Za-z0-9_]{3,24}$/.test(name)) return "Usernames are 3–24 letters, digits or underscores. No spaces or other symbols.";
  if (/^\d+$/.test(name)) return "A username needs at least one letter.";
  if (/^_|_$|__/.test(name)) return "Underscores go between letters or digits, one at a time.";
  const lower = name.toLowerCase();
  const letters = flat(name);
  if (RESERVED.includes(lower) || RESERVED_PREFIX.some((r) => letters.startsWith(r))) return "That username is reserved. Pick another one.";
  if (offensive(name)) return "That username isn't allowed. Pick another one.";
  return null;
}
