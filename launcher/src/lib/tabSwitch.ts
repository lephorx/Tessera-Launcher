/**
 * Which way a tab switch goes, for the panel slide (`<Transition :name="`panel-${dir}`">`,
 * CSS in app.css). Same motion as the dashboard's tabs, but done with plain CSS animations
 * on the panels instead of a view transition: a view transition draws the page from
 * snapshots, and WebKit flashes the old snapshot (old pill position) when it ends.
 */
export type PanelDirection = "forward" | "back" | "none";

export function tabDirection<T>(order: readonly T[], from: T, to: T): PanelDirection {
  const a = order.indexOf(from);
  const b = order.indexOf(to);
  if (a === -1 || b === -1 || a === b) return "none";
  return b > a ? "forward" : "back";
}
