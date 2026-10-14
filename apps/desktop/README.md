# post-tester desktop and mobile app

A native app built with [Tauri 2](https://v2.tauri.app). It starts the same
local server as `pt serve` inside the app, on a random port with a fresh
session token, and shows the web UI in a native window. Browser, desktop and
mobile all use one engine and one UI.

- On first start it opens (or creates) a workspace in `Documents/post-tester`.
- **Workspace → Open Workspace…** (`Cmd/Ctrl+O`) opens another folder; an empty
  folder becomes a new workspace.
- Secrets are stored in the OS keychain.

## Run from source

Build the web UI first, because the server embeds it:

```bash
pnpm install
pnpm --filter web build
cargo run -p post-tester-desktop
```

## Build installers

Install the Tauri CLI once (`cargo install tauri-cli --version "^2"`), then:

```bash
cd apps/desktop/src-tauri
cargo tauri build
```

This produces `.dmg`/`.app` on macOS, `.msi`/`.exe` on Windows and
`.deb`/`.AppImage`/`.rpm` on Linux in `target/release/bundle/`. Signed and
notarized releases need your own certificates; see the Tauri distribution guide.

## Mobile (iOS and Android)

The app code is mobile-ready (`tauri::mobile_entry_point`, a library target
for iOS/Android). Generating the native projects needs Xcode (iOS) or Android
Studio with the NDK (Android):

```bash
cd apps/desktop/src-tauri
cargo tauri ios init       # or: cargo tauri android init
cargo tauri ios dev        # or: cargo tauri android dev
```

Current mobile limitations: secrets are kept in memory only (not yet in the
iOS Keychain or Android Keystore), and the "Open Workspace" menu is desktop-only
(mobile uses a workspace in the app's documents folder).
