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
kdedupe --help        # human-readable
```

Always run `describe` before driving — it reports the resolved default db path and
the exact argument schema, so you never have to guess.

## Safe workflow

```sh
kdedupe scan   --root "D:\cards" --mode byte      # scan + hash -> {total,groups,dup_files,new,pruned}
kdedupe groups --mode byte                         # [{hash,files:[{name,path,size,mtime}]}]  <- decide here
kdedupe delete --root "D:\cards" NAME1 NAME2 ...   # DRY-RUN: prints what it WOULD delete, deletes nothing
kdedupe delete --root "D:\cards" --apply NAME1 ... # actually deletes (-> Recycle Bin, recoverable)
```

- `--mode byte` = byte-identical files. `--mode char` = same character, different cover art.
- `--db` defaults to the GUI's library (`%APPDATA%\io.github.soapproject.koikatsu-dedupe\dedupe.sqlite`); pass `--db` to target another index.

## Choosing which file to keep

The CLI does **not** decide keepers — you do. Read `groups`, and for each group
keep exactly one file (e.g. the newest by `mtime`, or by a naming rule) and pass the
*other* filenames to `delete`. Names are the bare filename (not the full path).

## Rules

- **Always dry-run first.** Run `delete` without `--apply`, confirm `would_delete` is
  what you intend, then re-run with `--apply`.
- Never delete every file in a group — keep one.
- Deletes go to the Recycle Bin on local drives (recoverable); on a network share the
  NAS's own versioning is the safety net.
