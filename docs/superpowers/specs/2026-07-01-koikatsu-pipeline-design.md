# Koikatsu 卡片 intake pipeline 整合 — 設計草稿

> **狀態：部分被取代** · 2026-07-01
> 本草稿的 **A（結構化解析入 core）與 B（分類/搬移）已由
> `2026-07-26-card-organize-design.md` 取代並定案**，該文件同時推翻了本草稿三處前提：
> 轉換器已不是黑盒（見該文〈為 B 建立的事實〉）、不採用 `rmp-serde`、不動 SQLite schema。
> 本草稿的 **C（KKS→KK 轉換）與 D（pipeline 分頁串接）仍有效且仍是 DRAFT**，
> 下方「未決問題」屬於 D 的範圍，仍待回答。
> 對應的互動 mockup：`.superpowers/brainstorm/840-1782917483/content/pipeline-shell-v1.html`（未進版控）。

## 目標

把使用者現有、散在多個工具的 Koikatsu 卡片整理流程，整合進這個 `koikatsu-dedupe`
app，做成一條有分頁的 intake pipeline。目前流程：

```
下載到 C:\Users\weiss\Desktop\scan\dl
  → koikatsu-hamster.exe          （依「遊戲 + 性別 + 卡類型」分類、搬到子資料夾）
  → CharaCardConv_KKStoKK.exe     （用 all.bat 把 KKS/Sunshine 卡轉成 KK 格式）
  → 搬進收藏 Z:\XHTA\koikatsu\cards\female_bk\collect
  → 本工具去重
```

