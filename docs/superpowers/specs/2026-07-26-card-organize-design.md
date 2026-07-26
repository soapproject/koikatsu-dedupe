# 卡片整理模組（organize）— 設計

> **狀態：定案，待轉 writing-plans** · 2026-07-26
> 取代 `2026-07-01-koikatsu-pipeline-design.md` 的 A + B 兩項。該草稿的其餘部分
> （UI 分頁結構、pipeline 串接）仍然有效，維持 DRAFT。本文只規範**整理模組**一項。
>
> 相對於 2026-07-01 草稿的三處**推翻**，理由見各節：
> 1. 轉換器不再是黑盒（已取得等效實作原始碼並實測驗證）→ 影響子專案 B，不影響本文。
> 2. 不採用 `rmp-serde`，改為自寫解碼器。
> 3. 不動 SQLite schema。

## 目標

把使用者散在多個工具的卡片整理流程中的**分類**一步，做成 `koikatsu-dedupe`
內一個可獨立執行的任務模組，取代 `koikatsu-hamster.exe`，並修掉它一個會靜默漏卡的缺陷。

## 使用者的實際流程

```
1. dl/          下載混合物：卡片 PNG、zipmod、Stiletto 設定、壓縮檔
2. hamster      把卡片分類搬出 dl                    ← 本文取代這步
3. (外部)        zipmod 搬運（Claude 的 zipmod-dedup-move skill）
4. KKS/Female   CharaCardConv_KKStoKK.exe + all.bat 整批轉成 KK 卡
5. KK/Female    搬進收藏夾 Z:\XHTA\koikatsu\cards\female_bk\collect（161,963 檔）
6. deduplate    對收藏夾去重
```

第 2 步與第 6 步在讀同一批卡（一個看 marker、一個算 hash），卻由兩支程式各掃一遍。
`KoikatsuSunshine/Female` 實際上不是終點而是**中繼站**——其內容注定要被第 4 步轉換掉。

## 要修的缺陷（已實測確認）

`koikatsu-hamster/Program.cs:21`：

```csharp
!gameTypeNames.Any(gameTypeName => s.Directory.FullName.Contains(gameTypeName))
```

本意是跳過已整理好的目的地資料夾（`Koikatu/`、`RoomGirl/`…），實作卻是拿 `GameType`
列舉名對**整條絕對路徑做子字串比對**。Koikatsu 匯出卡片的預設檔名是
`Koikatu_F_<時間戳>_<名字>`，卡站下載的卡包資料夾幾乎都叫這個——這支工具因此
**系統性跳過最常見的卡包資料夾命名**，且不報錯、不計數。

沙箱實測（同一張真實 KK 卡複製 5 份放進 5 種路徑，跑 `koikatsu-hamster.exe`）：

| 路徑情境 | 結果 |
|---|---|
| `A_plain\` | 搬走 |
| `C_[kk] 御坂セット\`（中括號） | 搬走 |
| `D_深層\姫野\カード\日本語 folder\` | 搬走 |
| `E_xxx…\yyy…\`（316 字元） | 搬走 |
| `B_Koikatu_F_20260725003553199_姬野 夜王\card\` | **靜默跳過** |

長路徑、CJK、中括號 .NET 都撐得住；唯一的失敗原因是那行子字串比對。

（附帶：`[kk] …` 這類中括號路徑對 **glob 式 API** 確實是坑——PowerShell `-Path`
會把 `[kk]` 當字元類別而回報 0 檔。Rust 端只要用非 glob 的目錄走訪即可免疫，
但仍列入回歸測試。）

## 架構

新增兩個模組，`core.rs` 維持純去重：

- **`src/card.rs`** — 結構化卡片 metadata。從現有 `core::png_char_block()` 取得
  附加資料起點（該函式走 PNG chunk 鏈，已經是正確作法，直接沿用），往後解析區塊表。
  這也是 `core.rs:506` 註解記載的升級路徑（"real MessagePack decode of the
  Parameter/KKEx blocks"）。此模組是整理、日後轉換器、以及 `card_strings()`
  改建的共同基礎。
- **`src/organize.rs`** — 分類決策與規劃／套用。

### 卡片格式（實測驗證）

附加資料起點之後：

```
i32  ProductNo
str  marker            .NET BinaryReader 字串：7-bit 編碼長度前綴 + UTF-8
str  version
i32  臉部 PNG 長度      > 0 則跳過該長度
i32  n  +  n bytes     MessagePack 區塊表 { lstInfo: [{name, version, pos, size}] }
i64  total
     各區塊位於 base + pos，base = 讀完 total 後的位置
