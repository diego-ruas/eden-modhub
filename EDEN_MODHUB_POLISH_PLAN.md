# Polish the rest of the app (Eden ModHub)

## Context
The first design pass covered the scrollbars, button press feedback, open animations, checkboxes, the "Select a game." empty state and the Settings dialog. This pass polishes everything else: dialogs that open with a focus ring, emoji and Unicode status glyphs, a loud button hierarchy, the "Matching version only" toggle, the empty NSZ list, the Guide list, the install chooser, the update notes and toast entry.

Constraints:
- Keep the layout, the palette tokens in `:root` and the font (Geist) as they are.
- All edits go in `src/routes/+page.svelte` (markup, `<script>` and the `<style>` block).
- No new i18n strings are needed: the icons are `aria-hidden` and the existing labels stay.
- Do not commit, tag or release.

## Approach
The steps are independent. Run `npm run check` after each one; it must report `0 ERRORS 0 WARNINGS`.

### 1. Open every dialog without a focus ring
Problem: `showModal()` focuses the first control, so the Guide dialog opens with a ring around "Got it" and the update dialog opens with a ring around "Later". Settings already solves this with `tabindex="-1"` plus `sett?.focus()`. Make that the shared pattern.
- In `<script>`, add next to `openSettings()` (~335):
  `function openModal(d?: HTMLDialogElement) { d?.showModal(); d?.focus(); }`
- Replace every `showModal` call with it. The grep `showModal` must then return only the helper:
  - ~148 `dlg?.showModal()` → `openModal(dlg)`
  - ~264 `upd?.showModal()` → `openModal(upd)`
  - ~338-339 `sett?.showModal(); sett?.focus();` → `openModal(sett);`
  - ~451 `guide?.showModal()` → `openModal(guide)`
  - ~474 `upd?.showModal()` → `openModal(upd)`
  - ~522 `onclick={() => guide?.showModal()}` → `onclick={() => openModal(guide)}`
- Add `tabindex="-1"` to the `dlg` (~745), `guide` (~766) and `upd` (~776) `<dialog>` tags. `sett` already has it.
- CSS: replace `.settings:focus-visible { outline: none; }` with `.modal:focus-visible { outline: none; }`.

### 2. Stroke icons instead of glyphs
- Add these entries to the `ICON` map (~85):
  - `check: "M5 12l5 5 9-10",`
  - `x: "M6 6l12 12 M18 6L6 18",`
  - `alert: "M12 3l9.5 17h-19z M12 10v4 M12 17h.01",`
- Toasts (~740-741):
  - `<span class="ico" aria-hidden="true">⚠</span>` → `<span class="ico">{@render icon(ICON.alert)}</span>`
  - `<span class="ico" aria-hidden="true">✓</span>` → `<span class="ico">{@render icon(ICON.check)}</span>`
  - Close button content `×` → `{@render icon(ICON.x)}`. Keep its `aria-label`. The icon snippet already sets `aria-hidden` on the svg.
- NSZ rows:
  - ~608: `<div class="sub run">⏳ {runVerb(nszCur.op)}… {elapsed}</div>` → `<div class="sub run">{@render icon(ICON.refresh)}{runVerb(nszCur.op)}… {elapsed}</div>`
  - ~610: `{nszRes[r.path].ok ? "✓" : "✗"} ` → `{@render icon(nszRes[r.path].ok ? ICON.check : ICON.x)}`
- Mod version badge (~492): `{match ? "✓ " : ""}` → `{#if match}{@render icon(ICON.check)}{/if}`
- CSS:
  - `.toast .x`: drop `font-size: 16px;`, use `padding: 4px; display: inline-flex;`
  - Add `.ico { display: inline-flex; }`
  - Add `.sub .icon { width: 12px; height: 12px; vertical-align: -2px; margin-right: 4px; }`
  - Add `.sub.run .icon { animation: rot 1s linear infinite; }`. `@keyframes rot` already exists.
  - Add `.badge .icon { width: 11px; height: 11px; vertical-align: -1px; margin-right: 3px; }`
  - In the `prefers-reduced-motion` rule (~1017), add `.sub.run .icon { animation: none; }`.
- After this step, `grep -nE '[✓✗⏳⚠×]' src/routes/+page.svelte` must return nothing, except line ~671, which step 3 removes.

### 3. "Matching version only" becomes a checkbox
- Replace ~671
  `<div class="seg"><button aria-pressed={onlyMatch} onclick={() => (onlyMatch = !onlyMatch)}>{onlyMatch ? "✓ " : ""}{t("onlyMatch")} ({selected.version})</button></div>`
  with
  `<label class="toggle"><input type="checkbox" bind:checked={onlyMatch} /> {t("onlyMatch")} ({selected.version})</label>`
- `onlyMatch` is already `$state(false)` (~26). The checkbox styling and the generic `label:has(> input[type="checkbox"])` rule come from the first pass.
- CSS: add `.toggle { font-size: 12px; color: var(--fg-soft); }`.

### 4. Quieter button hierarchy
- `button.ghost` has no CSS today, so "View on GitHub", "Cancel" and "Later" look exactly like Install. Add:
  - `button.ghost { background: transparent; color: var(--muted); }`
  - `button.ghost:hover:not(:disabled) { background: var(--ghost); color: var(--fg); }`
- `button.danger` (~945): make it borderless red text, so a list of installed mods no longer shows a column of red boxes. Change it to `button.danger { background: transparent; color: var(--danger); }`. Keep the existing hover rule `background: var(--danger-bg)`.

### 5. Empty NSZ list uses the hint style
- ~626: `<p class="muted">{t("noRoms")}</p>` → `<div class="hint">{@render icon(ICON.nsz)}<p>{t("noRoms")}</p></div>`
- `.nsz-list` is a flex column with `flex: 1`, so the existing `.hint { margin: auto }` centers it. No new CSS.
- The games sidebar's `noGames` stays as `<p class="muted pad">`; the narrow column does not need it.

