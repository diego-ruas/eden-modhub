# User guide

This guide covers setup, mod installation, supported archive layouts and common problems.

## Contents

- [First start](#first-start)
- [Install a mod](#install-a-mod)
- [Supported mod packages](#supported-mod-packages)
- [Manage installed mods](#manage-installed-mods)
- [Smash Ultimate dependencies](#smash-ultimate-dependencies)
- [Settings and data folders](#settings-and-data-folders)
- [NSZ tools](#nsz-tools)
- [Troubleshooting](#troubleshooting)

## First start

1. Select Eden, yuzu or Ryujinx at the top of the app.
2. Select a game from the list.
3. If the app uses the wrong emulator folder, click **Change** and select the correct folder.
4. Choose a mod source and install a mod.
5. Enable the mod in the emulator under *Configure game → Add-Ons*.

The app reads the game list from the selected emulator. The **Play** button starts a game in that emulator.

The app can ask you to select the emulator executable or game file if it cannot find one. It finds `.nsp` and `.xci` files by the title ID in the file name. Only base games are listed in the sidebar (standalone updates and DLCs are excluded). If a game is compressed (`.nsz`/`.xcz`), the app shows an **NSZ** badge and a banner suggesting decompression with a direct shortcut to the NSZ tab. Game updates (`.nsp`) in your game folders are automatically detected and registered in the emulator configuration.

The macOS build is not notarized. Allow it in System Settings on first launch.

## Install a mod

### From the repository

Open the Repository tab and select a game. It combines the official database, TheBoy181, the Switch Mods Wiki Archive and the PT-BR translation pack.

Use search, source and game-version filters to find a mod.

Click **Install**. Choose the variants you want when the installer asks. Then enable the mod under *Configure game → Add-Ons* in the emulator.

### From GameBanana

Open the GameBanana tab for a selected game. Select a mod to read its details, then click **Install**.

GameBanana mods are uploaded by users. The app does not review these files. Install files only from authors you trust.

The app skips GameBanana uploads whose names end in `_tkcl.zip`. It selects the newest remaining `.zip`, `.7z` or `.rar` upload. The archive must still use a layout that the app recognizes.

### From a local archive

Open the Installed panel and click **+**. Select a `.zip`, `.7z` or `.rar` file. The app extracts the archive and looks for a supported mod layout.

The archive extension alone does not show whether the contents are supported. See [Supported mod packages](#supported-mod-packages).

## Supported mod packages

The app prepares mod files for the selected game. The emulator loads those files. The app does not install a separate LayeredFS loader or support every game-specific mod format.

### Standard game mods

The installer recognizes archives with `romfs`, `exefs`, `romfslite`, `romfs_ext` or `cheats` content. It also recognizes `.pchtxt` and `.ips` patch files as ExeFS content.

The archive can include a parent folder around these paths. The installer finds the known content folders inside the archive. It does not guess raw folder layouts for other games.

### Mario Kart 8 Deluxe

Some Mario Kart 8 Deluxe mods contain raw `Audio`, `Course`, `Driver`, `Kart` or `UI` folders. The app places these files under `romfs` for Mario Kart 8 Deluxe.

### Super Smash Bros. Ultimate

The app supports ARCropolis mod packages for Smash Ultimate on Eden and Ryujinx. Yuzu is not supported for ARCropolis.

Other archive layouts can fail with an unrecognized-layout error. Open the mod page and download a package for a supported format. TKMM-only `.tkcl` packages are not supported.

Some games need loaders or other tools that the app does not provide. Check the mod page for its requirements.

## Manage installed mods

- Use the Installed panel to update or remove a mod.
- Select a mod variant when the installer asks which files to install.
- Use the enable or disable control to change a mod without removing it.
- Review conflict notices when two mods contain the same game files.

Close the emulator before you enable, disable or remove mods. Some emulators do not reload mod files while a game is running.

## Smash Ultimate dependencies

Some Smash Ultimate mods need Skyline and ARCropolis. The app shows a notice on the Repository and GameBanana tabs when you select Smash Ultimate.

Open **External dependencies** to review the detected state and install missing files. The app downloads the latest Skyline and ARCropolis releases. It does not replace files that are already present.

Use **Install Skyline + ARCropolis** to install missing files.

The app checks for these files under the emulated SD root:

```text
atmosphere/contents/01006A800016E000/exefs/subsdk9
atmosphere/contents/01006A800016E000/romfs/skyline/plugins/libarcropolis.nro
```

The app installs ARCropolis mods under `ultimate/mods/<mod>` on the emulated SD card.

Default SD roots are `<emulator data folder>/sdmc` for Eden and `<emulator data folder>/sdcard` for Ryujinx.

Close the game before you change these mods.

If needed, enable a mod in ARCropolis's in-game Mod Manager after its first discovery.

The app does not change the ARCropolis workspace selection.

For other games, the app can install Skyline when a mod includes `romfs/skyline/plugins/*.nro`. It adjusts `main.npdm` to the game's title ID. Check the mod page for other dependencies.

## Settings and data folders

Click the app icon at the top left to open Settings, the in-app Guide or the update command. The interface supports English and Portuguese.

Settings include emulator folders, cache, NSZ tools and update preferences. The app checks for updates on launch by default. You can disable this check in Settings.

Default data folders:

| Emulator | Windows | Linux | macOS |
|---|---|---|---|
| Eden and yuzu | `%APPDATA%\<name>` | `~/.local/share/<name>` | `~/Library/Application Support/<name>` |
| Ryujinx | `%APPDATA%\Ryujinx` | `~/.config/Ryujinx` | `~/Library/Application Support/Ryujinx` |

### Portable mode on Windows

Place a file named `portable` beside `EdenModManager.exe`. The app then stores its settings, cache and tools in a `data/` folder beside the executable.

## NSZ tools

The optional NSZ tool compresses NSP/XCI files and decompresses NSZ/XCZ files. It shows progress and verifies the result. The app downloads the tool when you first use it.

Configure `prod.keys` in your emulator before you use the tool. The macOS tool supports Apple Silicon only.

## Troubleshooting

### The game does not appear

Check that the selected emulator and data folder are correct. Make sure the game appears in the emulator first.

### The mod installs but does not load

Enable it in the emulator under *Configure game → Add-Ons*. Close the emulator before you change mod settings. For Smash Ultimate, also check the in-game ARCropolis Mod Manager.

### The archive has an unrecognized layout

The app supports known mod layouts, not every archive. Open the mod page and look for a package with `romfs` or `exefs` content. For Mario Kart 8 Deluxe, the supported raw folders are listed above. TKMM-only `.tkcl` packages are not supported.

### A Smash mod needs Skyline or ARCropolis

Open **External dependencies** from the Smash notice. Install the missing files, then check the mod in ARCropolis's in-game Mod Manager.
