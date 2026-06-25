import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const code = readFileSync(join(here, '..', 'dist', 'i18n.js'), 'utf8');
const sb = {};
vm.createContext(sb);
vm.runInContext(code, sb); // i18n.js targets globalThis → assigns onto sb

const { detectLang, setLang, t, pluralCategory } = sb;

// detection
assert.equal(detectLang('zh-TW'), 'zh-Hant');
assert.equal(detectLang('zh-Hant-HK'), 'zh-Hant');
assert.equal(detectLang('zh-CN'), 'zh-Hans');
assert.equal(detectLang('zh'), 'zh-Hans');
assert.equal(detectLang('en-US'), 'en');
assert.equal(detectLang('ja'), 'ja');
assert.equal(detectLang('ko-KR'), 'ko');
assert.equal(detectLang('ru'), 'ru');
assert.equal(detectLang('es-419'), 'es');
assert.equal(detectLang('fr-FR'), 'en'); // fallback
assert.equal(detectLang(''), 'en');

// interpolation + number local/escape-free
setLang('zh-Hant');
assert.equal(t('sync.log_error'), '錯誤：');
assert.equal(t('review.group_count', { i: 1, n: 20 }), '第 1 組 / 共 20 組');

// fallback to reference when key missing in a language
setLang('en');
assert.equal(t('__definitely_missing__'), '__definitely_missing__'); // last resort = key

// plural categories
assert.equal(pluralCategory('en', 1), 'one');
assert.equal(pluralCategory('en', 2), 'other');
assert.equal(pluralCategory('es', 1), 'one');
assert.equal(pluralCategory('ru', 1), 'one');
assert.equal(pluralCategory('ru', 2), 'few');
assert.equal(pluralCategory('ru', 5), 'many');
assert.equal(pluralCategory('ru', 21), 'one');
assert.equal(pluralCategory('ru', 11), 'many');
assert.equal(pluralCategory('ja', 5), 'other');

console.log('i18n.test.mjs OK');