```

產出 `CardMeta { game, card_type, sex, lastname, firstname, personality, blocks }`。
`sex`：0=Male、1=Female。

### marker 對應表

| marker | 遊戲 | 卡片類型 |
|---|---|---|
| `【KoiKatuChara】` / `【KoiKatuCharaS】` / `【KoiKatuCharaSP】` | Koikatu | Character |
| `【KoiKatuClothes】` | Koikatu | Coordinate |
| `【KoiKatuCharaSun】` | KoikatsuSunshine | Character |
| `【HCChara】` / `【HCPChara】` | HoneyCome | Character |
| `【SVChara】` / `【SVClothes】` | SVC | Character / Coordinate |
| `【ACChara】` / `【ACClothes】` | Aicomi | Character / Coordinate |

hamster 註解掉的 EmotionCreators / AiSyoujyo / RoomGirl **不進表**，直到有真實樣本。
本次的教訓就是不要靠猜——沒有樣本的 marker 一律走 `Unrecognized`。

### 三條硬規則

1. **卡片姓名可能不是合法 UTF-8** → 一律 lossy 解碼，絕不 panic，絕不拿來當識別鍵。
2. **marker 不認得** → `Unrecognized`，回報，不搬動，不猜測。
3. **任何解析失敗** → `Unreadable { reason }`，回報，不搬動。

### MessagePack 解碼（推翻草稿的 `rmp-serde`）

自寫最小解碼器（約 120 行），理由：

- 子專案 B 的轉換**不需要編碼器**——實測證明轉換是兩處各改 1 個位元組的原地修補
  （見〈為 B 建立的事實〉），需要的是「解碼時回報位元組偏移」，而非重新序列化。
- 通用 Value 型別重新編碼有整數寬度正規化的風險，會讓「其餘部分維持 byte 相同」
  這個安全性質失守。
- 本專案相依刻意極簡（`twox-hash`/`rusqlite`/`trash`/serde），CLI 參數解析與
  PNG chunk 走訪都是手寫的；此選擇與既有風格一致，且新增相依為零。

## 分類規則

### 目的地版面（沿用現況，使用者既有資料夾即為此結構）

```
[Game]/[CardType]/               非 Character 卡（目前僅 Coordinate）
[Game]/[Male|Female]/            Character 卡
```

hamster 的列舉有 `Studio` 但沒有任何 marker 會產生它——`ParseMarker` 從未回傳
Studio。本文同樣不產生 `Studio/`：場景卡在拿到真實樣本並確認其 marker 之前，
一律落入 `unrecognized` 桶。憑空造一個到不了的資料夾只會製造假象。

### 排除規則（核心修正）

目的地資料夾名集合 = marker 對應表中出現過的遊戲名，即
`Koikatu`、`KoikatsuSunshine`、`HoneyCome`、`SVC`、`Aicomi`。

```
取檔案相對於掃描根目錄的路徑 → 只看第一個區段
→ 該區段「完全等於」上述集合中的某個名字（不分大小寫）才跳過
```

絕對路徑上游有什麼字**完全不影響**。`Koikatu_F_…\card\` 第一段不等於 `Koikatu`，
會被掃描；`Koikatu\Female\` 第一段正好相等，跳過（保住原本的意圖）。

跳過數與各類未處理數**一律明確回報**——原本的失敗之所以能長期潛伏，正因為它是靜默的。

## 性格語音相容性檢查

KKS 卡若使用 KK 沒有的性格 ID，轉換後**能載入但沒有語音**，且過程無任何提示
（使用者實際踩過）。這是卡片與**目標安裝環境**的關係，不是卡片自身的屬性，
因此支援集合必須從真實安裝推導，不可寫死：

```
支援集合 = 目標安裝 abdata\sound\data\pcm\c<N> 目錄的 <N>（N >= 0）
```

實測（KK 本體 + 使用者 20,502 個模組）：**0–38，連續 39 個，無任何模組新增性格**。
但這是**算出來的**，不是常數——裝了性格模組答案就會變。

**範圍限制（刻意）**：只掃安裝目錄的 `abdata`，**不掃 zipmod**。Sideloader 模組理論上
可以帶 `sound/data/pcm/c<N>` 進來，但讀 zipmod 需要 zip 相依（違反零新增相依），而上述
實測顯示真實環境中無一模組這麼做。報告必須一併輸出**集合的來源與此限制**，不可讓讀者
以為涵蓋了模組。真的出現性格模組時再加，屆時才有引入相依的實據。

`organize plan` 對每張 KKS Character 卡讀出 `Parameter.personality`，不在支援集合內
者列入 `voice-incompatible` 桶。此桶**不阻止分類**，但在進入轉換前就把名單攤開。

## 流程：先規劃再套用

`organize plan`（唯讀）輸出每張卡的去向，外加四個明確的桶：

| 桶 | 意義 |
|---|---|
| `skipped` | 已在目的地資料夾（排除規則命中） |
| `unrecognized` | marker 不在對應表 |
| `unreadable` | 解析失敗（附原因） |
| `voice-incompatible` | KKS 卡性格 ID 不受目標安裝支援 |

確認後 `organize apply` 才動檔案。搬運方式為**移動**（符合使用者流程與 hamster 現況）。

### 撞名處理

hamster 撞名就改名成 `card(1).png`，這等於在製造重複，正好餵給本 app 要解決的問題。改為：

1. 目的地已有同名 → 比對內容雜湊
2. **內容相同** → 判定為已歸檔，回報，**不再複製第二份**
3. 內容不同 → 加後綴後併存

檔名比對一律 **case-insensitive**（Windows 視 `A.png` 與 `a.png` 為同一檔）。

## 模組獨立性（已發佈的約束）

本 app 已發佈給他人使用，且多數使用者只需要去重。因此：

- CLI 新增 `kdedupe organize --root <dir> [--recursive] [--apply]`，並登記進
  `describe` 的 commands 清單，讓 agent 能自我發現。
- GUI 新增獨立任務面板；**不啟用此模組的使用者行為完全不變**。
- 新增字串須進 7 語 `dist/i18n.js`，`node scripts/check-i18n.mjs` 會擋不完整翻譯。
- `dist/index.html` 已 999 行、`i18n.js` 已 1228 行。加面板時順手把前端依面板拆分
  ——限於本次改到的範圍，不做無關重構。

## 不動 SQLite schema（推翻草稿）

2026-07-01 草稿計畫在 `files` 表加 `game`/`card_type`/`sex`/`name` 欄位。本文不採用：

- 整理跑在 `dl`（數十至數百檔），不在收藏夾（161,963 檔），規劃時現場解析即可，
  無效能問題。
- **dl 裡的卡片生命週期很短**——分類完就搬出去了，替過路檔案建立持久記錄沒有意義。
  流程最後對收藏夾跑去重時，索引本來就會建起來，該有的表那時自然會有。
- 為尚未存在的功能，對已發佈 app 的資料庫做 schema 遷移，是投機性風險。

需要持久化時，由真正需要它的那個子專案連同其使用情境一起規範。

## 測試

沿用既有 `tests/round.rs` + `testdata/` 真實卡片夾具慣例（版權關係不隨庫發佈，
缺席時自動跳過）。回歸測試須涵蓋本次實測出的每一種情境：

1. `Koikatu_F_20260725003553199_姬野 夜王\card\` 底下的卡**要**被整理 ← 本次核心缺陷
2. 已在 `Koikatu\Female\` 的卡要被跳過（原本意圖不可失守）
3. `[kk] 御坂セット` 中括號路徑
4. 超過 260 字元的長路徑
5. 非 UTF-8 卡片姓名不 panic、不污染目的地檔名
6. marker 不認得 → 回報，不搬動
7. 撞名但內容相同 → 不製造第二份
8. KKS 卡性格 ID 不受支援 → 進 `voice-incompatible` 桶

## 範圍外

- **B：KKS→KK 轉換模組**（另開 spec）
- **C：流水線編排**（沿用 2026-07-01 草稿的分頁結構，該草稿的未決問題仍待答）
- zipmod / Stiletto 的搬運（由 Claude 的 `zipmod-dedup-move` skill 負責）

### 為 B 建立的事實（本次調查所得，B 開 spec 時直接引用）

- 使用者實際用的轉換器 `CharaCardConv_KKStoKK.exe`（271,360 bytes，2025-04-26）
  仍在 `scan\KoikatsuSunshine\Female\`，另有一份在 `Z:\XHTA\koikatsu\cards\`。
  搭配的 `all.bat` 對每張 png `start /B` 開一個 process、**不等待、不檢查錯誤**，
  最後印的 "All done!" 與實際完成無關，且只處理當層資料夾。
- 等效公開實作 <https://github.com/astralash/ConvertKKS-CC-to-KK-CC> 的 Python
  原始碼顯示，轉換的全部內容是把 `Parameter` 的 schema 版本由 `0.0.6` 改為 `0.0.5`
  ——**兩處**：區塊表的 `lstInfo[].version` 與 Parameter map 內的 `version`
  （作者註解自承為此耗掉半天）。
- 實測 4 張 KKS 卡：字串 `"0.0.6"` 在附加區塊中**恰好出現兩次**，前一位元組皆為
  `0xA5`（msgpack fixstr 長度 5）。故轉換可實作為**兩處各 1 位元組的原地修補**，
  區塊大小不變、偏移不動、區塊表不需重算，其餘位元組完全不變。
- marker 由 `【KoiKatuCharaSun】` 變為 `【KoiKatuChara】` 是 kkloader 以 KK 寫入器
  重新序列化的**副作用**，非刻意設計。marker 是長度前綴字串，改動長度只會平移
  整段尾巴、不破壞任何內部偏移（區塊 `pos` 相對於 `base`）。**KK 是否必須看到 KK
  marker 才載入，尚無證據**，留給 B 驗證。
- `Coordinate` / `Custom` 的資產 ID **完全未被觸碰**，KK 直接吃得下 KKS 的服裝髮型
  ID。先前擔心的「整套資產映射表」不存在。
- 不被支援的 key/value 的處理方式是**什麼都不做**：KKS 的 attribute 鍵與 KKS 專屬的
  `About` 區塊原封留著，KK 的 MessagePack 反序列化忽略不認得的鍵、缺少的取預設值。
  代價是性格特質**靜默流失**：KKS 獨有的 `okute`/`active`/`info`/`love`/`talk`/
  `nakama`/`nonbiri`/`lonely` 被丟棄，KK 獨有的 `donkan`/`kappatu`/`taida` 靜默變
  `false`。使用者的 `Koikatu\Female` 中有 2 張這樣的卡（marker 已是 KK、Parameter
  版本已是 0.0.5，但 attribute 仍是 KKS 結構且 `About` 區塊仍在），即現成對照組。
- B 的研究方法已定：由使用者提供「同一張卡的 KKS 原檔 + 用其轉換器產出的 KK 檔」
  配對，做逐區塊 diff。

## 未決問題

無。（2026-07-01 草稿的 3 個未決問題屬於 C 的範圍，仍待答。）
