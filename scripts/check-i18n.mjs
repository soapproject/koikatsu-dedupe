import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const code = readFileSync(join(here, '..', 'dist', 'i18n.js'), 'utf8');
const sb = {};
vm.createContext(sb);
vm.runInContext(code, sb);
const { I18N, LANGS, PLURAL_KEYS } = sb;

const REF = 'zh-Hant';
const refKeys = Object.keys(I18N[REF]);
let problems = 0;
const fail = (m) => { console.error('  ✗ ' + m); problems++; };

const isEmpty = (v) =>
  v == null ||
  (typeof v === 'string' && v.trim() === '') ||
  (typeof v === 'object' && Object.values(v).some((x) => String(x).trim() === ''));

for (const { code } of LANGS) {
  const d = I18N[code];
  if (!d) { fail(`language "${code}" missing entirely`); continue; }
  const keys = Object.keys(d);
  for (const k of refKeys) if (!(k in d)) fail(`[${code}] missing key: ${k}`);
  for (const k of keys) if (!(k in I18N[REF])) fail(`[${code}] extra key not in ${REF}: ${k}`);
  for (const k of keys) if (k in I18N[REF] && isEmpty(d[k])) fail(`[${code}] empty value: ${k}`);
  for (const k of PLURAL_KEYS) {
    const v = d[k];
    if (typeof v !== 'object') { fail(`[${code}] plural key not an object: ${k}`); continue; }
    if (code === 'ru') {
      for (const c of ['one', 'few', 'many']) if (!(c in v)) fail(`[ru] plural ${k} missing "${c}"`);
    } else if (code === 'en' || code === 'es') {
      for (const c of ['one', 'other']) if (!(c in v)) fail(`[${code}] plural ${k} missing "${c}"`);
    } else {
      if (!('other' in v)) fail(`[${code}] plural ${k} missing "other"`);
    }
  }
}

if (problems) { console.error(`\ni18n parity FAILED: ${problems} problem(s).`); process.exit(1); }
console.log(`i18n parity OK — ${LANGS.length} language(s), ${refKeys.length} keys each.`);
