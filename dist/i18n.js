// dist/i18n.js — zero-dependency runtime i18n. Loaded BEFORE the main <script>.
// Browser: assigns onto window. Node (tests): assigns onto the vm context globalThis.
(function (g) {
  'use strict';
  const REF = 'zh-Hant';
  let _lang = REF;

  const LANGS = [
    { code: 'zh-Hant', native: '繁體中文' },
    { code: 'zh-Hans', native: '简体中文' },
    { code: 'en',      native: 'English' },
    { code: 'ja',      native: '日本語' },
    { code: 'ko',      native: '한국어' },
    { code: 'ru',      native: 'Русский' },
    { code: 'es',      native: 'Español' },
  ];

  // keys whose value is a plural object in EVERY language; category from params.n
  const PLURAL_KEYS = [
    'setup.avail', 'review.sel_info', 'detail.tally_same', 'detail.tally_diff',
    'summary.total', 'toast.deleted', 'toast.deleted_size', 'toast.delete_errors',
  ];

  function detectLang(tag) {
    const s = String(tag || '').toLowerCase();
    if (s === 'zh' || s.startsWith('zh-') || s.startsWith('zh_')) {
      if (s.includes('hant') || /^zh[-_](tw|hk|mo)\b/.test(s)) return 'zh-Hant';
      return 'zh-Hans';
    }
    for (const code of ['en', 'ja', 'ko', 'ru', 'es']) {
      if (s === code || s.startsWith(code + '-') || s.startsWith(code + '_')) return code;
    }
    return 'en';
  }

  function setLang(code) { _lang = I18N[code] ? code : REF; return _lang; }
  function getLang() { return _lang; }

  function pluralCategory(lang, n) {
    n = Math.abs(Number(n) || 0);
    if (lang === 'ru') {
      const a = n % 10, b = n % 100;
      if (a === 1 && b !== 11) return 'one';
      if (a >= 2 && a <= 4 && (b < 12 || b > 14)) return 'few';
      return 'many';
    }
    if (lang === 'en' || lang === 'es') return n === 1 ? 'one' : 'other';
    return 'other';
  }

  function rawLookup(lang, key) {
    const d = I18N[lang];
    if (d && Object.prototype.hasOwnProperty.call(d, key)) return d[key];
    const r = I18N[REF];
    if (r && Object.prototype.hasOwnProperty.call(r, key)) return r[key];
    return key; // last resort — never throw
  }

  function t(key, params) {
    params = params || {};
    let val = rawLookup(_lang, key);
    if (val && typeof val === 'object') {
      const cat = pluralCategory(_lang, params.n);
      val = (cat in val) ? val[cat] : (val.other != null ? val.other : val.one);
    }
    return String(val).replace(/\{(\w+)\}/g, function (_m, name) {
      const v = params[name];
      if (v == null) return '';
      if (typeof v === 'number') { try { return v.toLocaleString(_lang); } catch (e) { return String(v); } }
      return String(v);
    });
  }

  const I18N = {
    'zh-Hant': {
      'app.title': '卡片去重',
      'help.btn_title': '這是什麼？怎麼運作？',
      'step.sync': '1 整理卡片',
      'step.setup': '2 選一批',
      'step.review': '3 挑重複',
      'step.delete': '4 刪除',

      'sync.title': '整理卡片（建一份清單）',
      'sync.sub': '先掃一次你的卡片資料夾、記下每張卡的特徵，之後找重複都用這份清單，不必每次重掃。',
      'sync.label.root': '卡片資料夾',
      'sync.browse': '瀏覽…',
      'sync.label.db': '索引檔',
      'sync.pick_loc': '選位置',
      'sync.label.game': '遊戲路徑',
      'sync.game_ph': '（選填）例：…\\Koikatsu\\UserData\\chara\\female，用於「複製到遊戲目錄」',
      'sync.pick_folder': '選資料夾',
      'sync.label.mode': '找重複的方式',
      'sync.label.rebuild': '重建索引',
      'sync.rebuild_opt': '重新建立整份清單',
      'sync.recent': '最近：',
      'sync.recent_load': '點擊載入',
      'sync.idle': '待命中',
      'sync.log_start': '按「開始整理」啟動…',
      'sync.start': '開始整理',
      'sync.skip': '下一步 →',
      'sync.root_ok': '✓ 資料夾存在 · {n} 個 png',
      'sync.root_bad': '✗ 找不到 png · {n} 個 png',
      'sync.computing': '計算中…',
      'sync.starting': '開始整理{full}…',
      'sync.full_note': '（重建整份清單）',
      'sync.log_starting': '開始整理…',
      'sync.log_done': '完成 · 共 {total} 張，找到 {groups} 組重複（可清 {dup} 張）；新增 {added}、移除 {pruned}',
      'sync.done': '✓ 整理完成',
      'sync.log_error': '錯誤：',
      'sync.error': '✗ 出錯了',

      'mode.byte': '完全一樣的檔案',
      'mode.char': '同角色、不同封面',
      'mode.byte_short': '完全一樣',
      'mode.char_short': '同角色不同封面',

      'phase.scan': '掃描卡片',
      'phase.compare': '比對中',
      'phase.read_char': '讀取角色資料',

      'setup.title': '選一批來看',
      'setup.sub': '從剛剛那份清單，把重複的卡撈出來處理。一次看幾組由你決定。',
      'setup.byte_desc': '同一張卡被複製或改名存了好幾份，內容一模一樣，留一張即可。',
      'setup.char_desc': '只看卡片裡的角色資料、不管封面圖，所以換了封面的同一個角色也抓得到。',
      'setup.badge_ignore_cover': '無視封面',
      'setup.batch_label': '一次看幾組',
      'setup.avail': { other: '／ 可用 {n} 組' },
      'setup.avail_not_synced': '／「{label}」還沒整理過 — 回上一步跑一次',
      'setup.avail_none': '／ 還沒整理',
      'setup.start': '開始挑 →',

      'common.back': '← 回上一步',

      'review.group_count': '第 {i} 組 / 共 {n} 組',
      'review.badge_char': '同一個角色 · 封面不同',
      'review.title_byte': '這幾張是一模一樣的檔案',
      'review.title_char': '這幾張其實是同一個角色',
      'review.hint': '點一下你<b style="color:var(--hl)">想刪掉</b>的卡（會亮黃框），沒點的會留著。挑好按「下一組」。（數字鍵 1–9 快速選、← → 換組）',
      'review.copied': '✓ 已複製',
      'review.copy_to_game': '複製到遊戲目錄',
      'review.copied_btn': '已複製 ✓',
      'review.select_others': '留第一張、其餘都選',
      'review.detail': '看裡面內容',
      'review.auto': '⚡ 全部交給它',
      'review.sel_info': { other: '已選 {n} 張要刪' },
      'review.prev': '← 上一組',
      'review.next': '下一組 →',

      'detail.loading': '載入角色資料中…',
      'detail.read_fail': '(讀取失敗) ',
      'detail.rescan': '▶ 逐行比對',
      'detail.same': '相同',
      'detail.diff': '不同',
      'detail.tally_same': { other: '{same} 列相同' },
      'detail.tally_diff': { other: '{diff} 列不同' },

      'summary.title': '最後確認',
      'summary.sub': '這些是你標記要刪的卡。確認後才會真的刪 —— 本機進系統回收桶、NAS 進 NAS 回收桶，都救得回來。',
      'summary.total': { other: '共 {count} 張，約 {size}' },
      'summary.warn': '⚠ 這是「同角色、不同封面」的卡 —— 被刪那幾張的<b>封面會消失</b>（仍可從回收桶復原）。',
      'summary.col_group': '組',
      'summary.col_name': '檔名',
      'summary.col_size': '大小',
      'summary.delete': '確認刪除',
      'summary.back': '← 回去再看',

      'toast.deleted': { other: '已刪除 {n} 張 ✓' },
      'toast.deleted_size': { other: '已刪除 {n} 張（{mb} MB）' },
      'toast.delete_errors': { other: '（{n} 個錯誤）' },
      'toast.none_selected': '沒有選取任何卡',
      'toast.delete_fail': '刪除失敗：{e}',
      'toast.copy_desktop_only': '複製到遊戲目錄僅桌面版可用',
      'toast.set_game_path': '先在步驟1填「遊戲 chara 路徑」',
      'toast.no_src_path': '此卡無來源路徑',
      'toast.copied_ok': '已複製到遊戲目錄 ✓',
      'toast.copy_fail': '複製失敗：{e}',
      'toast.read_fail': '讀取失敗：{e}',
      'toast.no_dups': '找不到重複的卡，先回上一步「整理卡片」跑一次',
      'toast.auto_desktop_only': '全部自動：桌面版功能',
      'toast.pick_root_first': '先選卡片資料夾',

      'err.src_no_name': '來源路徑無檔名',
      'err.game_path_invalid': '遊戲 chara 路徑不存在或不是資料夾：{path}',

      'help.title': 'How it works',
      'help.subtitle': '· 技術細節',
      'help.lead': '兩種比對模式：<b>全圖 hash</b> 與 <b>只比對角色資料區塊</b>，共用一張 SQLite 索引表做增量更新。',
      'help.h_normal': '一般模式 · 全圖 hash（找一模一樣的檔案，快速跳過明顯不一致的）',
      'help.p_normal': '先比對檔案大小，size 不一樣的跳過；再比前 1 MB 的 hash，都一樣才跑完整檔 hash（都用 <code>xxHash64</code>）。',
      'help.h_advanced': '進階模式 · 只比角色資料區塊（找同角色、不同卡面，無視卡面差異）',
      'help.p_advanced': '走 chunk 到 <code>IEND</code>，只 hash 後面那段角色資料、卡面不管。先比長度（<code>char_len</code>）跳掉不一樣的，再比內容（<code>char_hash</code>）分組。純位元組、不解碼字串 → 非 UTF-8 角色名也安全。',
      'help.h_build': '怎麼建表',
      'help.p_build': '掃描頂層 PNG → 寫進 SQLite 單表 <code>files(path PK, size, mtime, head_hash, full_hash, char_len, char_hash)</code> → 依模式分層把雜湊填進對應欄位。這張表就是兩種模式<b>共用的索引</b>；之後每批處理都查表，不再逐檔讀磁碟。',
      'help.h_incremental': '為什麼不用每次全表掃描',
      'help.list_incremental': '<li>scan 用 <code>INSERT OR IGNORE</code> 只補新卡、<code>prune</code> 清掉消失的卡，既有列不動。</li><li>每層雜湊的候選 SELECT 都過濾 <code>hash IS NULL</code> → 只算還沒算的。</li><li>兩種模式用<b>各自獨立的欄位</b>（位元＝head/full、進階＝char_len/char_hash）→ 換模式重用同一批列，只補那一欄。</li><li>長同步每約 <code>10s</code> commit 一次，中途關掉下次靠 <code>hash IS NULL</code> 接著跑。</li><li>想強制整池重算就勾「重建索引」清表重建（只有同路徑被<b>原地改內容</b>時才需要，因為 <code>INSERT OR IGNORE</code> 不會更新既有列）。</li>',
      'help.h_subfolder': '會掃子資料夾嗎？',
      'help.p_subfolder': '<b>不會，只掃頂層。</b>子資料夾一律忽略（含工具自己的輸出）—— 讓正式卡池維持扁平、範圍可控。',
      'help.h_delete': '刪除怎麼處理',
      'help.p_delete': '先試 OS 回收桶（<code>trash</code> ／ Windows <code>IFileOperation</code>），<b>刪完驗證檔案真的不見了</b>才算數；網路磁碟沒有回收桶、shell 可能假報成功 → 偵測到還在就 <code>fs::remove_file</code> 硬刪，交給 NAS 自身的回收桶／版本控制保留可復原性。同時把該列從索引表移除。',
      'help.retour': '重看所有導覽圈',
      'help.close': '確認',

      'tour.dismiss': '這頁知道了 ✕',
      'tour.sync.db': '會在這份檔案內建立卡片清單，之後都用這份清單比對檔案',
      'tour.sync.rebuild': '重新建立整份清單：比較花時間，但能確保清單跟資料夾現況完全一致',
      'tour.sync.start': '開始掃描資料夾、建立卡片清單 —— 之後找重複都用這份清單',
      'tour.setup.modes': '用上一步建好的清單來挑檔案。換成還沒整理過的方式 → 清單還沒建，要先回上一步整理一次（免勾「重建索引」）',
      'tour.review.auto': '自動幫每組留最新的一張、其餘列成清單；你確認後才會刪（可復原）',
      'tour.summary.delete': '按下才真的刪 —— 本機進系統回收桶、NAS 進 NAS 回收桶，都救得回來',
    },
    // Other languages added in Tasks 5–10. zh-Hant is the only entry for now.
  };

  g.I18N = I18N; g.LANGS = LANGS; g.PLURAL_KEYS = PLURAL_KEYS;
  g.detectLang = detectLang; g.setLang = setLang; g.getLang = getLang;
  g.t = t; g.pluralCategory = pluralCategory;
})(typeof window !== 'undefined' ? window : globalThis);
