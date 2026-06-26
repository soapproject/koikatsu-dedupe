import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const code = readFileSync(join(here, '..', 'dist', 'keep.js'), 'utf8');
const sb = {};
vm.createContext(sb);
vm.runInContext(code, sb); // keep.js uses top-level var/function → assigns onto sb

const { pickKeeper, DEFAULT_KEEP } = sb;

const f = (name, mtime, size) => ({ name, mtime: mtime || 0, size: size || 0 });
const PIX = { type: 'regex', label: 'pixiv', pattern: '^\\d+_p?\\d+\\.png$' };
const VER = { type: 'regex', label: 'ver', pattern: '[vV](er)?[ ._]?\\d+([._]\\d+)*' };
const MT = { type: 'meta', key: 'mtime', dir: 'desc' };

// headline: pixiv file kept over its newer windows-copy (the whole point of the feature)
assert.equal(pickKeeper([f('100_9.png', 1), f('100_9 - 複製.png', 9)], [PIX, MT]), 0);
// real pixiv shape
assert.equal(pickKeeper([f('a.png', 9), f('100000498_9.png', 1)], [PIX, MT]), 1);
assert.equal(pickKeeper([f('a.png', 9), f('111_p3.png', 1)], [PIX, MT]), 1);
// version regex kept over plain
assert.equal(pickKeeper([f('foo.png', 9), f('foo_v2.png', 1)], [PIX, VER, MT]), 1);
// priority order: pixiv beats version beats mtime (user's headline order)
assert.equal(pickKeeper([f('z_v2.png', 999), f('500_p1.png', 1)], [PIX, VER, MT]), 1);
// neither matches → newest mtime
assert.equal(pickKeeper([f('a.png', 100), f('b.png', 200)], [PIX, VER, MT]), 1);
// two matches tie on regex → newer mtime decides
assert.equal(pickKeeper([f('111_p2.png', 500), f('222_3.png', 400)], [PIX, MT]), 0);

// meta directions
assert.equal(pickKeeper([f('x', 1), f('y', 2), f('z', 3)], DEFAULT_KEEP), 2);          // default = newest
assert.equal(pickKeeper([f('a', 100), f('b', 200)], [{ type: 'meta', key: 'mtime', dir: 'asc' }]), 0);  // oldest
assert.equal(pickKeeper([f('a', 0, 100), f('b', 0, 300)], [{ type: 'meta', key: 'size', dir: 'desc' }]), 1); // largest
assert.equal(pickKeeper([f('a', 0, 100), f('b', 0, 300)], [{ type: 'meta', key: 'size', dir: 'asc' }]), 0);  // smallest
assert.equal(pickKeeper([f('img (2).png'), f('img.png')], [{ type: 'meta', key: 'namelen', dir: 'asc' }]), 1); // shorter

// robustness
assert.equal(pickKeeper([f('a', 5), f('b', 9)], []), 0);                                // no rules → stable first
assert.equal(pickKeeper([f('a', 1), f('b', 2)], [{ type: 'regex', pattern: '(' }, MT]), 1);  // invalid regex ignored → mtime
assert.equal(pickKeeper([f('a', 1), f('b', 2)], [{ type: 'regex', pattern: '' }, MT]), 1);    // empty pattern ignored
assert.equal(pickKeeper([f('only.png', 7)], [PIX, MT]), 0);                             // single file

// default seed shape (JSON compare: vm-sandbox objects live in another realm → deepStrictEqual rejects prototypes)
assert.equal(JSON.stringify(DEFAULT_KEEP), JSON.stringify([{ type: 'meta', key: 'mtime', dir: 'desc' }]));

console.log('keep.test.mjs OK');
