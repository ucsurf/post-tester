import { loadWorkspace } from "../../state/app";

/** Full-page states shown before the main layout can be used. */

export function NoTokenScreen() {
  return (
    <main className="screen">
      <div className="screen-card stack">
        <img src="./favicon.svg" alt="" width={40} height={40} />
        <h1>Open the link printed by `pt serve`</h1>
        <p className="muted">
          For your safety, this page needs the session token that <code>pt serve</code> prints when it starts. Run it in your workspace and open the link it shows,
          for example:
        </p>
        <pre className="code-block">$ pt serve{"\n"}  Open http://127.0.0.1:7777/?token=…</pre>
        <p className="hint">The token is kept only for this browser tab and removed from the address bar.</p>
      </div>
    </main>
  );
}

export function ConnectionErrorScreen({ message }: { message: string }) {
  return (
    <main className="screen">
      <div className="screen-card stack">
        <h1>Cannot talk to the post-tester server</h1>
        <p className="error-box">{message}</p>
        <p className="muted">
          Make sure <code>pt serve</code> is still running. If it was restarted, open the new link it printed (the token changes on every start).
        </p>
        <div>
          <button type="button" className="btn btn-primary" onClick={() => loadWorkspace()}>
            Try again
          </button>
        </div>
      </div>
    </main>
  );
}

export function NoWorkspaceScreen() {
  return (
    <main className="screen">
      <div className="screen-card stack">
        <img src="./favicon.svg" alt="" width={40} height={40} />
        <h1>No workspace here yet</h1>
        <p className="muted">
          A workspace is a folder with your collections, environments and settings, stored as plain YAML files. Create one in the folder where you started{" "}
          <code>pt serve</code>:
        </p>
        <pre className="code-block">$ pt init{"\n"}$ pt serve</pre>
        <p className="muted">Then open the new link that is printed, or press Try again if the server is still running in that folder.</p>
        <div>
          <button type="button" className="btn btn-primary" onClick={() => loadWorkspace()}>
            Try again
          </button>
        </div>
      </div>
    </main>
  );
}

export function LoadingScreen() {
  return (
    <main className="screen">
      <span className="spinner" aria-label="Loading" />
    </main>
  );
}
