import type { Request } from "@post-tester/bindings";
import { useEffect, useState } from "react";
import { api } from "../../api/client";
import { errorText } from "../../api/http";
import type { CodegenResult, PreparedView } from "../../api/types";
import { appStore } from "../../state/app";
import { Modal } from "../../ui/Modal";
import { CopyButton, HeaderTable, MethodBadge, Spinner } from "../../ui/widgets";

function useLoad<T>(load: () => Promise<T>): { data: T | null; error: string | null } {
  const [state, setState] = useState<{ data: T | null; error: string | null }>({ data: null, error: null });
  useEffect(() => {
    let active = true;
    load()
      .then((data) => active && setState({ data, error: null }))
      .catch((e: unknown) => active && setState({ data: null, error: errorText(e) }));
    return () => {
      active = false;
    };
    // Loaded once when the dialog opens.
  }, []);
  return state;
}

function sendBody(request: Request, requestId: string | null) {
  return { request, ...(requestId ? { requestId } : {}), environment: appStore.get().environment };
}

/** "Copy as code": the request rendered by a code generator (variables resolved, secrets masked). */
export function CodegenDialog({ request, requestId, target, onClose }: { request: Request; requestId: string | null; target: string; onClose: () => void }) {
  const { data, error } = useLoad<CodegenResult>(() => api.codegen({ ...sendBody(request, requestId), target }));
  return (
    <Modal title={`Code: ${target}`} onClose={onClose} width={760} footer={data && <CopyButton text={data.code} label="Copy code" small={false} />}>
      {error && <p className="error-box">{error}</p>}
      {!data && !error && <Spinner label="Generating…" />}
      {data && <pre className="code-block">{data.code}</pre>}
    </Modal>
  );
}

/** What would be sent, after variables, auth and folder settings (secrets shown as •••). */
export function PreviewDialog({ request, requestId, onClose }: { request: Request; requestId: string | null; onClose: () => void }) {
  const { data, error } = useLoad<PreparedView>(() => api.preview(sendBody(request, requestId)));
  return (
    <Modal title="Preview" onClose={onClose} width={760}>
      {error && <p className="error-box">{error}</p>}
      {!data && !error && <Spinner label="Preparing…" />}
      {data && (
        <div className="stack">
          <p className="hint">Variables, folder settings and auth are applied. Secrets are shown as •••. Scripts run only when the request is sent.</p>
          <div className="row mono">
            <MethodBadge method={data.method} />
            <span style={{ wordBreak: "break-all" }}>{data.url}</span>
          </div>
          <h3 className="section-title">Headers</h3>
          <HeaderTable headers={data.headers} />
          <h3 className="section-title">Body</h3>
          {data.body ? <pre className="code-block">{data.body}</pre> : <p className="muted">No body.</p>}
        </div>
      )}
    </Modal>
  );
}
