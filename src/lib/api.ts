import { invoke } from "@tauri-apps/api/core";

export type Source = "official" | "theboy181" | "wiki";
// Espelha catalog.rs: repositório, branch e prefixo do id de cada fonte.
const SOURCES: Record<Source, { repo: string; branch: string; label: string }> = {
  official: { repo: "ADEMOLA200/Switch-Emulator-Mod-Database", branch: "develop", label: "" },
  theboy181: { repo: "theboy181/switch-ptchtxt-mods", branch: "main", label: "TheBoy181" },
  wiki: { repo: "amakvana/Switch-Mods-Wiki-Archive", branch: "main", label: "Wiki" },
};
export const SOURCE_LABEL: Record<Source, string> = {
  official: SOURCES.official.label,
  theboy181: SOURCES.theboy181.label,
  wiki: SOURCES.wiki.label,
};

export type ModFile = { src: string; dest: string };
export type ModEntry = {
  id: string;
  tid: string | null;
  name: string;
  version: string | null;
  kind: "archive" | "files";
  files: ModFile[];
  size: number;
  group: string;
  source: Source;
};
export type Catalog = {
  treeSha: string;
  fetchedAt: number;
  mods: ModEntry[];
  names: Record<string, string>;
};
export type Game = { tid: string; name: string | null; version: string | null; icon: string | null };
export type RootInfo = { key: string; name: string; fileCount: number };
export type Prepared = { token: string; roots: RootInfo[] };
export type Installed = {
  tid: string;
  folder: string;
  modId: string;
  rootKey: string;
  name: string;
  version: string | null;
  installedAt: number;
};

export type Emu = "eden" | "yuzu" | "ryujinx";
export type RomFile = { path: string; name: string; ext: string; size: number };
export type NszOp = "compress" | "decompress" | "verify" | "info";
export type NszResult = { ok: boolean; log: string; output: string | null; outputSize: number | null };
export type UpdateCheck = { current: string; version: string | null; notes: string | null };

export const norm = (s: string) =>
  s
    .replace(/\s*-\s*\d+$/, "")
    .toLowerCase()
    .replace(/[^a-z0-9]/g, "");

/** Caminho no repositório (o id das fontes extras tem o prefixo "<fonte>:"). */
export const modPath = (m: ModEntry) => (m.source === "official" ? m.id : m.id.slice(m.source.length + 1));

export const githubUrl = (m: ModEntry) =>
  `https://github.com/${SOURCES[m.source].repo}/tree/${SOURCES[m.source].branch}/${modPath(m)
    .split("/")
    .map(encodeURIComponent)
    .join("/")}`;

export const api = {
  getEmu: () => invoke<{ kind: Emu; dir: string | null }>("get_emu"),
  setEmulator: (kind: Emu) => invoke<void>("set_emulator", { kind }),
  setEmuDir: (path: string) => invoke<void>("set_emu_dir", { path }),
  getCatalog: (force: boolean) => invoke<Catalog>("get_catalog", { force }),
  listGames: () => invoke<Game[]>("list_games"),
  gameCover: (tid: string) => invoke<string | null>("game_cover", { tid }),
  listInstalled: (tid: string) => invoke<Installed[]>("list_installed", { tid }),
  checkUpdate: () => invoke<UpdateCheck>("check_update"),
  installUpdate: () => invoke<void>("install_update"),
  prepareInstall: (tid: string, modId: string) => invoke<Prepared>("prepare_install", { tid, modId }),
  commitInstall: (token: string, keys: string[]) => invoke<Installed[]>("commit_install", { token, keys }),
  cancelInstall: (token: string) => invoke<void>("cancel_install", { token }),
  uninstall: (tid: string, folder: string) => invoke<void>("uninstall", { tid, folder }),
  openModFolder: (tid: string) => invoke<void>("open_mod_folder", { tid }),
  listRoms: () => invoke<RomFile[]>("list_roms"),
  nszRun: (op: NszOp, path: string, deleteSource: boolean) =>
    invoke<NszResult>("nsz_run", { op, path, deleteSource }),
  nszCanVerify: () => invoke<boolean>("nsz_can_verify"),
  peekArchive: (modId: string) => invoke<RootInfo[]>("peek_archive", { modId }),
};
