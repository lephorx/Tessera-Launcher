import { reactive } from "vue";

export const toastState = reactive<{ kind: "success" | "error" | "info"; text: string; id: number } | { id: number; text: "" ; kind: "info" }>({
  id: 0,
  text: "",
  kind: "info",
});

let timer: ReturnType<typeof setTimeout> | undefined;
function show(kind: "success" | "error" | "info", text: string) {
  Object.assign(toastState, { kind, text, id: toastState.id + 1 });
  clearTimeout(timer);
  timer = setTimeout(() => (toastState.text = ""), kind === "error" ? 6000 : 3000);
}

export const toast = {
  success: (t: string) => show("success", t),
  error: (t: string) => show("error", t),
  info: (t: string) => show("info", t),
};

export function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
