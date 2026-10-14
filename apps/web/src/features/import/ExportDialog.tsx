import { useEffect, useState } from "react";
import { api } from "../../api/client";
import { errorText } from "../../api/http";
import type { ExportTarget } from "../../api/types";
import { Modal } from "../../ui/Modal";
import { Spinner } from "../../ui/widgets";

/** Exports a folder with a plugin exporter and downloads the result. */
export function ExportDialog({ folder, onClose }: { folder: string; onClose: () => void }) {
  const [targets, setTargets] = useState<ExportTarget[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    api
      .exportTargets()
      .then(setTargets)
      .catch((e: unknown) => setError(errorText(e)));
  }, []);

  const run = async (target: ExportTarget) => {
    setBusy(target.id);
    setError(null);
    try {
      const { content } = await api.exportFolder(folder, target.id);
      const url = URL.createObjectURL(new Blob([content], { type: "text/plain" }));
      const link = document.createElement("a");
      link.href = url;
      link.download = `${folder.split("/").pop() || "collection"}.${target.fileExtension.replace(/^\./, "") || "txt"}`;
      link.click();
      URL.revokeObjectURL(url);
      onClose();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <Modal title={`Export ${folder || "workspace"}`} onClose={onClose} width={480}>
      <div className="stack">
        {!targets && !error && <Spinner label="Loading exporters…" />}
        {targets?.length === 0 && <p className="muted">No exporters are installed. Exporters come from plugins (see the Plugins panel and guide/plugins/README.md).</p>}
        {targets?.map((t) => (
          <button key={t.id} type="button" className="btn" onClick={() => run(t)} disabled={busy !== null}>
            {busy === t.id ? "Exporting…" : `${t.name} (.${t.fileExtension.replace(/^\./, "")})`}
          </button>
        ))}
        {error && <p className="error-box">{error}</p>}
      </div>
    </Modal>
  );
}
