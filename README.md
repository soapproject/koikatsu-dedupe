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
   Multi-select the ones to delete and advance through the batch. Toggle
   **詳細資料** to lay each card's readable character-data strings (name, block
   names, mod GUIDs) side by side for a manual check, or **複製到遊戲** to copy a
   card into the game's chara folder (set its path in step 1) and compare in-game.
4. Confirm — selected cards are moved to the Recycle Bin (recoverable); the kept card stays in place.

Only the top level of the chosen folder is scanned (subfolders, including the app's
own output, are ignored). The character-data parser is byte-level and never decodes
the embedded name, so non-UTF-8 / special-character card names are handled safely.

## Build

Requires [Rust](https://rustup.rs) and the [Tauri CLI](https://tauri.app)
(`cargo install tauri-cli` or `npm i -g @tauri-apps/cli`).

```sh
cd src-tauri
cargo build --release      # -> target/release/app.exe (standalone)
# or a bundled installer:
cargo tauri build
```

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
