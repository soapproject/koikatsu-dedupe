// keep.js — 自動保留優先序的純邏輯（無 DOM）。
// index.html（classic script，掛到 window）與 scripts/keep.test.mjs（node vm）共用。
// 規則型別：
//   {type:'regex', label, pattern}              // 檔名符合 pattern 者優先保留
//   {type:'meta',  key:'mtime'|'size'|'namelen', dir:'desc'|'asc'}
// 檔案物件：{name, mtime, size}

var DEFAULT_KEEP = [{ type: 'meta', key: 'mtime', dir: 'desc' }];   // 預設＝留最新（同舊行為）

// 一條規則對某檔的排序鍵：越小越優先保留
function ruleKey(rule, f) {
  if (rule.type === 'regex') {
    if (rule._re === undefined) {                                   // 編譯一次、快取在 rule._re
      try { rule._re = rule.pattern ? new RegExp(rule.pattern) : null; }
      catch (e) { rule._re = null; }
    }
    return (rule._re && rule._re.test(f.name || '')) ? 0 : 1;       // 符合→0 優先；無效/空 pattern→1 無偏好
  }
  var v = rule.key === 'size' ? (+f.size || 0)
        : rule.key === 'namelen' ? ((f.name || '').length)
        : (+f.mtime || 0);
  return rule.dir === 'desc' ? -v : v;                              // desc：大/新者優先
}

// 逐條比較；平手回 0 → 保持原序（穩定）
function cmpFiles(a, b, rules) {
  for (var i = 0; i < rules.length; i++) {
    var d = ruleKey(rules[i], a) - ruleKey(rules[i], b);
    if (d) return d;
  }
  return 0;
}

// 回傳該組裡「要保留」的那個 index；其餘交給呼叫端標記刪除
function pickKeeper(files, rules) {
  var best = 0;
  for (var i = 1; i < files.length; i++) if (cmpFiles(files[i], files[best], rules) < 0) best = i;
  return best;
}