### 6. Guide steps with numbered badges
Replace `.guide ol { … }` (~1040) and `.guide li b { display: block; }` with:
```css
.guide ol { margin: 14px 0 18px; padding: 0; list-style: none; counter-reset: step; display: flex; flex-direction: column; gap: 14px; overflow-y: auto; }
.guide li { counter-increment: step; display: grid; grid-template-columns: 24px 1fr; column-gap: 12px; }
.guide li::before { content: counter(step); grid-row: span 2; width: 24px; height: 24px; border-radius: 50%; background: var(--badge); color: var(--fg-soft); font-size: 12px; font-weight: 600; display: grid; place-items: center; }
.guide li > * { grid-column: 2; }
.guide li b { font-weight: 600; }
```
The markup (~769-771) stays unchanged: `<li><b>…</b><span class="muted">…</span></li>`.

### 7. Install chooser rows
The rows in the `.rootlist` labels (~751) now run as flex rows because of the first-pass checkbox rule. Add:
```css
.rootlist label { padding: 8px 10px; border-radius: 8px; background: var(--input); border: 1px solid var(--border-soft); }
.rootlist label .muted { margin-left: auto; font-size: 12px; }
```

### 8. Update notes and toast entry
- Change `.upd-notes` (~1041) to:
  `.upd-notes { max-height: 240px; overflow-y: auto; white-space: pre-wrap; font: inherit; font-size: 13px; margin: 8px 0 16px; padding: 10px 12px; background: var(--input); border: 1px solid var(--border-soft); border-radius: 8px; }`
- Toast entry: add
  - `.toast { transition: opacity 0.16s ease, transform 0.16s ease; }` as a separate rule after `.toast`.
  - `@starting-style { .toast { opacity: 0; transform: translateY(8px); } }`
  - In the reduced-motion rule, add `.toast` to the `transition: none` list.

## Critical files & anchors
- `src/routes/+page.svelte`:
  - `ICON` map (~85): new icons.
  - `openSettings()` (~335): where `openModal` goes.
  - `modRow` snippet (~484): badge and ghost button.
  - Toasts (~729-742).
  - Style block (~897-1073).

## Verification
Run from `C:\Users\diego\Documents\Projetos\Eden-ModHub`, shell bash.

1. `npm run check` → `0 ERRORS 0 WARNINGS`.
   - `grep -nE '[✓✗⏳⚠×]' src/routes/+page.svelte` → no output.
   - `grep -n showModal src/routes/+page.svelte` → only the `openModal` line.
2. Build and launch with CDP:
   - Stop any running instance: `powershell.exe -Command "Stop-Process -Name eden-modhub -Force"`.
   - Build: `npm run tauri build -- --no-bundle` (~2.5 min).
   - Spawn `src-tauri/target/release/eden-modhub.exe` with env `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`.
   - Wait 10 s, then connect to the `page` target from `http://127.0.0.1:9222/json`.
   - `Emulation.setDeviceMetricsOverride` 1100×720, scale 1.
3. Run these `Runtime.evaluate` checks:
   - Open the Guide via `document.querySelector('.logo-btn').click()`, then `document.querySelectorAll('#app-menu button')[1].click()`.
     - `document.activeElement === document.querySelector('dialog.guide')` → `true`.
     - `getComputedStyle(document.querySelector('.guide li'), '::before').borderRadius` → `"50%"`.
     - Close with `Input.dispatchKeyEvent` Escape (rawKeyDown and keyUp, `windowsVirtualKeyCode` 27).
   - Click the game whose `.game` text includes "Mario Party Superstars":
     - `document.querySelector('.filters .toggle input[type=checkbox]')` exists.
     - Clicking it changes `document.querySelectorAll('.detail .row').length` (filter applied). Click it again to restore.
     - `getComputedStyle(document.querySelector('.row button.ghost')).backgroundColor` → `"rgba(0, 0, 0, 0)"`.
     - `getComputedStyle(document.querySelector('button.danger')).borderTopStyle` → `"none"` (only if a mod is installed).
   - Toast: `document.querySelector('.toasts [role=status]')` is filled after the menu's "Check for updates" when already up to date. Assert `.toast.ok .ico svg` exists.
4. Screenshots to `%TEMP%`, then view each one:
   - Mods with a game selected: quiet "View on GitHub", borderless Remove, checkbox toggle, check icon in the matching badge.
   - The Guide: numbered circles, no ring on "Got it".
   - The NSZ tab.
   - The toast.
   - The Mods view in light mode, via `Emulation.setEmulatedMedia({ features: [{ name: "prefers-color-scheme", value: "light" }] })`.
5. Re-capture the README screenshots `games`, `mods`, `nsz`, `menu`, `guide` and `settings` into `docs/screenshots/` with the same procedure as before:
   - English UI.
   - Before each capture, run a text-node walk that replaces `Users\diego` with `Users\user`.
   - `mods`: after clicking "Mario Party Superstars".
   - `guide`: via menu item 2.
   - `settings`: via menu item 1.
   - Then `Stop-Process -Name eden-modhub -Force`. Leave everything uncommitted.

## Assumptions & contingencies
- The scope stays the same as the first pass: refine the current look; no palette, font or layout changes. The user can override the button decisions in step 4 (borderless Remove, muted ghost buttons).
- If `@starting-style` on `.toast` does not animate, because the toast is inserted inside an `{#if}` that Svelte mounts, leave the rule in place. It is harmless; do not add a Svelte `transition:` instead.
- If the `noGames` sidebar message or another empty state still looks unstyled in the screenshots, leave it. It is outside this list.
