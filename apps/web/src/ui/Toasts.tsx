import { useStore } from "../state/store";
import { dismissToast, toastStore } from "../state/toasts";
import { Icon, IconButton } from "./Icon";
import "./overlays.css";

export function Toasts() {
  const toasts = useStore(toastStore, (s) => s.toasts);
  return (
    <div className="toasts" role="region" aria-label="Notifications" aria-live="polite">
      {toasts.map((toast) => (
        <div key={toast.id} className={`toast toast-${toast.tone}`} role={toast.tone === "error" ? "alert" : "status"}>
          <Icon name={toast.tone === "error" ? "alert" : toast.tone === "success" ? "check" : "info"} />
          <div className="grow">
            <strong>{toast.title}</strong>
            {toast.message && <p>{toast.message}</p>}
          </div>
          <IconButton icon="x" label="Dismiss" small onClick={() => dismissToast(toast.id)} />
        </div>
      ))}
    </div>
  );
}
