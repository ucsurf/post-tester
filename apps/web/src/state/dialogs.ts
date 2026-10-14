import { Store } from "./store";

/**
 * Promise-based confirm/prompt dialogs (styled replacements for `window.confirm`
 * and `window.prompt`). Rendered by `ui/QuestionDialog`.
 */

export type Question =
  | { type: "confirm"; title: string; message: string; confirmLabel: string; danger: boolean; resolve: (ok: boolean) => void }
  | {
      type: "prompt";
      title: string;
      message: string;
      label: string;
      initial: string;
      placeholder: string;
      confirmLabel: string;
      validate: (value: string) => string | null;
      resolve: (value: string | null) => void;
    };

export const questionStore = new Store<{ question: Question | null }>({ question: null });

export function confirmDialog(options: { title: string; message: string; confirmLabel?: string; danger?: boolean }): Promise<boolean> {
  return new Promise((resolve) => {
    questionStore.set({
      question: {
        type: "confirm",
        confirmLabel: "OK",
        danger: false,
        ...options,
        resolve: (ok) => {
          questionStore.set({ question: null });
          resolve(ok);
        },
      },
    });
  });
}

export function promptDialog(options: {
  title: string;
  message?: string;
  label: string;
  initial?: string;
  placeholder?: string;
  confirmLabel?: string;
  validate?: (value: string) => string | null;
}): Promise<string | null> {
  return new Promise((resolve) => {
    questionStore.set({
      question: {
        type: "prompt",
        message: "",
        initial: "",
        placeholder: "",
        confirmLabel: "OK",
        validate: () => null,
        ...options,
        resolve: (value) => {
          questionStore.set({ question: null });
          resolve(value);
        },
      },
    });
  });
}

/** Request ids are paths inside `collections/`, e.g. `auth/login`. */
export function validateRequestId(value: string): string | null {
  const id = value.trim();
  if (!id) return "Enter an id such as users/list.";
  if (id.startsWith("/") || id.endsWith("/")) return "The id cannot start or end with /.";
  if (id.split("/").some((part) => part === "" || part === "." || part === "..")) return "Each part of the id needs a name (no empty, . or .. parts).";
  if (/[\\:*?"<>|]/.test(id)) return 'The id cannot contain \\ : * ? " < > |';
  return null;
}
