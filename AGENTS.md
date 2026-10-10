# AGENTS.md

Eden Mod Manager: desktop app (Tauri 2 + SvelteKit 5 + Rust) to browse, install and manage Switch mods for Eden, yuzu and Ryujinx, with an optional NSZ compressor and a self-updater. Repo: `pendiego/Eden-Mod-Manager`. The old name survives in exactly two places on purpose, both for compatibility with installed copies: the bundle `identifier` in `tauri.conf.json` (changing it moves the app data folder, so users would lose settings and the installed-mods manifest) and the legacy `EdenModHub-portable.zip` asset (see Portable mode).

## Layout

| Path | What lives there |
|---|---|
| `src/routes/+page.svelte` | The whole UI (single page, scoped CSS at the bottom, dialogs at the end of the markup) |
| `src/lib/api.ts` | Typed `invoke` wrappers and shared types. Mirrors Rust command signatures |
| `src/lib/i18n.svelte.ts` | `pt` and `en` strings, `t()`, and `ERR_EN` (maps Portuguese Rust error text to English) |
| `src-tauri/src/lib.rs` | `run()`, command registration, `app_dir`, `portable_dir`, settings |
| `src-tauri/src/emu.rs` | Emulator detection, data folders, game list |
| `src-tauri/src/catalog.rs` | Mod catalogs (official, TheBoy181, Wiki), cache, covers. Defines `UA` |
| `src-tauri/src/pack.rs` | PT-BR translation pack: one release `.zip` read by HTTP Range (list from the central directory, extract only one TID). GitHub's CDN rejects suffix ranges (`bytes=-N`) with 501 |
| `src-tauri/src/gamebanana.rs` | GameBanana API v11, fetched per game when it is opened: index pages 6 at a time (global cap of 6 requests in flight, one retry on 429), 10 minute in-memory cache (curated is derived from the full list), `gamebanana_detail` for the popup. Mods live only in the backend `CatalogState`, not in the frontend catalog |
| `src-tauri/src/install.rs` | `download` (emits `download-progress`), install/uninstall, manifest |
| `src-tauri/src/nsz.rs` | NSZ tool download and runs |
| `src-tauri/src/update.rs` | `check_update` / `install_update`, portable self-replace, minisign `verify` |
| `src-tauri/src/prefs.rs` | Settings backend: `storage_info`, `clear_cache`, `remove_tools` |
| `src-tauri/tauri.conf.json` | App config, updater `pubkey` and endpoint |
| `src-tauri/tauri.updater.conf.json` | `createUpdaterArtifacts`, passed with `--config` by CI only |
| `.github/workflows/build.yml` | Tag-triggered build and release |
| `docs/screenshots/` | English README screenshots |

## Commands

```sh
npm ci
npm run tauri dev                                  # needs Vite on :1420; builds src-tauri/target/debug
npm run tauri build                                # release exe + NSIS installer, no signing key needed
npm run check                                      # svelte-check, must be 0 errors 0 warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

Run `npm run check` and `cargo test` after code changes. Shell is bash on Windows; use `powershell.exe` explicitly for PowerShell.

## Conventions

- **Backend errors are Portuguese strings** returned as `Result<_, String>`. Every user-visible error prefix needs a matching pair in `ERR_EN` in `i18n.svelte.ts`, or English users see Portuguese.
- **UI strings go in both `pt` and `en`** in `i18n.svelte.ts`. No hardcoded text in the Svelte file.
- **New Tauri command** = Rust `#[tauri::command]` + entry in `generate_handler!` in `lib.rs` + wrapper in `api.ts`.
- **All paths go through `app_dir(app, Dir::...)`** so portable mode works. Never use `app.path()` directly for app data.
- **HTTP**: send `User-Agent: catalog::UA`. Downloads go through `install::download` so `download-progress` events and the toast keep working. `catalog::HTTP` has gzip off on purpose (downloads and Range reads need exact bytes); JSON APIs use `catalog::HTTP_JSON`, which negotiates gzip.
- Icons are inline stroke paths in the `ICON` map (viewBox 24). No icon library.
- The frontend does not use `@tauri-apps/plugin-updater` or `plugin-process`; the updater is fully in Rust, so `capabilities/default.json` needs no updater permissions.
- Code comments are in Portuguese; the README and `AGENTS.md` are in English.
- **`Dir::Cache` is not a private folder.** On Windows it is `%LOCALAPPDATA%\<identifier>`, which also holds the WebView2 profile (`EBWebView`, in use while the app runs). Delete only what the app writes there (`catalog.json`, `covers/`), as `prefs.rs` does; never `remove_dir_all` the whole directory.
- **UI preferences** (`lang`, `mica`, `autoUpdate`, `guideSeen`) live in `localStorage`, not in `Settings`. The app icon at the top of the rail opens a native `popover` menu (Settings, Guide, Check for updates); Settings is a `<dialog>`.
- **Screenshots and README follow the UI.** Any change that alters what a screen looks like (layout, new tab or dialog, copy, window size) must refresh the affected `docs/screenshots/*.png` and the README text in the same PR. Capture at the default window size (1360x840), in English, with the user name masked as `user`, via the WebView2 CDP route (see Testing notes); uninstall anything installed for the shot and clear the scratch data afterwards.

