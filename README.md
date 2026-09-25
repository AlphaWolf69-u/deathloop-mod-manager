# Deathloop Mod Manager

Install and manage Deathloop mods from one application. Supports the inspected Steam and Epic Windows 1.820.5.1 builds.
## Getting started

1. Extract the manager ZIP into a folder you can write to. Keep its files together.
2. Open **deathloop-mod-manager.exe**.
3. Click **Browse** and select `Deathloop.exe` in your game installation. You only need to do this once.
4. Click **Install mod** and select a downloaded mod ZIP.
5. Check the mods you want to enable, then click **Launch game**.

If Windows denies access to the game folder, close the manager and run it as administrator.

The manager starts the selected `Deathloop.exe` directly, without detecting or choosing a store launcher. Store login may still be required by the game itself. Launch errors are recorded under **Tools → Logs**.

## Installing mods

1. Download a mod package made for this manager, such as **AllMapsAreInvadable-1.0.2.zip**. Mods are downloaded separately from the manager.
2. Click **Install mod** and select the ZIP. There is no need to extract it or copy it into the game's installation folder. For an already extracted package, select its `mod.toml` instead.
3. The mod appears in the installed-mod list. Check its box to enable it in the selected profile, then click **Launch game**.

Installed files are stored in the manager's `mods` folder, with one subfolder per mod. Installing a package does not enable it automatically. Install any required dependencies as well.

## Deleting mods

1. Close Deathloop.
2. Uncheck the mod in every profile where it is enabled. If another mod requires it, disable that dependent mod first.
3. Click **Remove** beside the mod and confirm.

Unchecking disables a mod but keeps it installed. Removing moves its files into the manager's `removed-mods` folder for recovery. You can permanently delete those archived files later.

To replace a mod with a newer version, remove the old version using these steps, install the new package, and enable it again. The installer does not overwrite an existing mod folder.

## Enable or disable mods

Use the checkboxes beside installed mods. Your choices are saved automatically.
Changes apply on the next launch. Close Deathloop before changing the mods used by a running session. To play without mods, uncheck every mod and click **Launch game**.

## Profiles

A profile is simply a named selection of installed mods. Select a profile from the dropdown; its checkboxes are saved automatically.

Click **Edit profiles** to open the editor:

- Enter a name and choose **New** for an empty selection, or **Duplicate** to copy the current selection under that name.
- Edit the name and choose **Rename** to keep the selection under a new label.
- **Delete** removes the selected profile, not its mods. At least one profile must remain. A recoverable `.deleted` copy is kept in `profiles`.
- Click **Done** to close the editor. Your last selected profile is remembered.

Profile names do not affect matchmaking. Profiles do not pin versions or copy mod packages: updating an installed mod affects every profile using it.

## Multiplayer

Modded public matchmaking is separate from vanilla matchmaking. Players need matching enabled mods and versions to find each other. Installing a mod without enabling it does not change your matchmaking pool.

### Comparing setups when players cannot match

Each launch automatically writes a `launch-*.log` file containing the selected setup and launch result. Open **Tools → Logs** and compare the latest launch logs with the other player. Runtime logs record activation details.
1. Compare **Matchmaking pool**. Different values mean the selected setups use different public matchmaking pools.
2. Compare the enabled-mod list: mod IDs, versions, and load order must match. Disabled mods do not matter.
3. If those match but the pool differs, compare each **Package checksum**. A checksum covers functional manifest fields and exact script contents. Display names, descriptions, manifest comments and formatting are ignored. Script edits, including Lua comments, still count. Reinstall the same package on both computers if needed.
4. Compare the protocol and supported game build, and use the same manager release. Profile names and installation paths do not have to match.
5. After correcting differences, both players close Deathloop, launch through the manager, and wait for activation.

If the logs show matching setups but you still cannot connect, check activation results and normal connectivity. Matching setups do not guarantee an available matchmaking partner or a working network connection. The manager cannot automatically inspect another player's installed mods.

## Startup support

The manager installs its own startup support automatically. For modded launches, it disables the game's built-in protection before the game initializes and selects the modded matchmaking pool.
For vanilla launches, the manager removes its startup DLL from the active `dinput8.dll` filename, keeping a recoverable `dlmod-disabled-<hash>.dll` backup.

Startup checkpoints are recorded in `dlmod-startup.log` beside the game and copied into the manager launch log. Early process exits are logged; the manager allows time for a replacement process from the selected installation rather than immediately reporting a failed launch.

The manager does not edit your saves or replace `Deathloop.exe`.

Files in the game installation folder:

- `dinput8.dll`: the manager's active startup DLL. It applies startup changes only for a modded launch requested by the manager.


## Troubleshooting

- **Game is already running:** close it before starting with different mods.
- **Access denied:** run the manager as administrator.
- **Different startup DLL detected:** another loader uses the same DLL name. Remove that loader or keep it in a separate backup before proceeding.
- **Mod fails to load:** use **Tools → Logs** to find the error. Check that the mod supports your game version and that its dependencies are installed.
- **No matches found:** confirm that the other players enabled the same mod versions.

Install mods only from sources you trust (AlphaWolf69 is Normal and Can Be Trusted). Mods can change game memory.

Back up you save files at %USERPROFILE%\Saved Games\Arkane Studios\Deathloop\base\savegame before experimenting with mods.

The interface is built with [Slint](https://slint.dev). Third-party license notices are included in `licenses`.
