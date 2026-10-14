import type { Auth, FolderSettings, KeyValue } from "@post-tester/bindings";
import { recordToRows, rowsToRecord } from "../../ui/KeyValueEditor";

/** The folder settings editor's form, and its conversion to and from `_folder.yaml`. */

export type FolderForm = { name: string; description: string; headers: KeyValue[]; auth: Auth | undefined; variables: KeyValue[] };

export function toForm(settings: FolderSettings): FolderForm {
  return {
    name: settings.name,
    description: settings.description,
    headers: settings.headers,
    auth: settings.auth,
    variables: recordToRows(settings.variables),
  };
}

/**
 * Merges the form into the settings that were loaded, so fields this editor does not
 * know about (written by a newer version or by hand) are kept.
 */
export function applyForm(loaded: FolderSettings, form: FolderForm): FolderSettings {
  const { auth: _auth, ...rest } = loaded;
  return {
    ...rest,
    name: form.name.trim(),
    description: form.description,
    headers: form.headers.filter((h) => h.key.trim() !== ""),
    variables: rowsToRecord(form.variables.filter((v) => v.key.trim() !== "")),
    ...(form.auth ? { auth: form.auth } : {}),
  };
}
