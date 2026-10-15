const assert = require('node:assert/strict');
const { selectRows } = require('./compatibility.js');
const rows = [
  { id: 'a', name: 'FX 10', product: 'Kontakt', area: 'Effects', status: 'done', label: 'Implemented', note: 'Stereo processor', proof: 'Verified mapping' },
  { id: 'b', name: 'FX 2', product: 'Kontakt', area: 'Effects', status: 'partial', label: 'Partial', note: 'Stereo processor', proof: 'Needs verification' },
  { id: 'c', name: 'Callback', product: 'UVI', area: 'Scripting', status: 'todo', label: 'Not implemented', note: '', proof: '' },
];
const empty = { query: '', product: '', area: '', status: '' };
const pick = (filter = {}, sort = 'name', direction = 1) => selectRows(rows, { ...empty, ...filter }, sort, direction).map(r => r.id);
assert.deepEqual(pick(), ['c', 'b', 'a'], 'feature sort handles numbers naturally');
assert.deepEqual(pick({}, 'status'), ['a', 'b', 'c'], 'status has semantic implementation order');
assert.deepEqual(pick({}, 'status', -1), ['c', 'b', 'a']);
assert.deepEqual(pick({ product: 'Kontakt', area: 'Effects', status: 'partial', query: 'stereo verification' }), ['b']);
assert.deepEqual(pick({ product: 'UVI', area: 'Effects' }), []);
assert.deepEqual(pick({ query: '  STEREO   processor ' }), ['b', 'a']);
assert.deepEqual(pick({ query: 'not implemented' }), ['c']);
assert.equal(rows[0].id, 'a', 'sorting does not mutate source data');
console.log('Compatibility sorting, combined filters, search, and empty results passed.');
