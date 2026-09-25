# Developer notes

Rust workspace: launcher (Slint desktop UI, software renderer), bootstrap (startup DirectInput proxy), runtime (Lua host), and core (packages/profiles/patch ownership).

All maps are invadable is a separate project in `../all-maps-are-invadable`. The manager distribution starts with no mods installed. Each project has its own build script and downloadable ZIP.

## Packages

ZIP root contains `mod.toml` and its Lua entry script. Manifest fields: id, name, version, game_build, entry, optional description, dependencies and conflicts. Dependencies are mod IDs. Enabling a mod enables installed dependencies in dependency order. Cycles/conflicts are rejected; a required dependency cannot be disabled while its dependent remains enabled.

Lua entry returns a once-per-second callback. Global game exposes base, read_u8/u32/u64, read_string(address,max) and write_flags(rows). Rows have address/expected/value Boolean-byte fields. Initial evaluation cannot write. Transactions validate expected values and ownership first, with best-effort rollback on error. This is not a security sandbox.

Compatibility protocol dlmods-v2 hashes package ID, version, game_build, entry, dependencies, conflicts and exact Lua script bytes, followed by ordered package digests, game build and protocol. Manifest display name/description/comments/formatting are excluded. Lua comments still count. Unknown manifest fields are rejected. When adding functional manifest fields, explicitly extend the checksum and tests; do not silently exclude behavior-affecting settings.

Profile editor operations are in core/src/profiles.rs. Stable file IDs are separate from display names. Profile names and disabled packages do not affect matchmaking. Profiles are lightweight ordered mod selections, without pinned versions or import/export. Each GUI launch writes its selected setup and result into a timestamped launch log; the runtime's per-process log records activation.

Slint is used under its royalty-free desktop license. Tools → About includes the required AboutSlint widget. Build output includes upstream license notices, including license files missing from published crate archives under third-party-notices.

## Startup implementation

The new bootstrap implements the researched startup changes without redistributing a third-party DLL. Modded startup skips three initializer entries, returns immediately from the protection callback dispatcher, initializes the empty compatibility suffix and sets the profile-specific version source. Known pointers/prologues are checked before writing. Writes affect only the process image; page protections are restored and the instruction cache is flushed.

An 80-byte dlmod.launch file is a five-minute, single-use request consumed at DLL attach. No request means passive forwarding. Invalid requests/unsupported layouts fail startup. No allocations, dynamic library loading, worker creation or waits are intentionally performed in DllMain. DirectInput forwarding resolves the system DLL lazily on DirectInput8Create. The manager initializes the Lua runtime separately after the menu is ready.

## Build

Requires Rust x64 MSVC and Visual Studio C++ Build Tools. Run build.ps1. Lua is compiled into the runtime. Tests: cargo test --workspace; cargo clippy --workspace --all-targets -- -D warnings.

CLI: --install ZIP; --set-enabled PROFILE MOD-ID on|off; --validate PROFILE; --prepare PROFILE GAME-EXE; --enable PROFILE; --check PROFILE; --ui-check. Prepare installs startup support and arms the next launch but does not start Steam. The GUI handles the full sequence.

TESTING.md holds machine-specific verification evidence, separate from user documentation.

## Native code mods (API 2, manager 0.4.0)

`game.api_version` is 2. A callback may call `game.install_code(plan)` once successfully. It returns the allocated cave address. Initial script evaluation cannot install code. Code remains installed until the game exits; disabling a mod takes effect on the next launch. Existing flag-only mods remain compatible.

```lua
assert(game.api_version and game.api_version >= 2, 'Update the mod manager')
local cave
return function()
    if not cave then
        cave = game.install_code({
            size = 4096,
            segments = {{offset=0, bytes='B8 2A 00 00 00 C3'}},
            sites = {{
                -- Example only: replace with a verified RVA and whole instructions.
                rva=0x100, expected='B8 01 00 00 00 C3',
                bytes='E9 00 00 00 00 90',
                rel32={{offset=1, kind='cave', target=0}}
            }}
        })
    end
    return 'Installed'
end
```

Hex strings encode literal bytes. `size` is 4–64 KiB, page-aligned. Segments are nonoverlapping offsets within the zero-initialized allocation; data and code may share it. Sites are executable-image RVAs with exact expected bytes and equal-length replacements (1–256 bytes). There are at most 64 sites and 64 segments. Empty segments allow direct instruction patches without a trampoline.

`rel32.offset` identifies the four-byte displacement field, relative to its segment/site. `kind='cave'` targets an offset within the allocation; `kind='image'` targets a game-image-relative address. Displacements are calculated from the end of that field and range checked. Authors must supply complete instructions, preserve the calling convention/registers, replay displaced instructions when needed, and relocate all position-dependent instructions themselves. This API is not an automatic detour disassembler or a security boundary.

Installation prepares the cave, validates sites and shared byte ownership, then suspends enumerated game threads. A thread inside an overwritten range aborts that attempt. Signatures are checked again before publication; caches are flushed and original page protections restored. New-thread races and arbitrary unsafe machine code are not eliminated by these checks. Install during startup, not from a hot combat path. Installation errors are surfaced in the runtime log. An error requesting restart must not be treated as permission to unload or reuse the allocation.

The exact Lua bytes, including the native plan, already participate in the compatibility fingerprint. No package-format change or hard-coded group-invasion functionality is needed in the manager.
