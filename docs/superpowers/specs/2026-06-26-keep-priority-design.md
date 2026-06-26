# 進階：自訂「自動保留優先順序」— design

2026-06-26 · ships in step 1 (sync screen)

## Goal
Let users control which file in a duplicate group is **kept** (the rest get
selected for deletion), instead of the hardcoded "keep newest mtime". Lives in
a collapsible **進階設定** section so default users see no change.

## UX
- `<details class="advbox">` at the bottom of step 1, **collapsed by default**.
- An ordered, **drag-reorderable** list of rules (priority 1 → 2 → 3 …).
- Rule kinds:
  - **regex** — editable label + pattern; matches against filename. Match = preferred-to-keep.
  - **meta** — `mtime` / `size` / `namelen`, each with a direction toggle
    (新/舊, 大/小, 短/長).
- Add-rule chips (presets): 修改時間 / 檔案大小 / 檔名長度 / pixiv id / 有版本號 / 自訂 regex.
- 重設為預設 button.

## Keep algorithm (`pickKeeper`)
Per group, sort files by the rule vector (top-down); first = keeper.
- regex rule → key 0 if `new RegExp(pattern).test(name)` else 1 (matched first).
  Invalid / empty pattern → key 1 (no preference). Compiled RegExp cached on
  `rule._re`, cleared on edit, stripped before persisting.
- meta rule → numeric value (mtime/size/name.length), negated for `desc`.
- Ties fall through to the next rule; full tie → keep the earliest file (stable).

## Applies to
- ⚡ 全自動 (`autoRun`) — keeper per group, rest pre-selected for delete.
- Review screen "留第一張、其餘都選" (`btnSelOthers`) — keeper kept, rest selected.
  (Required adding `mtime` to the review file objects in `btnStart`.)

## Persistence (seed-once, never overwrite)
- Rules stored in the existing `localStorage` config (`dedupe.cfg`) under `keep`.
  Same store as folder/db/mode — proven to survive app updates (WebView2 data dir
  keyed by app identifier, not version).
- **Seed only when absent**: `loadCfg().keep == null` → seed `DEFAULT_KEEP`
  (`[{meta,mtime,desc}]` = today's behavior). A saved value (even `[]`) is never
  overwritten on update. So the author's pixiv/version regexes are **presets**,
  not forced defaults.
- Future safety net (not built, YAGNI): export/import rules as JSON.

## i18n
27 new keys (`adv.*`, `keep.*`) across all 7 languages; meta/direction labels are
rendered via `t()` and re-rendered on language switch.

## Real-data grounding (Z:\…\female_bk\collect, 176,134 files)
pixiv `^\d+_p?\d+\.png$` ≈ 33k · KK_id ≈ 63k · Koikatu_F_ export ≈ 28k ·
version markers ≈ 3.7k · Windows copy suffixes ≈ 5.6k. Version-number *magnitude*
comparison intentionally skipped (too noisy; identical-hash groups rarely contain
real version collisions) — "有版本號" is presence-only.
