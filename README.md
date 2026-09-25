# Deathloop Mod Manager

Install mods, choose a setup, and launch Deathloop. Supports the inspected Steam and Epic Windows 1.820.5.1 builds. Individual mods may support fewer builds.

## Getting started

1. Extract the manager ZIP and open **deathloop-mod-manager.exe**.
2. Click **Browse** and select your **Deathloop.exe**.
3. Click **Install mod** and select a mod ZIP or its extracted **mod.toml**.
4. Check the mods you want, then click **Launch game**.

Mods are downloaded separately. Installing a mod adds it to the list; checking its box enables it. Required dependencies must also be installed and enabled.

## Changing your setup

Changes apply on the next launch. Close the game before launching with a different setup. To play without mods, uncheck all mods and launch through the manager.

Profiles save different selections of mods. Use **Edit profiles** to create, duplicate, rename or delete them. Updating a mod updates it for every profile that uses it.

To remove a mod, uncheck it in every profile, then click **Remove**. Removed packages are kept in the manager's **removed-mods** folder for recovery. To replace a mod, remove the old package, install the new one, and enable it again.

## Multiplayer

Players need matching enabled mods, versions and load order. Modded random matchmaking is separate from vanilla matchmaking. Steam and Epic players can use the same setup when all enabled mods support both stores. Profile names and installation paths do not need to match.

If players cannot match:

1. Check that everyone enabled the same mod versions and restarted through the manager.
2. Open **Tools → Logs** and compare the latest launch logs. The **Matchmaking pool** should match. Different package checksums mean the mod files differ; reinstall the same packages.
3. Check for activation errors and normal network connectivity. Matching setups do not guarantee an available opponent.

Mixed vanilla/modded sessions are not supported by this release.

## Troubleshooting

- **Access denied:** run the manager as administrator if Windows blocks access to the game folder.
- **Another startup DLL detected:** another loader occupies the same filename. Back it up before removing it or changing loaders.
- **Mod fails to load:** check **Tools → Logs**, the mod's supported game version, and its dependencies.
- **Game already running:** close it before launching another setup.
- **Startup stack overflow (`0xC00000FD`):** use manager 0.4.1 or later. Its startup DLL no longer places large temporary buffers on the game's startup thread stack. Do not modify `Deathloop.exe` to change its stack settings. If the error persists, send the startup log and crash dump so the call stack can be inspected.

Startup support is installed automatically. The manager does not replace **Deathloop.exe** or edit saves itself; individual mods can affect game progress. Install only mods you trust and keep save backups when experimenting.

Third-party license notices are included in **licenses**. Developer documentation is in **DEVELOPMENT.md** in the source repository.
