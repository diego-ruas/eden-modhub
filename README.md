# Eden ModHub

Instalador de mods e compressor NSZ para Eden, yuzu e Ryujinx. Tauri 2 + SvelteKit.

| Sistema | Pacotes | Observações |
|---|---|---|
| Windows | NSIS | Mica no Windows 11 |
| Linux | deb, AppImage | config/cache seguem XDG (`~/.config`, `~/.cache`) e Flatpak (yuzu, Ryujinx) |
| macOS | app, dmg | nsz só em Apple Silicon (sem binário Intel) |

Pastas padrão: Eden/yuzu em `%APPDATA%` · `~/.local/share` · `~/Library/Application Support`; Ryujinx em `%APPDATA%` · `~/.config` · `~/Library/Application Support`. Outra pasta: botão "Alterar".

## Desenvolvimento

```
npm ci
npm run tauri dev
npm run tauri build
```

Linux precisa de `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev patchelf` (veja `.github/workflows/build.yml`).

## Portátil (Windows)

Baixe `EdenModHub-portable.zip` da página de Releases e extraia. Enquanto o arquivo `portable` ficar ao lado do `.exe`, configurações, cache e ferramentas ficam em `data/` na mesma pasta.

## Licença

MIT
