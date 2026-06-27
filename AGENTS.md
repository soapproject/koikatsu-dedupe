# Driving koikatsu-dedupe headlessly (for AI agents)

This tool has a headless CLI, **`kdedupe`**, that shares the same `dedupe.sqlite`
index as the GUI. Use it to scan, list duplicate groups, and delete — no GUI, no
clicking. JSON goes to **stdout**, progress/errors to **stderr**, exit code `0`
on success, `1` on error, `2` on usage mistakes.

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

## Rules

- **Always dry-run first.** Run `delete` without `--apply`, confirm `would_delete` is
  what you intend, then re-run with `--apply`.
- Never delete every file in a group — keep one.
- Deletes go to the Recycle Bin on local drives (recoverable); on a network share the
  NAS's own versioning is the safety net.
