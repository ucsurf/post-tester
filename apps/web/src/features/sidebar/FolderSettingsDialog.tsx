import type { FolderSettings } from "@post-tester/bindings";
import { useEffect, useState } from "react";
import { api } from "../../api/client";
import { errorText } from "../../api/http";
import { closeDialog, refreshTree, refreshVariables } from "../../state/app";
import { showToast } from "../../state/toasts";
import { KeyValueEditor } from "../../ui/KeyValueEditor";
import { Modal } from "../../ui/Modal";
import { Spinner } from "../../ui/widgets";
import { AuthEditor } from "../request-editor/AuthEditor";
import { applyForm, toForm, type FolderForm as Form } from "./folderSettings";
import "./sidebar.css";

/** Edits a folder's `_folder.yaml`: display name, description, shared headers, auth and variables. */
export function FolderSettingsDialog({ folder }: { folder: string }) {
  const [loaded, setLoaded] = useState<FolderSettings | null>(null);
  const [form, setForm] = useState<Form | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const label = folder || "all collections";

  useEffect(() => {
    let active = true;
    api
      .getFolderSettings(folder)
      .then((settings) => {
        if (!active) return;
        setLoaded(settings);
        setForm(toForm(settings));
      })
      .catch((e: unknown) => active && setLoadError(errorText(e)));
    return () => {
      active = false;
    };
  }, [folder]);

  const set = (patch: Partial<Form>) => setForm((f) => (f ? { ...f, ...patch } : f));

  const save = async () => {
    if (!loaded || !form) return;
    setSaving(true);
    setSaveError(null);
    try {
      await api.saveFolderSettings(folder, applyForm(loaded, form));
      showToast("success", `Saved settings for ${label}`);
      void refreshTree();
      void refreshVariables();
      closeDialog();
    } catch (e) {
      setSaveError(`Could not save collections/${folder ? `${folder}/` : ""}_folder.yaml: ${errorText(e)}`);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      title={`Folder settings · ${label}`}
      onClose={closeDialog}
      width={680}
      footer={
        <>
          <button type="button" className="btn" onClick={closeDialog}>
            Cancel
          </button>
          <button type="button" className="btn btn-primary" onClick={save} disabled={!form || saving}>
            {saving ? "Saving…" : "Save"}
          </button>
        </>
      }
    >
      {loadError ? (
        <p className="callout danger" role="alert">
          Could not load the folder settings: {loadError}
        </p>
      ) : !form ? (
        <Spinner label="Loading folder settings…" />
      ) : (
        <form
          className="stack folder-settings"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <p className="hint">
            Saved in <code>collections/{folder ? `${folder}/` : ""}_folder.yaml</code>. Headers, auth and variables apply to every request in this folder and
            its subfolders; a request's own values win.
          </p>
          <label className="field">
            <span>Display name</span>
            <input className="input" value={form.name} placeholder={folder.split("/").pop() || "Collections"} onChange={(e) => set({ name: e.target.value })} data-autofocus />
          </label>
          <label className="field">
            <span>Description</span>
            <textarea className="textarea" rows={2} value={form.description} onChange={(e) => set({ description: e.target.value })} />
          </label>
          <fieldset className="folder-settings-section">
            <legend>Headers</legend>
            <KeyValueEditor label="Folder headers" items={form.headers} onChange={(headers) => set({ headers })} keyPlaceholder="Header" />
          </fieldset>
          <fieldset className="folder-settings-section">
            <legend>Auth</legend>
            <AuthEditor
              auth={form.auth}
              onChange={(auth) => set({ auth })}
              inheritLabel="Inherit from the parent folder"
              inheritHint="Requests in this folder use the parent folder's auth, if any."
            />
          </fieldset>
          <fieldset className="folder-settings-section">
            <legend>Variables</legend>
            <KeyValueEditor label="Folder variables" items={form.variables} onChange={(variables) => set({ variables })} keyPlaceholder="Name" toggleable={false} />
          </fieldset>
          {saveError && (
            <p className="callout danger" role="alert">
              {saveError}
            </p>
          )}
          {/* Lets Enter in a text field submit the form. */}
          <button type="submit" hidden />
        </form>
      )}
    </Modal>
  );
}
