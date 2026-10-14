import { errorText } from "../api/http";
import { Store } from "./store";

export type ToastTone = "info" | "success" | "error";
export type Toast = { id: number; tone: ToastTone; title: string; message?: string };

export const toastStore = new Store<{ toasts: Toast[] }>({ toasts: [] });

let nextId = 1;

export function showToast(tone: ToastTone, title: string, message?: string): void {
  const id = nextId++;
  toastStore.set((s) => ({ toasts: [...s.toasts.slice(-4), { id, tone, title, ...(message ? { message } : {}) }] }));
  setTimeout(() => dismissToast(id), tone === "error" ? 9000 : 3500);
}

export function dismissToast(id: number): void {
  toastStore.set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
}

/** Shows an error toast with the server's explanation. */
export function showError(title: string, error: unknown): void {
  showToast("error", title, errorText(error));
}
