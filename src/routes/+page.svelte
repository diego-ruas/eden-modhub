<script lang="ts">
  import "@fontsource-variable/geist";
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getVersion } from "@tauri-apps/api/app";
  import { open, confirm } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    api, githubUrl, modPath, SOURCE_LABEL, norm,
    type Catalog, type Conflict, type Game, type InstalledView, type ModEntry, type NszOp, type Prepared, type RomFile,
    type Emu, type RootInfo, type Source, type UpdateCheck, type EmuDir, type StorageInfo,
  } from "$lib/api";
  import { EMU_HINT, EMU_NAME, i18n, locale, setLang, t, trErr, type Lang } from "$lib/i18n.svelte";

  let emu = $state<Emu>("eden");
  let emuDir = $state<string | null>(null);
  let catalog = $state<Catalog | null>(null);
  let games = $state<Game[]>([]);
  let selected = $state<Game | null>(null);
  let installed = $state<InstalledView[]>([]);
  let conflicts = $state<Conflict[]>([]);
  let gameFilter = $state("");
  let contents = $state<Record<string, RootInfo[]>>({});
  const PEEK_MAX = 30 * 1048576;
  let search = $state("");
  let srcFilter = $state<Source | "all">("all");
  let onlyMatch = $state(false);
  let error = $state("");
  let notice = $state("");
  let busy = $state(false);
  let loadingCatalog = $state(false);
  let progress = $state<{ received: number; total: number | null } | null>(null);
  let prepared = $state<Prepared | null>(null);
  let dlg = $state<HTMLDialogElement>();
  let guide = $state<HTMLDialogElement>();
  let upd = $state<HTMLDialogElement>();
  let update = $state<UpdateCheck | null>(null);
  let sett = $state<HTMLDialogElement>();
  let appVersion = $state("");
  let emuDirs = $state<EmuDir[]>([]);
  let storage = $state<StorageInfo | null>(null);
  let autoUpdate = $state(localStorage.getItem("autoUpdate") !== "off");
  let micaOn = $state(localStorage.getItem("mica") !== "off");
  let micaOk = $state(false);
  let checked = $state<Record<string, boolean>>({});
  let tab = $state<"mods" | "nsz">("mods");
  let roms = $state<RomFile[]>([]);
  let picked = $state<RomFile[]>([]);
  let deleteSource = $state(false);
  let nszLog = $state("");
  let nszStep = $state<{ percent: number | null; step: string } | null>(null);
  let nszCur = $state<{ path: string; name: string; op: NszOp; stage: string } | null>(null);
  let nszRes = $state<Record<string, { ok: boolean; text: string }>>({});
  let nowMs = $state(0);
  let startMs = 0;
  const fmtDuration = (ms: number) => {
    const s = Math.floor(ms / 1000);
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  };
  const elapsed = $derived(fmtDuration(nowMs - startMs));
  const runVerb = (op: NszOp) =>
    t(op === "compress" ? "runCompress" : op === "decompress" ? "runDecompress" : op === "verify" ? "runVerify" : "runInfo");
  const stageText = $derived(
    nszCur?.stage === "tool" ? t("stageTool")
    : nszCur?.stage === "legacy" ? t("stageLegacy")
    : nszStep ? `${nszStep.step} ${nszStep.percent?.toLocaleString(locale(), { maximumFractionDigits: 0 }) ?? ""}%`.trim()
    : "",
  );
  $effect(() => {
    if (!nszCur) return;
    const id = setInterval(() => (nowMs = Date.now()), 1000);
    return () => clearInterval(id);
  });
  // mantém o log rolado até a última linha
  function stick(node: HTMLElement, _: string) {
    node.scrollTop = node.scrollHeight;
    return { update: () => (node.scrollTop = node.scrollHeight) };
  }
  let romFilter = $state("");
  const romList = $derived([...picked, ...roms.filter((r) => !picked.some((p) => p.path === r.path))]);
  const shownRoms = $derived.by(() => {
    const q = romFilter.trim().toLowerCase();
    return q ? romList.filter((r) => r.path.toLowerCase().includes(q)) : romList;
  });
  // ícones de traço próprios (viewBox 24); sem biblioteca de ícones instalada
  const ICON = {
    mods: "M4 4h7v7H4z M13 4h7v7h-7z M4 13h7v7H4z M13 13h7v7h-7z",
    nsz: "M3 7h18v13H3z M3 7l2-3h14l2 3 M10 11h4",
    search: "M11 4a7 7 0 1 0 0 14a7 7 0 1 0 0-14z M20 20l-4.2-4.2",
    refresh: "M20 12a8 8 0 1 1-2.3-5.6 M20 4v5h-5",
    folder: "M3 6h6l2 2h10v11H3z",
    plus: "M12 5v14 M5 12h14",
    chev: "M9 6l6 6-6 6",
    help: "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18z M9.6 9.5a2.5 2.5 0 1 1 3.5 2.3c-.7.4-1.1.9-1.1 1.7 M12 17h.01",
    update: "M12 4v11 M7 10l5 5 5-5 M5 20h14",
    check: "M5 12l5 5 9-10",
    x: "M6 6l12 12 M18 6L6 18",
    alert: "M12 3l9.5 17h-19z M12 10v4 M12 17h.01",
    settings: "M12 15a3 3 0 1 0 0-6a3 3 0 1 0 0 6z M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
  };
  const initials = (s: string) =>
    s.split(/\s+/).map((w) => w.match(/[\p{L}\p{N}]/u)?.[0] ?? "").filter(Boolean).slice(0, 2).join("").toUpperCase() || "?";
  const cleanName = (n: string) =>
    n.replace(/\s*\[(?:[0-9a-f]{16}|v\d+)\]/gi, "").replace(/\s*\([\d.]+ GB\)/, "").replace(/\.[^.]+$/, "") || n;
  // TID de jogo base termina em 000, update em 800, DLC em qualquer outro sufixo.
  const kindOf = (n: string): "tagBase" | "tagUpdate" | "tagDlc" | null => {
    const tid = n.match(/\b[0-9a-f]{13}([0-9a-f]{3})\b/i)?.[1].toLowerCase();
    if (/dlc/i.test(n)) return "tagDlc";
    return tid === "000" ? "tagBase" : tid === "800" ? "tagUpdate" : tid ? "tagDlc" : null;
  };
  // Agrupa por TID base (updates/DLCs caem no jogo: últimos 13 bits zerados).
  const baseKey = (n: string) => {
    const tid = n.match(/\b[0-9a-f]{16}\b/gi)?.pop();
    return tid ? (BigInt("0x" + tid) & ~0x1fffn).toString(16).padStart(16, "0") : cleanName(n);
  };
  const groups = $derived.by(() => {
    const m = new Map<string, RomFile[]>();
    for (const r of shownRoms) {
      const k = baseKey(r.name);
      m.set(k, [...(m.get(k) ?? []), r]);
    }
    return [...m].map(([key, items]) => {
      const base = items.find((r) => kindOf(r.name) === "tagBase") ?? items[0];
      return {
        key,
        items,
        name: base.name.split(" [")[0].replace(/\.[^.]+$/, ""),
        size: items.reduce((s, r) => s + r.size, 0),
      };
    }).sort((a, b) => a.name.localeCompare(b.name));
  });
  // Capas dos grupos do compressor: reaproveita as dos jogos; o resto é buscado uma vez por sessão.
  let romCovers = $state<Record<string, string>>({});
  const romTried = new Set<string>();
  const romCover = (key: string) => games.find((g) => g.tid === key)?.icon ?? romCovers[key];
  $effect(() => {
    const keys = groups.map((g) => g.key).filter((k) => /^[0-9a-f]{16}$/.test(k) && !romCover(k) && !romTried.has(k));
    keys.forEach((k) => romTried.add(k));
    void (async () => {
      for (const k of keys) {
        const icon = await api.gameCover(k).catch(() => null);
        if (icon) romCovers[k] = icon;
      }
    })();
  });
  const itemName = (r: RomFile, game: string) => {
    const n = cleanName(r.name);
    return n.startsWith(game) ? n.slice(game.length).replace(/^[\s\-\[\]]+|[\s\]]+$/g, "") || n : n;
  };

  $effect(() => {
    if (prepared && !dlg?.open) openModal(dlg);
    else if (!prepared && dlg?.open) dlg.close();
  });

  let canVerify = $state(true);
  async function loadRoms() {
    roms = (await run(api.listRoms)) ?? [];
    canVerify = await api.nszCanVerify().catch(() => true);
  }

  async function showNsz() {
    tab = "nsz";
    await loadRoms();
  }

  async function pickRom() {
    const p = await open({
      multiple: false,
      filters: [{ name: t("switchGames"), extensions: ["nsp", "xci", "nsz", "xcz"] }],
    });
    if (typeof p !== "string" || picked.some((r) => r.path === p)) return;
    const name = p.split(/[\\/]/).pop() ?? p;
    picked = [...picked, { path: p, name, ext: (name.split(".").pop() ?? "").toLowerCase(), size: 0 }];
  }

  async function nsz(op: NszOp, r: RomFile) {
    if (busy) return;
    busy = true;
    notice = "";
    nszLog = "";
    delete nszRes[r.path];
    nszStep = null;
    nszCur = { path: r.path, name: r.name, op, stage: "run" };
    nowMs = startMs = Date.now();
    const del = deleteSource && (op === "compress" || op === "decompress");
    const res = await run(() => api.nszRun(op, r.path, del));
    const failure = error;
    progress = null;
    nszStep = null;
    nszCur = null;
    busy = false;
    if (res) {
      nszLog = res.log;
      const mark = (text: string, ok: boolean) => {
        nszRes[r.path] = { ok, text };
        if (res.output) nszRes[res.output] = { ok, text };
      };
      if (res.ok) {
        const took = fmtDuration(Date.now() - startMs);
        const sizes = res.outputSize && r.size ? `${fmtSize(r.size)} → ${fmtSize(res.outputSize)} · ` : "";
        notice =
          op === "verify" ? t("verifyOk")
          : op === "info" ? t("infoBelow")
          : t("done", { file: res.output?.split(/[\\/]/).pop() ?? r.name });
        mark(op === "verify" ? t("verifyOk") : op === "info" ? t("infoBelow") : `${sizes}${took}`, true);
        if (del) picked = picked.filter((p) => p.path !== r.path);
      } else {
        error = t("nszFailed");
        mark(t("failedShort"), false);
      }
    } else if (failure) {
      nszRes[r.path] = { ok: false, text: failure };
    }
    await loadRoms();
  }

  const fmtSize = (n: number) =>
    n >= 1073741824
      ? `${(n / 1073741824).toLocaleString(locale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 })} GB`
      : n >= 1048576
        ? `${Math.round(n / 1048576).toLocaleString(locale())} MB`
        : `${Math.max(1, Math.round(n / 1024)).toLocaleString(locale())} KB`;

  const filteredGames = $derived(
    games.filter((g) =>
      `${g.name ?? ""} ${g.tid}`.toLowerCase().includes(gameFilter.trim().toLowerCase()),
    ),
  );
  const q = $derived(search.trim().toLowerCase());
  const pass = (m: ModEntry, matchOnly = true) =>
    (srcFilter === "all" || m.source === srcFilter) &&
    (!matchOnly || !onlyMatch || !selected?.version || !m.version || m.version === selected.version) &&
    (!q || m.name.toLowerCase().includes(q) || m.id.toLowerCase().includes(q));
  const available = $derived(
    selected && catalog ? catalog.mods.filter((m) => m.tid === selected!.tid && pass(m)) : [],
  );
  const possible = $derived.by(() => {
    if (!selected?.name || !catalog) return [];
    const g = norm(selected.name);
    if (!g) return [];
    return catalog.mods.filter((m) => {
      if (m.tid !== null || !pass(m)) return false;
      const k = norm(m.group);
      return k && (k.includes(g) || g.includes(k));
    });
  });
  // Busca global (outros jogos): só com texto digitado; versão do jogo atual não se aplica.
  const searchHits = $derived.by(() => {
    if (!q || !catalog) return [];
    const listed = new Set([...available, ...possible].map((m) => m.id));
    return catalog.mods.filter((m) => !listed.has(m.id) && pass(m, false));
  });
  const searchResults = $derived(searchHits.slice(0, 200));

  async function run<T>(fn: () => Promise<T>): Promise<T | undefined> {
    error = "";
    try {
      return await fn();
    } catch (e) {
      error = trErr(String(e));
    }
  }

  async function checkUpdate() {
    const r = await run(() => api.checkUpdate());
    if (!r) return;
    if (r.version) { update = r; openModal(upd); } else notice = t("updLatest", { v: r.current });
  }
  async function installUpdate() {
    upd?.close();
    busy = true;
    progress = { received: 0, total: null };
    await run(() => api.installUpdate()); // em caso de sucesso o app fecha/reinicia; daqui pra baixo só em falha
    busy = false;
    progress = null;
  }

  // Capas que o emulador não guardou: busca uma a uma (sem rajada); cada jogo é tentado uma vez por sessão.
  const coverCache = new Map<string, string | null>();
  async function loadCovers() {
    for (const g of games) {
      if (!g.icon && coverCache.get(g.tid)) g.icon = coverCache.get(g.tid)!;
    }
    for (const g of games.filter((g) => !g.icon && !coverCache.has(g.tid))) {
      const icon = await api.gameCover(g.tid).catch(() => null);
      coverCache.set(g.tid, icon);
      const cur = games.find((x) => x.tid === g.tid);
      if (icon && cur) cur.icon = icon;
    }
  }

  async function loadGames() {
    games = (await run(api.listGames)) ?? [];
    if (selected && !games.some((g) => g.tid === selected!.tid)) selected = null;
    void loadCovers();
  }

  async function loadCatalog(force: boolean) {
    busy = true;
    loadingCatalog = true;
    const c = await run(() => api.getCatalog(force));
    loadingCatalog = false;
    busy = false;
    if (c) catalog = c;
    await loadGames();
  }

  // Relê emulador/pasta salvos e recarrega tudo que depende deles.
  async function reloadEmu() {
    const i = await run(api.getEmu);
    emu = i?.kind ?? "eden";
    i18n.emu = EMU_NAME[emu];
    emuDir = i?.dir ?? null;
    selected = null;
    installed = [];
    conflicts = [];
    roms = [];
    picked = [];
    nszLog = "";
    if (!emuDir) games = [];
    else if (catalog) await loadGames();
    else await loadCatalog(false);
  }

  async function switchEmu(k: Emu) {
    if ((await run(() => api.setEmulator(k))) === undefined && error) return;
    tab = "mods";
    await reloadEmu();
  }

  async function changeDir(kind: Emu = emu) {
    const p = await open({ directory: true, title: t("edenDirTitle", { emu: EMU_NAME[kind] }) });
    if (typeof p !== "string") return;
    if ((await run(() => api.setEmuDir(kind, p))) === undefined && error) return;
    if (kind === emu) await reloadEmu();
    if (sett?.open) await refreshSettings();
  }

  function openModal(d?: HTMLDialogElement) { d?.showModal(); d?.focus(); }

  async function openSettings() {
    error = "";
    notice = "";
    openModal(sett);
    appVersion ||= await getVersion();
    await refreshSettings();
  }
  async function refreshSettings() {
    emuDirs = (await run(api.getEmuDirs)) ?? [];
    storage = (await run(api.storageInfo)) ?? null;
  }
  async function clearCache() {
    if ((await run(() => api.clearCache())) === undefined && error) return;
    notice = t("setCleared");
    await refreshSettings();
  }
  async function removeTools() {
    if ((await run(() => api.removeTools())) === undefined && error) return;
    notice = t("setNszRemoved");
    await refreshSettings();
  }
  function applyMica() {
    if (micaOk && micaOn) document.documentElement.dataset.mica = "";
    else delete document.documentElement.dataset.mica;
  }
  function setMica(on: boolean) {
    micaOn = on;
    localStorage.setItem("mica", on ? "on" : "off");
    applyMica();
  }
  function setAutoUpdate(on: boolean) {
    autoUpdate = on;
    localStorage.setItem("autoUpdate", on ? "on" : "off");
  }

  async function select(g: Game) {
    selected = g;
    notice = "";
    await refreshInstalled();
    peekAll(
      g,
      [...available, ...possible]
        .filter((m) => m.kind === "archive" && m.size <= PEEK_MAX && !contents[m.id])
        .sort((a, b) => a.size - b.size),
    );
  }

  // Lê pacotes do jogo em sequência (menores primeiro); para se trocar de jogo.
  async function peekAll(g: Game, mods: ModEntry[]) {
    for (const m of mods) {
      if (selected?.tid !== g.tid) return;
      try {
        contents[m.id] = await api.peekArchive(m.id);
      } catch {
        // pacote ilegível: fica só com "Instalar", que mostra o erro real
      }
    }
    if (!busy) progress = null;
  }

  async function refreshInstalled() {
    const tid = selected?.tid;
    installed = tid ? ((await run(() => api.listInstalled(tid))) ?? []) : [];
    const c = tid && installed.length > 1 ? await api.listConflicts(tid).catch(() => []) : [];
    if (selected?.tid === tid) conflicts = c;
  }

  /** Mod do catálogo com versão diferente da instalada (só quando as duas são conhecidas). */
  const newer = (i: InstalledView) => {
    const m = catalog?.mods.find((m) => m.id === i.modId);
    return m?.version && i.version && m.version !== i.version ? m : undefined;
  };

  async function toggle(i: InstalledView) {
    await run(() => api.setModEnabled(i.tid, i.folder, !i.enabled));
    await refreshInstalled();
  }

  async function install(m: ModEntry, only?: string) {
    if (!selected || busy) return;
    busy = true;
    notice = "";
    progress = { received: 0, total: null };
    const p = await run(() => api.prepareInstall(selected!.tid, m.id));
    progress = null;
    if (!p) {
      busy = false;
      return;
    }
    if (only !== undefined) {
      await commit(p.token, [only]);
      return;
    }
    if (p.roots.length === 1) {
      await commit(p.token, [p.roots[0].key]);
    } else {
      prepared = p;
      checked = {};
    }
  }

  async function commit(token: string, keys: string[]) {
    const r = await run(() => api.commitInstall(token, keys));
    prepared = null;
    busy = false;
    if (r) notice = t(emu === "ryujinx" ? "installedNoticeRyu" : "installedNotice");
    await refreshInstalled();
  }

  async function cancel() {
    if (prepared) await run(() => api.cancelInstall(prepared!.token));
    prepared = null;
    busy = false;
  }

  async function remove(i: InstalledView) {
    const ok = await confirm(t("confirmRemove", { name: i.folder }), {
      title: "Eden Mod Manager",
      kind: "warning",
      okLabel: t("remove"),
      cancelLabel: t("cancel"),
    });
    if (!ok) return;
    await run(() => api.uninstall(i.tid, i.folder));
    await refreshInstalled();
  }

  onMount(() => {
    document.documentElement.lang = locale();
    if (!localStorage.getItem("guideSeen")) openModal(guide);
    // Mica só existe no Windows 11 (platformVersion >= 13 no Chromium/WebView2); fora dele o fundo fica opaco
    const uad = (navigator as Navigator & {
      userAgentData?: { platform: string; getHighEntropyValues(h: string[]): Promise<{ platformVersion?: string }> };
    }).userAgentData;
    uad?.getHighEntropyValues(["platformVersion"]).then((v) => {
      micaOk = uad.platform === "Windows" && parseInt(v.platformVersion ?? "0") >= 13;
      applyMica();
    });
    const un = listen<{ received: number; total: number | null }>("download-progress", (e) => {
      progress = e.payload;
    });
    const unNsz = listen<{ percent: number | null; step: string }>("nsz-progress", (e) => {
      nszStep = e.payload;
    });
    const unStage = listen<string>("nsz-stage", (e) => {
      if (e.payload === "fallback") nszLog = "";
      if (nszCur) nszCur.stage = e.payload;
    });
    const unLog = listen<string>("nsz-log", (e) => {
      nszLog = (nszLog + e.payload + "\n").slice(-6000);
    });
    reloadEmu();
    if (autoUpdate) api.checkUpdate().then((r) => { if (r.version && !guide?.open) { update = r; openModal(upd); } }).catch(() => {});
    return () => {
      un.then((f) => f());
      unNsz.then((f) => f());
      unStage.then((f) => f());
      unLog.then((f) => f());
    };
  });