使用者的專案：
- `koikatsu-dedupe`（本專案，Rust/Tauri）：`C:\Users\weiss\Desktop\ws\deduplate`
- `koikatsu-hamster`（使用者自己的，C#/.NET 8/9）：`C:\Users\weiss\Desktop\ws\koikatsu-hamster`
- `CharaCardConv_KKStoKK.exe`：**黑盒第三方 exe**，位於 `...\scan\KoikatsuSunshine\Female\`；使用者猜網路上可能有 source。

## 使用者已定的方向

用**分頁**把 pipeline 分成 3 步（頂部 3 個 tab）：
- **Step 1（分頁1）**：設定下載卡片的位置 ＋ 分類
- **Step 2（分頁2）**：轉換（KKS→KK）＋ 搬移到收藏資料夾
- **Step 3（分頁3）**：現有的去重流程，原封不動搬進來（內部維持自己的 4 步 stepper）

流程：先畫 HTML（已做互動 mockup），確認結構後再逐個分頁開 spec 實作。

## 關鍵發現：hamster ＝ dedupe 目前缺的「結構化解析」

`core.rs:504` 的 comment 早已寫明升級路徑是「real MessagePack decode of the
Parameter/KKEx blocks」。hamster 的 `Parser.cs` 做的正是這件事：

| | dedupe 現況 | hamster |
|---|---|---|
| 切出 KK 區塊 | `png_char_block()` 走 chunk 到 IEND | `SearchForPngEnd()` 掃 IEND 位元組 |
| 讀內容 | `card_strings()` — strings 式掃描（模糊、有雜訊） | **MessagePack 結構化 decode** |
| 拿到的欄位 | 一堆字串（含角色名/GUID，但要猜） | 精準 `marker`(遊戲+卡類型)、`sex`、`firstname`/`lastname` |

**結論**：把 hamster 那段 parse 由 C# port 去 Rust `core.rs`，就同時：
1. 完成 core.rs 記錄的升級路徑；
2. 拿到結構化 metadata → 解鎖分類、改名、依 game/gender/type 篩選/瀏覽。
這是整條路的 keystone。

### hamster 解析格式（Rust port 用，免得重推）

`ParseCard`（`koikatsu-hamster/koikatsu-hamster/Parser.cs`）流程：
1. `SearchForPngEnd`：從頭掃到 PNG `IEND` chunk（`49 45 4E 44 AE 42 60 82`），回傳其後第一個位元組位置＝附加資料起點。（dedupe 的 `png_char_block` 走 chunk header、更嚴謹，port 時沿用 dedupe 的即可。）
2. 讀 `ProductNo`：`Int32`（跳過）。
3. 讀 `Marker`：**.NET `BinaryReader.ReadString`** ＝ 7-bit 編碼長度前綴 + UTF-8 bytes。字串 → (GameType, CardType)：
   - `【KoiKatuChara】`/`CharaS`/`CharaSP` → (Koikatu, Character)
   - `【KoiKatuClothes】` → (Koikatu, Coordinate)
   - `【KoiKatuCharaSun】` → (**KoikatsuSunshine**, Character) ← 這就是要轉換的
   - `【HCChara】`/`HCPChara` → (HoneyCome, Character)；`SVChara`/`SVClothes`→SVC；`ACChara`/`ACClothes`→Aicomi
   - 其他 → Unknown（略過）
4. 若 `CardType != Character`：直接歸 `[Game]/[CardType]`（例：Koikatu/Coordinate、.../Studio）。
5. Character 卡才續解 `ParseCharaParameter`：
   - 讀 `loadVersion`：String（跳過）
   - 讀 `faceLength`：Int32；>0 就 `Seek(faceLength)` 跳過臉部 PNG
   - 讀 `count`：Int32；讀 `count` bytes → MessagePack `BlockHeader { lstInfo: [{name,version,pos,size}] }`
   - 讀 `num2`：Int64（跳過）；記住此時 stream 位置 `position`
   - 從 `lstInfo` 找 `name=="Parameter"` 的 Info，`Seek(position + info.pos)`，讀 `info.size` bytes
   - MessagePack decode → `CharaParameter { byte sex; string firstname; string lastname }`；`sex` 0=Male、1=Female
6. 分類路徑：`[Game]/[Gender]`；有 searchTerm 且 fullname 含之 → `[Game]/[Gender]/[searchTerm]`。

Rust port 需要：`rmp-serde`（MessagePack）＋ 自己實作 .NET 7-bit-length-prefixed string 讀取。`fullname = lastname + " " + firstname`。

搬移邏輯（`Relocator.cs`）：撞名自動加 `(n)`。

## Pipeline 各步搬入 Rust 的可行性

| 步驟 | 做什麼 | 難度 |
|---|---|---|
| hamster 分類 | 結構化 decode → game/type/gender/name，move 到子資料夾 | **中**：port C# 解析（rmp-serde + .NET string quirk） |
| CharaCardConv KKS→KK | Sunshine 卡資料 → KK 格式重映射 | **難／黑盒**：先 shell out 現有 exe；找到 source 再議重寫 |
| 搬入收藏 | 檔案移動（撞名加 (n)） | **易** |
| 去重 | 現有 | 完成 |

## 建議拆解（每項各自 spec → plan → 實作）

- **A（keystone）**：結構化 metadata 解析入 `core.rs`。SQLite `files` 加欄位（`game`/`card_type`/`sex`/`name`），掃描時填。單此一步就等於把 hamster 的大腦搬進來，並解鎖改名/篩選/瀏覽。
- **B**：分類/搬移 action（GUI 分頁1 + CLI）。把卡 move 到 `[Game]/[Gender or Type]`。
- **C**：KKS→KK 轉換（分頁2）。先 shell out `CharaCardConv_KKStoKK.exe`；重寫另議。
- **D**：pipeline 分頁串接 + 各步產物自動接到下一步（分類輸出 → 轉換輸入；收藏夾 → 去重預設）。

**推薦順序**：先 A（＋B），風險最低、價值最高，正是 core.rs 記錄的升級路徑。C、D 之後各自開 spec。

## UI 結構（見 mockup）

- 頂部 3 分頁：`① 下載＋分類`、`② 轉換＋入庫`、`③ 去重`。
- 一條 ribbon 攤平顯示資料流：`dl → 分類 → KKS→KK → 入庫 → 去重`。
- 分頁3 = 現有 app 一比一，內部保留 1-2-3-4 stepper。
- 各步產物自動當下一步預設輸入。

## 未決問題（回來要先答）

1. **分頁 vs 流水線**：3 分頁自由點，還是「做完一步才解鎖下一步」的強制先後？
2. **Tab 1 分類維度**（遊戲/性別/卡類型）跟 hamster 實際輸出是否一致？有沒有漏（例：其他遊戲、Studio 卡處理）？
3. **Tab 2 轉換器**：暫時當黑盒 exe 呼叫可接受嗎？收藏夾預設路徑對嗎？是否要先研究 CharaCardConv 有無 source／格式（可能影響是否要在 C 一步就重寫）？

## 風險 / 注意

- **CharaCardConv 黑盒**：轉換正確性無法自驗；先包成外呼、保留原 exe 當事實來源。
- **MessagePack port**：.NET `BinaryReader` 的 7-bit 長度前綴字串、Parameter 區塊 pos/size 尋址，需對照真卡逐欄驗證（用 `testdata/` 真卡，且測試在缺 fixtures 時自動跳過）。
- **前端肥大**：`dist/index.html` 已 999 行、`i18n.js` 1228 行。加 2 個分頁前，順手把前端拆成分頁/模組（改到的地方順手改好，非無關重構）。
- **i18n**：新分頁的字串要進 7 語 `i18n.js`，`node scripts/check-i18n.mjs` 會擋不完整翻譯。

## 下一步

使用者思考後回答上面 3 題 → 定案 UI 結構 → 先為 **A（結構化解析入 core）** 開 writing-plans。
