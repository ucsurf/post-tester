import { useRef, useState } from "react";
import { api } from "../../api/client";
import { errorText } from "../../api/http";
import type { ImportResult } from "../../api/types";
import { plural } from "../../lib/format";
import { appStore, closeDialog, openRequest, refreshEnvironments, refreshTree } from "../../state/app";
import { useStore } from "../../state/store";
import { Icon } from "../../ui/Icon";
import { Modal } from "../../ui/Modal";

/** Import cURL commands, Postman collections, OpenAPI documents or plugin formats. */
export function ImportDialog({ into }: { into?: string | undefined }) {
  const importers = useStore(appStore, (s) => s.plugins.flatMap((p) => p.capabilities.importers.map((i) => `${i.displayName} (${p.displayName})`)));
  const [text, setText] = useState("");
  const [folder, setFolder] = useState(into ?? "");
  const [fileName, setFileName] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<ImportResult | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  const pickFile = async (file: File | undefined) => {
    if (!file) return;
    setFileName(file.name);
    setText(await file.text());
  };

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const imported = await api.importText(text, folder.trim() || undefined);
      setResult(imported);
      void refreshTree();
      if (imported.environments.length > 0) void refreshEnvironments();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  if (result) {
    return (
      <Modal
        title="Import finished"
        onClose={closeDialog}
        footer={
          <button type="button" className="btn btn-primary" onClick={closeDialog}>
            Done
          </button>
        }
      >
        <div className="stack">
          <p>
            Created {plural(result.requests.length, "request")} in <code>collections/{result.folder || ""}</code>
            {result.environments.length > 0 && <> and {plural(result.environments.length, "environment")}</>}.
          </p>
          {result.requests.length > 0 && (
            <ul className="import-list">
              {result.requests.map((id) => (
                <li key={id}>
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm"
                    onClick={() => {
                      closeDialog();
                      void openRequest(id);
                    }}
                  >
                    <Icon name="link" size={12} /> {id}
                  </button>
                </li>
              ))}
            </ul>
          )}
          {result.environments.length > 0 && <p className="muted">Environments: {result.environments.join(", ")}</p>}
        </div>
      </Modal>
    );
  }

  return (
    <Modal
      title="Import"
      width={680}
      onClose={closeDialog}
      footer={
        <>
          <button type="button" className="btn" onClick={closeDialog}>
            Cancel
          </button>
          <button type="button" className="btn btn-primary" onClick={submit} disabled={busy || !text.trim()}>
            {busy ? "Importing…" : "Import"}
          </button>
        </>
      }
    >
      <div className="stack">
        <p className="muted">
          Paste a cURL command, a Postman v2.1 collection, or an OpenAPI 3 document (JSON or YAML). The format is detected automatically.
          {importers.length > 0 && <> Plugins add: {importers.join(", ")}.</>}
        </p>
        <div className="row">
          <button type="button" className="btn btn-sm" onClick={() => fileInput.current?.click()}>
            <Icon name="import" size={12} /> Choose a file…
          </button>
          <input ref={fileInput} type="file" hidden accept=".json,.yaml,.yml,.txt,.sh,.har,.postman_collection" onChange={(e) => pickFile(e.target.files?.[0])} />
          {fileName && <span className="faint truncate">{fileName}</span>}
        </div>
        <textarea
          className="textarea"
          rows={12}
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder={"curl -X POST https://api.example.com/login \\\n  -H 'Content-Type: application/json' \\\n  -d '{\"user\":\"me\"}'"}
          aria-label="Text to import"
          data-autofocus
          spellCheck={false}
        />
        <label className="field">
          <span>Into folder (optional)</span>
          <input className="input input-mono" value={folder} onChange={(e) => setFolder(e.target.value)} placeholder="e.g. imported/petstore" />
        </label>
        {error && <p className="error-box">{error}</p>}
      </div>
    </Modal>
  );
}
