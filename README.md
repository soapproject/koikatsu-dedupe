# koikatsu-dedupe

A Windows desktop app for finding and removing duplicate Koikatsu character cards.

Koikatsu saves each character as a PNG with the character data appended after the
image, so the *same character* is often stored many times under different cover art —
a plain file hash misses those. This tool offers two modes:

- **Byte-exact** — identical files (a card re-saved or renamed). Groups by full-file
  xxHash64, pre-filtered by size then a 1 MB head hash.
- **Character-data (advanced)** — ignores the cover image and hashes only the appended
  Koikatsu block, so it catches *same character, different cover*. Groups by `char_hash`,
  pre-filtered by the block length (`char_len`).

Built with [Tauri](https://tauri.app) — a Rust backend plus a single static HTML
frontend. No server, no Python. A SQLite index (top-level scan, two-tier hashing,
automatic prune of vanished files) keeps re-syncs incremental.

## Use

1. Run the app (Windows 10/11 with WebView2 — preinstalled on current Windows).
2. Pick your card folder, choose a mode, and **Sync** (scans + hashes).
   **Full scan** rebuilds the whole index from scratch.
3. Review each duplicate group — cards are shown enlarged with filename + size.
   Multi-select the ones to delete and move through the batch (prev/next buttons or
   ←/→). Toggle the detail view to lay each card's readable character-data strings
   (name, block names, mod GUIDs) side by side for a manual check, or copy a card
   into the game's chara folder (set its path in step 1) to compare in-game. Or hit
   **auto-finish** to keep the newest card per group across the whole pool and jump
   straight to the confirmation list.
4. Confirm — selected cards are deleted (Windows Recycle Bin on local drives; on a
   network share / NAS, which has no Recycle Bin, the file is removed and the NAS's
   own recycle bin / versioning keeps it recoverable). The kept card stays in place.

The UI is plain-language with a first-run guided tour; a **?** dialog explains how
the matching, indexing, incremental re-sync and deletion actually work.
The UI ships in 7 languages — 繁中 / 简中 / English / 日本語 / 한국어 / Русский / Español —
auto-detected from the system on first run and switchable from the top-right dropdown
(choice remembered). Translations live in `dist/i18n.js`; completeness is guarded by
`node scripts/check-i18n.mjs`.

Only the top level of the chosen folder is scanned (subfolders, including the app's
own output, are ignored). The character-data parser is byte-level and never decodes
the embedded name, so non-UTF-8 / special-character card names are handled safely.

## CLI (headless / AI agents)

The same dedup engine ships as a headless binary, **`kdedupe`**, that shares the GUI's
`dedupe.sqlite`. It prints JSON to stdout (progress/errors to stderr) so a script — or
an AI agent — can scan, list, and delete without the GUI. See [AGENTS.md](AGENTS.md).

```sh
kdedupe describe                                  # JSON manifest: commands, flags, defaults
kdedupe scan   --root "D:\cards" --mode byte      # scan + hash
kdedupe groups --mode byte                         # duplicate groups (JSON)
kdedupe delete --root "D:\cards" NAME...           # DRY-RUN (deletes nothing)
kdedupe delete --root "D:\cards" --apply NAME...   # delete -> Recycle Bin
```

`delete` is dry-run unless `--apply` is given; `--db` defaults to the GUI's library.

## Build

Requires [Rust](https://rustup.rs) and the [Tauri CLI](https://tauri.app)
(`cargo install tauri-cli` or `npm i -g @tauri-apps/cli`).

```sh
cd src-tauri
cargo build --release      # -> target/release/app.exe (GUI) + kdedupe.exe (headless CLI)
# or a bundled installer:
cargo tauri build
```

Ship `kdedupe.exe` next to the GUI exe so the headless CLI travels with the app.

The frontend is `dist/index.html` (plain HTML/CSS/JS — no build step).

## Tests

```sh
cd src-tauri
cargo test
```

Tests run a full scan → group → delete round against local card fixtures under
`testdata/` (not shipped — real cards are copyrighted). They skip automatically when
the fixtures are absent.

## License

[MIT](LICENSE)
