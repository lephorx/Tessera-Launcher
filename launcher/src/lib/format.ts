import type { Loader } from "./api";

export const LOADERS: Record<Loader, { label: string; color: string }> = {
  vanilla: { label: "Vanilla", color: "#7dd87d" },
  fabric: { label: "Fabric", color: "#dbb69b" },
  quilt: { label: "Quilt", color: "#d6a2f0" },
  forge: { label: "Forge", color: "#959eef" },
  neoforge: { label: "NeoForge", color: "#f2a76d" },
};

export const LOADER_ORDER: Loader[] = ["vanilla", "fabric", "neoforge", "forge", "quilt"];

export function relative(iso: string | null): string {
  if (!iso) return "Never played";
  const days = (Date.now() - new Date(iso).getTime()) / 86400000;
  if (days >= 30 && days < 365) return `${Math.floor(days / 30)} month${days < 60 ? "" : "s"} ago`;
  if (days >= 365) return `${Math.floor(days / 365)} year${days < 730 ? "" : "s"} ago`;
  const s = (Date.now() - new Date(iso).getTime()) / 1000;
  if (s < 60) return "Just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  if (s < 86400 * 30) return `${Math.floor(s / 86400)} days ago`;
  return new Date(iso).toLocaleDateString();
}

export function compact(n: number): string {
  return Intl.NumberFormat(undefined, { notation: "compact" }).format(n);
}