</script>

{#snippet modRow(m: ModEntry)}
  <div class="row">
    <div class="info">
      <div class="name" title={m.name}>{m.name}</div>
      <div class="sub" title={modPath(m)}>{#if m.source !== "official"}<span class="src">{SOURCE_LABEL[m.source]}</span> {/if}{modPath(m)}</div>
    </div>
    {#if m.version}
      {@const match = selected?.version === m.version}
      <span class="badge" class:ok={match} title={match ? t("versionMatch") : undefined}>{#if match}{@render icon(ICON.check)}{/if}{m.version}</span>
    {/if}
    <span class="size">{fmtSize(m.size)}</span>
    <button disabled={busy} onclick={() => install(m)}>{t("install")}</button>
    <button class="ghost" onclick={() => openUrl(githubUrl(m))}>{t("viewGithub")}</button>
  </div>
  {#if m.kind === "archive"}
    {#if (contents[m.id]?.length ?? 0) > 1}
      <div class="subrows">
        {#each contents[m.id] as r (r.key)}
          <div class="row sm">
            <div class="info"><div class="name" title={r.name}>{r.name}</div></div>
            <span class="size">{t("nFiles", { n: r.fileCount })}</span>
            <button disabled={busy} onclick={() => install(m, r.key)}>{t("install")}</button>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
{/snippet}

{#snippet icon(d: string)}
  <svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path {d} /></svg>
{/snippet}

<div class="app">
  <nav class="rail" aria-label="Eden Mod Manager">
    <button class="logo-btn" popovertarget="app-menu" aria-haspopup="menu" aria-label={t("menu")} title={t("menu")}><img class="logo" src="/logo.png" alt="" /></button>
    <div id="app-menu" class="menu" popover role="menu">
      <button role="menuitem" popovertarget="app-menu" popovertargetaction="hide" onclick={openSettings}>{@render icon(ICON.settings)}{t("settings")}</button>
      <button role="menuitem" popovertarget="app-menu" popovertargetaction="hide" onclick={() => openModal(guide)}>{@render icon(ICON.help)}{t("guideOpen")}</button>
      <button role="menuitem" popovertarget="app-menu" popovertargetaction="hide" disabled={busy} onclick={checkUpdate}>{@render icon(ICON.update)}{t("updCheck")}</button>
    </div>
    <button class="rail-btn" title={t("tabMods")} aria-label={t("tabMods")} aria-current={tab === "mods" ? "page" : undefined} onclick={() => (tab = "mods")}>{@render icon(ICON.mods)}</button>
    <button class="rail-btn" title={t("tabNsz")} aria-label={t("tabNsz")} aria-current={tab === "nsz" ? "page" : undefined} disabled={!emuDir} onclick={showNsz}>{@render icon(ICON.nsz)}</button>
    <span class="spacer"></span>
    <button class="rail-btn" class:spin={loadingCatalog} title={loadingCatalog ? t("refreshing") : t("refreshCatalog")} aria-label={t("refreshCatalog")} disabled={busy || !emuDir} aria-busy={loadingCatalog} onclick={() => loadCatalog(true)}>{@render icon(ICON.refresh)}</button>
  </nav>
  <div class="content">
    <header class="topbar">
      <h1>{tab === "nsz" ? t("tabNsz") : t("tabMods")}</h1>
      <div class="seg" role="group" aria-label={t("emulator")}>
        {#each Object.entries(EMU_NAME) as [k, name] (k)}
          <button aria-pressed={emu === k} disabled={busy} onclick={() => emu !== k && switchEmu(k as Emu)}>{name}</button>
        {/each}
      </div>
      <span class="spacer"></span>
      {#if catalog}
        <span class="muted small">{t("catalogFrom", { date: new Date(catalog.fetchedAt * 1000).toLocaleDateString(locale()) })}</span>
      {/if}
      <span class="path muted small" title={emuDir ?? ""}>{t("edenLabel", { path: emuDir ?? t("notFound") })}</span>
      <button class="link" onclick={() => changeDir()}>{t("change")}</button>
    </header>

    {#if !emuDir}
      <div class="empty panel">
        <p>{t("noEden")}</p>
        <p class="muted">{t("noEdenHint", { path: EMU_HINT[emu] })}</p>
        <button class="primary" onclick={() => changeDir()}>{t("pickEden")}</button>
      </div>
    {:else if tab === "nsz"}
      <section class="nsz">
        <div class="toolbar">
          <label class="search">
            {@render icon(ICON.search)}
            <input type="search" placeholder={t("filterRoms")} aria-label={t("filterRoms")} bind:value={romFilter} />
          </label>
          <span class="spacer"></span>
          <button class="primary" disabled={busy} onclick={pickRom}>{@render icon(ICON.plus)}{t("pickFile")}</button>
        </div>
        <div class="panel nsz-head">
          <details class="what"><summary><span class="chev">{@render icon(ICON.chev)}</span>{t("nszWhat")}</summary><p class="muted">{emu === "eden" ? t("nszIntro") : t("nszIntroOther")}</p></details>
          <label class="del"><input type="checkbox" bind:checked={deleteSource} /> {t("deleteOriginal")}
            {#if deleteSource}<span class="muted"> — {t("quickVerifyHint")}</span>{/if}</label>
          {#if romFilter}<p class="muted small">{t("romCount", { shown: shownRoms.length, total: romList.length })}</p>{/if}
          {#if nszCur}
            <div
              class="running"
              role="progressbar"
              aria-label={t("nszProgress")}
              aria-valuemin="0"
              aria-valuemax="100"
              aria-valuenow={nszStep?.percent ?? undefined}
              aria-valuetext={`${runVerb(nszCur.op)} ${cleanName(nszCur.name)}`}
            >
              <div class="run-top">
                <strong>{runVerb(nszCur.op)}</strong>
                <span class="run-name">{cleanName(nszCur.name)}</span>
                <span class="muted">{stageText}</span>
                <span class="spacer"></span>
                <span class="muted">{elapsed}</span>
              </div>
              <div class="track">
                <div class="fill" class:indet={nszStep?.percent == null} style:width={nszStep?.percent == null ? undefined : `${nszStep.percent}%`}></div>
              </div>
            </div>
          {/if}
          {#if nszLog}<pre class="log" use:stick={nszLog}>{nszLog}</pre>{/if}
        </div>
        <div class="nsz-list">
          {#each groups as g (g.key)}
            <details class="panel group" open={!!romFilter}>
              <summary class="panel-head">
                <span class="chev">{@render icon(ICON.chev)}</span>
                <span class="avatar" aria-hidden="true">{#if romCover(g.key)}<img src={romCover(g.key)} alt="" />{:else}{initials(g.name)}{/if}</span>
                <span class="gname">{g.name}</span>
                <span class="count">{g.items.length}</span>
                <span class="muted small">{fmtSize(g.size)}</span>
              </summary>
              {#each g.items as r (r.path)}
                <div class="row" title={r.path}>
                  <div class="info">
                    <div class="name">
                      {#if kindOf(r.name)}<span class="badge tag">{t(kindOf(r.name)!)}</span>{/if}{itemName(r, g.name)}
                    </div>
                    {#if nszCur?.path === r.path}
                      <div class="sub run">{@render icon(ICON.refresh)}{runVerb(nszCur.op)}… {elapsed}</div>
                    {:else if nszRes[r.path]}
                      <div class="sub" class:bad={!nszRes[r.path].ok}>{@render icon(nszRes[r.path].ok ? ICON.check : ICON.x)}{nszRes[r.path].text}</div>
                    {:else}
                      <div class="sub">{r.ext.toUpperCase()}{r.size ? ` · ${fmtSize(r.size)}` : ""}</div>
                    {/if}
                  </div>
                  {#if canVerify}<button class="quiet" disabled={busy} onclick={() => nsz("verify", r)}>{t("verify")}</button>{/if}
                  <button class="quiet" disabled={busy} onclick={() => nsz("info", r)}>{t("info")}</button>
                  {#if r.ext === "nsp" || r.ext === "xci"}
                    <button class="act" disabled={busy} onclick={() => nsz("compress", r)}>{t("compress")}</button>
                  {:else if r.ext === "nsz" || r.ext === "xcz"}
                    <button class="act" disabled={busy} onclick={() => nsz("decompress", r)}>{t("decompress")}</button>
                  {/if}
                </div>
              {/each}
            </details>
          {:else}
            <div class="hint">{@render icon(ICON.nsz)}<p>{t("noRoms")}</p></div>
          {/each}
        </div>
      </section>
    {:else}
      <main>
        <aside class="panel games">
          <h3 class="panel-head">{t("games")} <span class="count">{filteredGames.length}</span></h3>
          <label class="search">
            {@render icon(ICON.search)}
            <input type="search" placeholder={t("filterGames")} aria-label={t("filterGames")} bind:value={gameFilter} />
          </label>
          <div class="list">
            {#each filteredGames as g (g.tid)}
              <button
                class="game"
                class:sel={selected?.tid === g.tid}
                aria-current={selected?.tid === g.tid ? "true" : undefined}
                onclick={() => select(g)}
              >
                <span class="avatar" aria-hidden="true">{#if g.icon}<img src={g.icon} alt="" />{:else}{initials(g.name ?? g.tid)}{/if}</span>
                <span class="info">
                  <span class="name" title={g.name ?? g.tid}>{g.name ?? g.tid}</span>
                  <span class="sub"><span title={t("tidTitle")}>{g.tid}</span>{#if g.version}<span class="badge">{g.version}</span>{/if}</span>
                </span>
              </button>
            {:else}
              <p class="muted pad">{t("noGames")}</p>
            {/each}
          </div>
        </aside>

        <div class="pane">
        {#if selected}
          <div class="filters">
            <label class="search">
              {@render icon(ICON.search)}
              <input type="search" placeholder={t("filterMods")} aria-label={t("filterMods")} bind:value={search} />
            </label>
            <div class="seg" role="group" aria-label={t("srcAll")}>
              {#each ["all", "official", "theboy181", "wiki", "ptbr"] as const as s (s)}
                <button aria-pressed={srcFilter === s} onclick={() => (srcFilter = s)}>{s === "all" ? t("srcAll") : s === "official" ? t("srcOfficial") : SOURCE_LABEL[s]}</button>
              {/each}
            </div>
            {#if selected.version}
              <label class="toggle"><input type="checkbox" bind:checked={onlyMatch} /> {t("onlyMatch")} ({selected.version})</label>
            {/if}
          </div>
        {/if}
        <div class="detail">
          {#if !selected}
            <div class="hint">{@render icon(ICON.mods)}<p>{t("selectGame")}</p></div>
          {:else}
            <div class="head">
              {#if selected.icon}<img class="cover" src={selected.icon} alt="" />{/if}
              <h2>{selected.name ?? selected.tid}</h2>
            </div>

            <section class="panel">
              <h3 class="panel-head">{t("installed")} <span class="count">{installed.length}</span><span class="spacer"></span><button class="icon-btn" aria-label={t("openModFolder")} title={t("openModFolder")} onclick={() => api.openModFolder(selected!.tid)}>{@render icon(ICON.folder)}</button></h3>
              {#each installed as i (i.folder)}
                {@const up = newer(i)}
                <div class="row" class:off={!i.enabled}>
                  <div class="info">
                    <div class="name" title={i.folder}>{i.folder}</div>
                    <div class="sub" title={i.modId}>{i.modId}</div>
                  </div>
                  {#if up}<span class="badge" title={t("updateAvailable", { v: up.version ?? "" })}>{i.version} → {up.version}</span>
                    <button disabled={busy} onclick={() => install(up, i.rootKey)}>{t("updateMod")}</button>{/if}
                  <button class="ghost" title={t("toggleHint")} onclick={() => toggle(i)}>{i.enabled ? t("disable") : t("enable")}</button>
                  <button class="danger" onclick={() => remove(i)}>{t("remove")}</button>
                </div>
              {:else}
                <p class="muted">{t("noInstalled")}</p>
              {/each}
              {#each conflicts as c (c.folders.join("|"))}
                <div class="row conflict" title={c.sample}>
                  <div class="info">
                    <div class="name">{t("conflict", { folders: c.folders.join(" × "), n: c.count })}</div>
                    <div class="sub">{c.sample}</div>
                  </div>
                </div>
              {/each}
            </section>

            <section class="panel">
              <h3 class="panel-head">{t("available")} <span class="count">{available.length}</span></h3>
              {#each available as m (m.id)}{@render modRow(m)}{:else}
                <p class="muted">{q || srcFilter !== "all" || onlyMatch ? t("nothingMatches") : t("noAvailable")}</p>
              {/each}
            </section>

            {#if possible.length}
              <section class="panel">
                <h3 class="panel-head">{t("possible")} <span class="count">{possible.length}</span></h3>
                {#each possible as m (m.id)}{@render modRow(m)}{/each}
              </section>
            {/if}

            {#if q && searchHits.length}
              <section class="panel">
                <h3 class="panel-head">{t("searchAll")} <span class="count">{searchHits.length}</span></h3>
                {#each searchResults as m (m.id)}{@render modRow(m)}{/each}
                {#if searchHits.length > 200}
                  <p class="muted">{t("showing", { shown: 200, total: searchHits.length })}</p>
                {/if}
              </section>
            {/if}
          {/if}
        </div>
        </div>
      </main>
    {/if}
  </div>

  <div class="toasts">
    {#if progress && busy}
      <div class="toast dl" role="progressbar" aria-label={t("downloadLabel")} aria-valuemin="0" aria-valuemax="100"
           aria-valuenow={progress.total ? Math.round((progress.received / progress.total) * 100) : undefined}>
        <div class="run-top small">
          <span>{t("downloadLabel")}</span><span class="spacer"></span>
          <span class="muted">{fmtSize(progress.received)}{progress.total ? ` / ${fmtSize(progress.total)}` : ""}</span>
        </div>
        <div class="track"><div class="fill" class:indet={!progress.total} style:width={progress.total ? `${Math.min(100, (progress.received / progress.total) * 100)}%` : undefined}></div></div>
      </div>
    {/if}
    <div role="alert">{#if error}<div class="toast err"><span class="ico">{@render icon(ICON.alert)}</span>{error}<button class="x" aria-label={t("dismiss")} onclick={() => (error = "")}>{@render icon(ICON.x)}</button></div>{/if}</div>
    <div role="status">{#if notice}<div class="toast ok"><span class="ico">{@render icon(ICON.check)}</span>{notice}<button class="x" aria-label={t("dismiss")} onclick={() => (notice = "")}>{@render icon(ICON.x)}</button></div>{/if}</div>
  </div>


  <dialog bind:this={dlg} class="modal" aria-labelledby="dlg-title" tabindex="-1" oncancel={(e) => { e.preventDefault(); cancel(); }}>
    {#if prepared}
      <h3 id="dlg-title">{t("chooseTitle")}</h3>
      <p class="muted">{t("chooseHint")}</p>
      <div class="rootlist">
        {#each prepared.roots as r (r.key)}
          <label><input type="checkbox" bind:checked={checked[r.key]} /> {r.name}
            <span class="muted">{t("filesCount", { n: r.fileCount })}</span></label>
        {/each}
      </div>
      <div class="actions">
        <button class="ghost" onclick={cancel}>{t("cancel")}</button>
        <button
          class="primary"
          disabled={!prepared.roots.some((r) => checked[r.key])}
          onclick={() => commit(prepared!.token, prepared!.roots.filter((r) => checked[r.key]).map((r) => r.key))}
        >{t("installSelected")}</button>
      </div>
    {/if}
  </dialog>

  <dialog bind:this={guide} class="modal guide" aria-labelledby="guide-title" tabindex="-1" onclose={() => localStorage.setItem("guideSeen", "1")}>
    <h3 id="guide-title">{t("guideTitle")}</h3>
    <ol>
      {#each [["g1t", "g1b"], ["g2t", "g2b"], ["g3t", "g3b"], ["g4t", "g4b"]] as const as [h, b] (h)}
        <li><b>{t(h)}</b><span class="muted">{t(b)}</span></li>
      {/each}
    </ol>
    <div class="actions"><button class="primary" onclick={() => guide?.close()}>{t("guideDone")}</button></div>
  </dialog>

  <dialog bind:this={upd} class="modal" aria-labelledby="upd-title" tabindex="-1">
    {#if update?.version}
      <h3 id="upd-title">{t("updTitle", { v: update.version })}</h3>
      <p class="muted">{t("updBody", { cur: update.current })}</p>
      {#if update.notes}<pre class="upd-notes">{update.notes}</pre>{/if}
      <div class="actions"><button class="ghost" onclick={() => upd?.close()}>{t("updLater")}</button><button class="primary" onclick={installUpdate}>{t("updNow")}</button></div>
    {/if}
  </dialog>

  <dialog bind:this={sett} class="modal settings" aria-labelledby="set-title" tabindex="-1">
    <h3 id="set-title">{t("settings")}</h3>
    <div class="sbody">
      <section class="srow">
        <h4>{t("language")}</h4>
        <div class="sctl">
          <div class="seg" role="group" aria-label={t("language")}>
            {#each [["pt", "Português"], ["en", "English"]] as const as [l, name] (l)}
              <button aria-pressed={i18n.lang === l} onclick={() => setLang(l)}>{name}</button>
            {/each}
          </div>
        </div>
      </section>
      {#if micaOk}
        <section class="srow">
          <h4>{t("setAppearance")}</h4>
          <div class="sctl">
            <label><input type="checkbox" checked={micaOn} onchange={(e) => setMica(e.currentTarget.checked)} /> {t("setMica")}</label>
          </div>
        </section>
      {/if}
      <section class="srow">
        <h4>{t("setUpdates")}</h4>
        <div class="sctl">
          <p class="muted small">{t("setVersion", { v: appVersion })}</p>
          <label><input type="checkbox" checked={autoUpdate} onchange={(e) => setAutoUpdate(e.currentTarget.checked)} /> {t("setAutoCheck")}</label>
          <button disabled={busy} onclick={checkUpdate}>{t("updCheck")}</button>
        </div>
      </section>
      <section class="srow">
        <h4>{t("setFolders")}</h4>
        <div class="sctl">
          <div class="dirs">
            {#each emuDirs as d (d.kind)}
              <div class="dirrow">
                <b>{EMU_NAME[d.kind]}</b>
                <span class="path muted small" title={d.dir ?? ""}>{d.dir ?? t("notFound")}</span>
                <button disabled={busy} onclick={() => changeDir(d.kind)}>{t("change")}</button>
              </div>
            {/each}
          </div>
        </div>
      </section>
      <section class="srow">
        <h4>{t("setCatalog")}</h4>
        <div class="sctl">
          {#if catalog}<p class="muted small">{t("catalogFrom", { date: new Date(catalog.fetchedAt * 1000).toLocaleDateString(locale()) })}</p>{/if}
          <p class="muted small">{t("setCache", { size: storage?.cacheBytes ? fmtSize(storage.cacheBytes) : "0 KB" })}</p>
          <div class="btns">
            <button disabled={busy || !emuDir} onclick={() => loadCatalog(true)}>{t("refreshCatalog")}</button>
            <button disabled={busy} onclick={clearCache}>{t("setClear")}</button>
          </div>
        </div>
      </section>
      <section class="srow">
        <h4>{t("setNsz")}</h4>
        <div class="sctl">
          <p class="muted small">{storage?.toolsBytes ? fmtSize(storage.toolsBytes) : t("setNszNone")}</p>
          <button disabled={busy || !!nszCur || !storage?.toolsBytes} onclick={removeTools}>{t("setNszRemove")}</button>
        </div>
      </section>
      <section class="srow">
        <h4>{t("setAbout")}</h4>
        <div class="sctl">
          <div class="btns">
            <button onclick={() => openUrl("https://github.com/pendiego/Eden-Mod-Manager")}>{t("setRepo")}</button>
            <button onclick={() => openUrl("https://github.com/pendiego/Eden-Mod-Manager/releases/latest")}>{t("setReleases")}</button>
          </div>
          <p class="muted small">{t("setLegal")}</p>
        </div>
      </section>
    </div>
    <div class="actions">
      {#if error}<p class="serr" role="alert">{error}</p>{:else if notice}<p class="muted small" role="status">{notice}</p>{/if}
      <button class="primary" onclick={() => sett?.close()}>{t("setClose")}</button>
    </div>
  </dialog>
</div>

<style>
  :global(:root) {
    color-scheme: light dark;
    --base-a: 1;
    --base: rgb(12 12 13 / var(--base-a));
    --fg: #f1f1f2; --fg-soft: #d0d0d3; --muted: #8e8f96;
    --panel: rgb(255 255 255 / 0.025); --card: rgb(255 255 255 / 0.03); --card-hover: rgb(255 255 255 / 0.05);
    --input: rgb(255 255 255 / 0.03); --border: rgb(255 255 255 / 0.08); --border-soft: rgb(255 255 255 / 0.05);
    --primary: #f1f1f2; --on-primary: #0c0c0d;
    --ghost: rgb(255 255 255 / 0.06); --ghost-hover: rgb(255 255 255 / 0.11); --ghost-fg: #f1f1f2;
    --accent: #f1f1f2; --focus: #9cc2ff;
    --danger: #ef6b63; --danger-bg: rgb(229 83 75 / 0.16); --ok: #4ac26b; --ok-bg: rgb(74 194 107 / 0.14);
    --sel: rgb(255 255 255 / 0.09); --badge: rgb(255 255 255 / 0.08); --badge-ok: rgb(74 194 107 / 0.22);
    --toast: rgb(24 25 29 / 0.92); --backdrop: rgb(0 0 0 / 0.45); --shadow: 0 12px 32px rgb(5 8 20 / 0.45); --scroll: rgb(255 255 255 / 0.16);
  }
  :global(:root[data-mica]) { --base-a: 0.5; }
  @media (prefers-color-scheme: light) {
    :global(:root) {
      --base: rgb(242 243 246 / var(--base-a));
      --fg: #15171c; --fg-soft: #2f333c; --muted: #565c69;
      --panel: rgb(255 255 255 / 0.55); --card: rgb(255 255 255 / 0.85); --card-hover: rgb(255 255 255 / 1);
      --input: rgb(255 255 255 / 0.9); --border: rgb(0 0 0 / 0.09); --border-soft: rgb(0 0 0 / 0.05);
      --primary: #15171c; --on-primary: #ffffff;
      --ghost: rgb(0 0 0 / 0.06); --ghost-hover: rgb(0 0 0 / 0.1); --ghost-fg: #15171c;
      --accent: #1a56db; --focus: #1a4fd6;
      --danger: #b3261e; --danger-bg: rgb(179 38 30 / 0.1); --ok: #1d7a3e; --ok-bg: rgb(29 122 62 / 0.1);
      --sel: rgb(0 0 0 / 0.07); --badge: rgb(0 0 0 / 0.07); --badge-ok: rgb(29 122 62 / 0.15);
      --toast: rgb(255 255 255 / 0.95); --backdrop: rgb(0 0 0 / 0.25); --shadow: 0 12px 32px rgb(40 60 110 / 0.14); --scroll: rgb(0 0 0 / 0.18);
    }
    :global(:root[data-mica]) { --base-a: 0.6; }
  }
  /* Windows → Efeitos de transparência desligado: volta ao fundo opaco */
  @media (prefers-reduced-transparency: reduce) { :global(:root[data-mica]) { --base-a: 0.94; } }

  :global(:focus-visible) { outline: 2px solid var(--focus); outline-offset: 2px; }
  :global(::-webkit-scrollbar) { width: 10px; height: 10px; }
  :global(::-webkit-scrollbar-thumb) { background: var(--scroll); border-radius: 5px; border: 3px solid transparent; background-clip: padding-box; }
  :global(::-webkit-scrollbar-button) { display: none; }
  :global(html), :global(body) { background: transparent; }
  :global(body) { margin: 0; font: 14px/1.45 "Geist Variable", "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif; letter-spacing: -0.006em; color: var(--fg); height: 100vh; overflow: hidden; }
  :global(#svelte) { height: 100%; }

  .app { display: flex; height: 100vh; background-color: var(--base); }
  .rail { display: flex; flex-direction: column; align-items: center; gap: 6px; width: 56px; flex-shrink: 0; padding: 12px 0; box-sizing: border-box; border-right: 1px solid var(--border); }
  .logo { width: 32px; height: 32px; display: block; }
  .rail-btn { width: 38px; height: 38px; justify-content: center; padding: 0; background: transparent; color: var(--muted); border-radius: 10px; }
  .rail-btn:hover:not(:disabled) { background: var(--ghost); color: var(--fg); }
  .rail-btn[aria-current="page"] { background: var(--sel); color: var(--fg); }
  .rail-btn.spin :global(.icon) { animation: rot 1s linear infinite; }
  @keyframes rot { to { transform: rotate(360deg); } }
  .content { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; }
  .topbar { display: flex; align-items: center; gap: 14px; height: 52px; padding: 0 20px; box-sizing: border-box; border-bottom: 1px solid var(--border); flex-shrink: 0; }
  h1 { margin: 0; font-size: 15px; font-weight: 600; letter-spacing: -0.01em; }
  .seg { display: flex; gap: 2px; padding: 2px; border-radius: 8px; background: var(--input); border: 1px solid var(--border); }
  .seg button { background: transparent; color: var(--muted); border-radius: 6px; padding: 3px 12px; font-size: 12px; font-weight: 500; }
  .seg button[aria-pressed="true"] { background: var(--sel); color: var(--fg); }
  .topbar .path { max-width: 34%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .link { background: none; color: var(--fg-soft); padding: 0; text-decoration: underline; text-underline-offset: 2px; font-size: 12px; }
  .spacer { flex: 1; }
  .muted { color: var(--muted); }
  .small { font-size: 12px; }
  .pad { padding: 12px; }
  .hint { margin: auto; display: flex; flex-direction: column; align-items: center; gap: 10px; color: var(--muted); }
  .hint p { margin: 0; }
  .hint :global(.icon) { width: 28px; height: 28px; opacity: 0.6; }
  .icon { width: 16px; height: 16px; flex-shrink: 0; }

  input:not([type="checkbox"]) { background: var(--input); color: inherit; border: 1px solid var(--border); border-radius: 8px; padding: 7px 10px; box-sizing: border-box; width: 100%; font: inherit; }
  .search { display: flex; align-items: center; gap: 8px; width: min(360px, 100%); height: 34px; padding: 0 10px; box-sizing: border-box; border-radius: 8px; background: var(--input); border: 1px solid var(--border); color: var(--muted); flex-shrink: 0; }
  .search:focus-within { outline: 2px solid var(--focus); outline-offset: 2px; }
  .search input { flex: 1; min-width: 0; height: 100%; border: 0; background: transparent; padding: 0; border-radius: 0; }
  .search input:focus-visible { outline: none; }

  button { display: inline-flex; align-items: center; gap: 6px; background: var(--ghost); color: var(--ghost-fg); border: 0; border-radius: 8px; padding: 6px 12px; cursor: pointer; font: inherit; white-space: nowrap; transition: background-color 0.15s, color 0.15s, transform 0.1s; }
  button:active:not(:disabled) { transform: translateY(1px); }
  input[type="checkbox"] { accent-color: var(--fg); width: 15px; height: 15px; margin: 0; flex-shrink: 0; cursor: pointer; }
  label:has(> input[type="checkbox"]) { display: flex; align-items: center; gap: 8px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--ghost-hover); }
  button:disabled { opacity: 0.4; cursor: not-allowed; }
  button.primary { background: var(--primary); color: var(--on-primary); font-weight: 600; padding: 7px 14px; }
  button.primary:hover:not(:disabled) { background: var(--primary); filter: brightness(0.9); }
  button.ghost { background: transparent; color: var(--muted); }
  button.ghost:hover:not(:disabled) { background: var(--ghost); color: var(--fg); }
  button.danger { background: transparent; color: var(--danger); }
  button.danger:hover:not(:disabled) { background: var(--danger-bg); }
  button.icon-btn { padding: 4px; background: transparent; color: var(--muted); }

  /* Secoes: caixa com borda fina, linhas divididas */
  .panel { background: var(--panel); border: 1px solid var(--border); border-radius: 12px; overflow: clip; }
  .panel > p.muted { margin: 0; padding: 14px 16px; }
  .panel-head { display: flex; align-items: center; gap: 8px; margin: 0; padding: 12px 16px; border-bottom: 1px solid var(--border); font-size: 13px; font-weight: 500; color: var(--muted); }
  .count { min-width: 18px; padding: 0 6px; border-radius: 9px; background: var(--badge); color: var(--fg-soft); font-size: 11px; font-weight: 600; line-height: 18px; text-align: center; font-variant-numeric: tabular-nums; }
  .row { display: flex; align-items: center; gap: 10px; padding: 10px 16px; border-bottom: 1px solid var(--border-soft); transition: background-color 0.15s; }
  .row:last-child { border-bottom: 0; }
  .row:hover { background: var(--ghost); }
  .row.sm { padding: 6px 16px 6px 32px; font-size: 13px; }
  .row.off .info { opacity: 0.5; }
  .row.conflict .name { color: var(--danger); }
  .subrows { background: var(--input); border-bottom: 1px solid var(--border-soft); }

  /* Sidebar de jogos + detalhe */
  main { display: grid; grid-template-columns: 300px 1fr; flex: 1; min-height: 0; }
  .games { display: flex; flex-direction: column; min-height: 0; border: 0; border-right: 1px solid var(--border); border-radius: 0; background: transparent; }
  .games .panel-head { padding: 14px 16px 10px; border: 0; color: var(--fg); font-size: 14px; font-weight: 600; }
  .games .search { width: auto; margin: 0 12px 10px; }
  .list { display: flex; flex-direction: column; gap: 1px; overflow-y: auto; min-height: 0; padding: 0 8px 12px; }
  .pane { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .filters { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; padding: 12px 28px; border-bottom: 1px solid var(--border); flex-shrink: 0; }
  .filters .search { flex: 1; min-width: 180px; width: auto; max-width: 320px; }
  .toggle { font-size: 12px; color: var(--fg-soft); }
  .detail { display: flex; flex-direction: column; gap: 20px; overflow-y: auto; flex: 1; min-height: 0; padding: 22px 28px 32px; }
  .detail h2 { margin: 0; font-size: 22px; font-weight: 600; letter-spacing: -0.02em; line-height: 1.15; text-wrap: balance; }
  .detail .panel-head { color: var(--fg); }
  .game { display: flex; align-items: center; gap: 10px; width: 100%; text-align: left; background: transparent; color: var(--muted); border-radius: 8px; padding: 6px 8px; }
  .game:hover:not(:disabled) { background: var(--ghost); color: var(--fg); }
  .game.sel { background: var(--sel); color: var(--fg); }
  .avatar { width: 28px; height: 28px; border-radius: 6px; display: grid; place-items: center; flex-shrink: 0; background: var(--badge); color: var(--fg-soft); font-size: 11px; font-weight: 600; overflow: hidden; }
  .avatar img, img.cover { width: 100%; height: 100%; object-fit: cover; display: block; }
  .game .avatar, .group .avatar { width: 36px; height: 36px; }
  .head { display: flex; align-items: center; gap: 14px; }
  .head img.cover { width: 56px; height: 56px; border-radius: 10px; flex-shrink: 0; }
  .info { flex: 1; min-width: 0; }
  .name, .sub { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .name { font-weight: 500; }
  .sub { font-size: 12px; color: var(--muted); }
  .sub :global(.icon) { width: 12px; height: 12px; vertical-align: -2px; margin-right: 4px; }
  .sub.run :global(.icon) { animation: rot 1s linear infinite; }
  .sub .src { border: 1px solid var(--border); border-radius: 4px; padding: 0 5px; margin-right: 4px; }
  .sub.bad { color: var(--danger); }
  .badge { background: var(--badge); color: var(--fg-soft); border-radius: 4px; padding: 1px 6px; font-size: 12px; margin-left: 6px; font-variant-numeric: tabular-nums; }
  .badge :global(.icon) { width: 11px; height: 11px; vertical-align: -1px; margin-right: 3px; }
  .badge.ok { background: var(--badge-ok); color: var(--fg); }
  .badge.tag { margin: 0 8px 0 0; font-size: 11px; }
  .size { color: var(--muted); font-size: 12px; min-width: 60px; text-align: right; font-variant-numeric: tabular-nums; }
  .empty { margin: 12vh auto 0; max-width: 460px; display: flex; flex-direction: column; align-items: center; gap: 10px; padding: 28px; text-align: center; text-wrap: balance; }

  .nsz { display: flex; flex-direction: column; gap: 16px; padding: 20px 28px 24px; flex: 1; min-height: 0; }
  .toolbar { display: flex; align-items: center; gap: 10px; flex-shrink: 0; }
  .nsz-head { flex-shrink: 0; padding: 14px 16px; }
  .nsz-head > p { margin: 6px 0 0; }
  .what { font-size: 13px; color: var(--muted); }
  .what summary { cursor: pointer; display: inline-flex; align-items: center; gap: 4px; list-style: none; }
  .what summary::-webkit-details-marker { display: none; }
  .what[open] .chev { transform: rotate(90deg); }
  .what p { margin: 6px 0 0; }
  .del { padding: 8px 0 0; }
  .nsz-list { display: flex; flex-direction: column; gap: 12px; overflow-y: auto; flex: 1; min-height: 0; }
  .group > summary { list-style: none; cursor: pointer; color: var(--fg); }
  .group > summary::-webkit-details-marker { display: none; }
  .group:not([open]) > summary { border-bottom: 0; }
  .chev { display: inline-flex; color: var(--muted); transition: transform 0.15s; }
  .group[open] > summary .chev { transform: rotate(90deg); }
  .gname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .log { background: var(--input); padding: 10px; border-radius: 8px; white-space: pre-wrap; font-size: 12px; max-height: 120px; overflow: auto; margin: 8px 0 0; }
  .running { margin: 10px 0 0; padding: 10px 12px; border-radius: 8px; background: var(--input); border: 1px solid var(--border); }
  .run-top { display: flex; align-items: baseline; gap: 8px; margin-bottom: 6px; min-width: 0; }
  .run-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .track { height: 4px; border-radius: 2px; background: var(--border); overflow: hidden; }
  .fill { height: 100%; background: var(--fg); transition: width 0.3s; }
  .fill.indet { width: 35%; animation: slide 1.2s ease-in-out infinite alternate; }
  @keyframes slide { from { margin-left: 0; } to { margin-left: 65%; } }
  .nsz .quiet { background: transparent; color: var(--muted); opacity: 0; }
  .nsz .row:hover .quiet, .nsz .row:focus-within .quiet { opacity: 1; }
  .nsz .quiet:hover { color: var(--fg); background: var(--ghost); }
  .nsz .act { min-width: 108px; justify-content: center; background: transparent; color: var(--fg); border: 1px solid var(--border); }
  .nsz .act:hover:not(:disabled) { background: var(--ghost); }

  .toasts { position: fixed; right: 20px; bottom: 20px; z-index: 10; display: flex; flex-direction: column; align-items: flex-end; gap: 8px; width: min(440px, calc(100vw - 40px)); pointer-events: none; }
  .toasts > * { pointer-events: auto; width: 100%; }
  .toast { display: flex; align-items: center; gap: 10px; box-sizing: border-box; width: 100%; padding: 10px 12px 10px 16px; border-radius: 12px; background: var(--toast); color: var(--fg); border: 1px solid var(--border); box-shadow: var(--shadow); backdrop-filter: blur(16px); }
  .toast { transition: opacity 0.16s ease, transform 0.16s ease; }
  @starting-style { .toast { opacity: 0; transform: translateY(8px); } }
  .toast.err { border-color: var(--danger); box-shadow: 0 0 0 3px var(--danger-bg), var(--shadow); }
  .toast.err .ico { color: var(--danger); }
  .toast.ok { border-color: var(--ok); box-shadow: 0 0 0 3px var(--ok-bg), var(--shadow); }
  .toast.ok .ico { color: var(--ok); }
  .toast.dl { flex-direction: column; align-items: stretch; gap: 0; border-color: var(--accent); }
  .toast .x { margin-left: auto; background: transparent; color: inherit; padding: 4px; display: inline-flex; }
  .ico { display: inline-flex; }

  .modal { background: var(--toast); color: var(--fg); border: 1px solid var(--border); border-radius: 16px; padding: 20px 22px; width: min(520px, 90vw); max-height: 80vh; box-shadow: var(--shadow); }
  .modal[open] { display: flex; flex-direction: column; }
  .modal::backdrop { background: var(--backdrop); backdrop-filter: blur(4px); }
  .modal[open], .menu:popover-open { transition: opacity 0.16s ease, transform 0.16s ease; }
  @starting-style { .modal[open], .menu:popover-open { opacity: 0; transform: translateY(6px) scale(0.98); } }
  .modal h3 { margin: 0 0 4px; font-size: 16px; }
  .rootlist { overflow-y: auto; display: flex; flex-direction: column; gap: 6px; margin: 8px 0; }
  .rootlist label { padding: 8px 10px; border-radius: 8px; background: var(--input); border: 1px solid var(--border-soft); }
  .rootlist label .muted { margin-left: auto; font-size: 12px; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; }
  .guide ol { margin: 14px 0 18px; padding: 0; list-style: none; counter-reset: step; display: flex; flex-direction: column; gap: 14px; overflow-y: auto; }
  .guide li { counter-increment: step; display: grid; grid-template-columns: 24px 1fr; column-gap: 12px; }
  .guide li::before { content: counter(step); grid-row: span 2; width: 24px; height: 24px; border-radius: 50%; background: var(--badge); color: var(--fg-soft); font-size: 12px; font-weight: 600; display: grid; place-items: center; }
  .guide li > * { grid-column: 2; }
  .guide li b { font-weight: 600; }
  .upd-notes { max-height: 240px; overflow-y: auto; white-space: pre-wrap; font: inherit; font-size: 13px; margin: 8px 0 16px; padding: 10px 12px; background: var(--input); border: 1px solid var(--border-soft); border-radius: 8px; }
  .logo-btn { width: 38px; height: 38px; justify-content: center; padding: 0; margin-bottom: 10px; background: transparent; border-radius: 10px; }
  .logo-btn:hover:not(:disabled) { background: var(--ghost); }
  .menu { position: fixed; inset: auto; top: 12px; left: 62px; margin: 0; padding: 6px; min-width: 210px; background: var(--toast); color: var(--fg); border: 1px solid var(--border); border-radius: 12px; box-shadow: var(--shadow); }
  .menu:popover-open { display: flex; flex-direction: column; gap: 2px; }
  .menu button { background: transparent; justify-content: flex-start; width: 100%; padding: 8px 10px; font-size: 13px; color: var(--fg); }
  .menu button:hover { background: var(--ghost); }
  .settings { width: min(680px, 92vw); max-height: 86vh; }
  .modal:focus-visible { outline: none; }
  .sbody { overflow-y: auto; display: flex; flex-direction: column; margin: 8px -22px 16px; padding: 0 22px; }
  .srow { display: grid; grid-template-columns: 150px 1fr; gap: 16px; padding: 14px 0; border-bottom: 1px solid var(--border-soft); }
  .srow:last-child { border-bottom: 0; }
  .srow h4 { margin: 0; padding-top: 4px; font-size: 13px; font-weight: 500; color: var(--fg-soft); }
  .sctl { display: flex; flex-direction: column; align-items: flex-start; gap: 8px; min-width: 0; }
  .sctl p { margin: 0; }
  .settings .actions { align-items: center; }
  .settings .actions p { margin: 0 auto 0 0; }
  .dirs { display: flex; flex-direction: column; gap: 8px; width: 100%; }
  .dirrow { display: flex; align-items: center; gap: 10px; }
  .dirrow b { width: 64px; flex-shrink: 0; font-weight: 600; }
  .dirrow .path { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .btns { display: flex; flex-wrap: wrap; gap: 8px; }
  .serr { color: var(--danger); font-size: 12px; }

  @media (forced-colors: active) {
    button, .badge, .row, .toast, .panel, .game, .search { border: 1px solid CanvasText; }
    .game.sel, .rail-btn[aria-current="page"], .seg button[aria-pressed="true"] { outline: 2px solid Highlight; }
    .fill { background: Highlight; }
  }
  @media (prefers-reduced-motion: reduce) { .fill.indet { animation: none; width: 100%; opacity: 0.5; } .rail-btn.spin :global(.icon), .sub.run :global(.icon) { animation: none; } .chev, button, .row, .modal, .menu, .toast { transition: none; } button:active { transform: none; } }
</style>