## Portable mode (Windows)

A file named `portable` next to the exe makes `portable_dir()` return the exe folder; data then lives in `data/{config,data,cache}`. The portable updater downloads `EdenModManager-portable.zip` + `.sig` from the release, verifies it against the `pubkey` in `tauri.conf.json`, renames the running exe to `.exe.old`, writes the new one and relaunches. `update::cleanup_old_exe` removes the leftover. CI also attaches a legacy `EdenModHub-portable.zip` (inner `EdenModHub.exe`) because portable builds <= 0.2.2 look for that name; drop it once those are gone.

## Updater and releases

- Signing key lives outside the repo: `~/.tauri/eden-modhub.key`, `.key.pub`, `.key.password`. Secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` are set in GitHub. **Losing the key means installed apps can never update again.** Never commit it, never regenerate it.
- Do not put `createUpdaterArtifacts` in `tauri.conf.json`: it would force a private key on every local build.
- A release is created by pushing a tag `v*` (version must match `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`). `tauri-action` builds the three platforms, uploads bundles and `latest.json`; a Windows step signs and attaches the portable zip. Pushes to `main` do not release.
- `.github/workflows/build.yml` generates GitHub release notes for each `v*` tag. `.github/release.yml` groups merged PRs by label.
- Apply `breaking-change`, `feature`, `enhancement`, `performance`, `bug`, `fix`, `documentation`, `dependencies`, `maintenance`, or `chore` labels before tagging. Unmatched PRs go under `Other Changes`.
- `skip-changelog` excludes a PR; Dependabot PRs are excluded.
- The updater reads `releases/latest/download/latest.json`, so deleting or breaking the latest release breaks updates for everyone.
- Bump all three version files together, then `npm install --package-lock-only` and `cargo check`.

## Git workflow (omp guard)

`main` is protected. Land changes via branch + PR:

1. `git checkout -b <branch>` (own call)
2. `git add -A; git commit -m "..."` (subject at most 72 chars, Conventional Commits)
3. `git push -u origin <branch>`
4. `gh pr create --base main ...`, then `gh pr merge <n> --squash --delete-branch`
5. `git checkout main`, `git pull --ff-only`, then tag if releasing

The guard reads the current branch before each command. It blocks `git commit` on `main`, and any command string that contains `git push` together with the bare word `main` (for example `git pull --ff-only origin main; git push origin v1`). Keep each branch-dependent step in its own call, and run `git pull --ff-only` without `origin main`.

## Testing notes

- The only permanent Rust test of note is `update::tests::verify_accepts_signed_and_rejects_tampered` (minisign verification, uses a throwaway test keypair).
- Inspecting the running app: launch with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` and drive it over CDP at `http://127.0.0.1:9222/json`.
- Windows NSIS installs are per-user: registry under `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall`, files in `%LOCALAPPDATA%\Eden Mod Manager`.
- Linux and macOS builds are only verified through CI and the `latest.json` platform keys.
