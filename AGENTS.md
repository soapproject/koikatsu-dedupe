# Driving koikatsu-dedupe headlessly (for AI agents)

This tool has a headless CLI, **`kdedupe`**, that shares the same `dedupe.sqlite`
index as the GUI. Use it to scan, list duplicate groups, delete, and sort cards into
per-game folders — no GUI, no clicking. JSON goes to **stdout**, progress/errors to
**stderr**, exit code `0` on success, `1` on error, `2` on usage mistakes.

Binary location: next to the GUI exe in a release, or `src-tauri/target/release/kdedupe.exe`
after `cargo build --release`.

## Discover the interface first

```sh
kdedupe describe      # JSON manifest: every command, its flags/types/defaults, output shape, default db path
kdedupe config        # what root/db/mode resolved to (and from which config.json) — confirm before scanning/deleting
kdedupe --help        # human-readable
```

Always run `describe` + `config` before driving — `describe` reports the argument
schema, `config` reports the actual library the GUI last used, so you never have to
guess or be told the paths.

## Safe workflow

```sh
kdedupe scan   --root "D:\cards" --mode byte      # scan + hash -> {total,groups,dup_files,new,pruned}
                                                  #   add --recursive to scan subfolders too
kdedupe groups --mode byte                         # [{hash,files:[{name,path,size,mtime}]}]  <- decide here
kdedupe delete --root "D:\cards" NAME1 NAME2 ...   # DRY-RUN: prints what it WOULD delete, deletes nothing
kdedupe delete --root "D:\cards" --apply NAME1 ... # actually deletes (-> Recycle Bin, recoverable)
```

- `--mode byte` = byte-identical files. `--mode char` = same character, different cover art.
- `--root`, `--db` and `--mode` default to the GUI's **last-used** values (mirrored to
  `%APPDATA%\io.github.soapproject.koikatsu-dedupe\config.json`); omit them to act on the
  library the user is actually working on, or pass them to override. Falls back to
  `dedupe.sqlite` / `byte` if no config exists. `kdedupe config` shows what resolved.

## Choosing which file to keep

The CLI does **not** decide keepers — you do. Read `groups`, and for each group
keep exactly one file (e.g. the newest by `mtime`, or by a naming rule) and pass the
*other* cards to `delete`. A bare filename works for a top-level scan; pass the full
`path` from `groups` when you scanned with `--recursive`, so cards sharing a basename
across subfolders delete the right one.

## Sorting cards into game folders

```sh
kdedupe organize --root "D:\dl" --recursive        # DRY-RUN: {root,moves,skipped,unrecognized,unreadable,...}
kdedupe organize --root "D:\dl" --recursive --apply # actually MOVES the cards
```

Each card carries the game and character data appended after the PNG, so `organize`
files it under `[Game]/[Male|Female]` (`[Game]/Coordinate` for outfit cards) inside
`--root`. Read `moves` before applying: `collision` on each entry says `None` (free
name), `AlreadyFiled` (identical bytes already there — not moved), `Renamed` (same
name, different card — filed as `name (2).png`), or `Unresolvable` (no free name
found; reported as an error, never moved). Cards already in one of those folders are
listed in `skipped`; anything unparseable is listed in `unrecognized` / `unreadable`
rather than dropped.

- `--root` is **mandatory** here — unlike `scan`/`count`/`delete` it does *not* fall
  back to the GUI's saved root, because `--apply` moves files. Both outputs echo the
  `root` they used; check it.
- `--game-root "D:\Koikatsu"` additionally reports Sunshine cards whose personality
  that install cannot voice after conversion (`voice_incompatible`). `voice_ok:false`
  means the install could not be scanned at all, so no voice claim was made.
- With `--game-root`, a path can appear in **both** `skipped` and
  `unreadable`/`unrecognized`: already-filed cards are voice-checked too, so a read
  failure is reported without cancelling the skip.

## Rules

- **Always dry-run first.** Run `delete` without `--apply`, confirm `would_delete` is
  what you intend, then re-run with `--apply`. Same for `organize`: confirm `moves`
  and the echoed `root`.
- Never delete every file in a group — keep one.
- Deletes go to the Recycle Bin on local drives (recoverable); on a network share the
  NAS's own versioning is the safety net.
- `organize --apply` is a **move**, not a delete: nothing goes to the Recycle Bin and
  there is no undo. The dry-run is the only safety net, so read it.
