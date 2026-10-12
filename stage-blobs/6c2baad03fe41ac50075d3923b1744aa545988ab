// Source/AST inspection only. Does not invoke a compiler or a Rust test binary.
// Usage: node check_source.mjs <checkout> <installed graft node_modules>
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';
const [root, modules] = process.argv.slice(2);
const base = '2a243bcfa2262abb789265aa0951fd2a85854023';
const target = 'crates/sampler-kontakt/tests/production_modulation.rs';
const git = (...args) => execFileSync('git', ['-C', root, ...args], { encoding: 'utf8' });
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const original = git('show', `${base}:${target}`);
const source = fs.readFileSync(path.join(root, target), 'utf8');
const lines = original.split('\n');
for (const line of [647, 710]) {
  assert.equal(lines[line - 1].trim(), '.unwrap()');
  lines[line - 1] += ';';
}
assert.equal(source, lines.join('\n'), 'Only the two reserved semicolons may change');
const names = text => [...text.matchAll(/#\[test\]\s*fn (serialized_\w+)\(/g)].map(m => m[1]);
assert.equal(names(original).length, 13);
assert.deepEqual(names(source), names(original));
assert.equal(git('diff', '--check', base, 'HEAD'), '');
const coreTarget = 'crates/sampler-core/tests/controls.rs';
const coreOriginal = git('show', `${base}:${coreTarget}`);
const coreSource = fs.readFileSync(path.join(root, coreTarget), 'utf8');
const coreLines = coreOriginal.split('\n');
for (const line of [1138, 1143, 1299]) {
  assert.equal(coreLines[line - 1].trim(), '.unwrap()');
  coreLines[line - 1] += ';';
}
assert.equal(coreSource, coreLines.join('\n'));
assert.deepEqual(git('diff', '--name-only', base, 'HEAD', '--', 'crates').trim().split('\n'), [coreTarget, target]);
const parserFile = path.join(modules, 'web-tree-sitter/web-tree-sitter.js');
const grammarFile = path.join(modules, 'tree-sitter-wasm/out/rust/tree-sitter-rust.wasm');
const { Parser, Language } = await import(pathToFileURL(parserFile).href);
await Parser.init();
const parser = new Parser();
parser.setLanguage(await Language.load(grammarFile));
const inspect = text => {
  const tree = parser.parse(text);
  assert.equal(tree.rootNode.hasError, false, 'Rust AST has ERROR/missing nodes');
  const matches = [];
  const visit = node => {
    if (node.type === 'call_expression' && node.childForFieldName('function')?.text === 'support::without_heap') {
      for (const closure of node.childForFieldName('arguments').namedChildren) {
        if (closure.type !== 'closure_expression' || !closure.text.includes('.edit_controls(')) continue;
        const body = closure.childForFieldName('body');
        const last = body.type === 'block'
          ? body.namedChildren.filter(c => c.type !== 'line_comment' && c.type !== 'block_comment').at(-1)
          : body;
        matches.push({ line: closure.startPosition.row + 1, tail: last.type,
          revision_is_tail: last.type === 'call_expression' && last.text.includes('.edit_controls('),
          tail_statement_has_semicolon: last.type === 'expression_statement' && last.text.trimEnd().endsWith(';') });
      }
    }
    for (const child of node.namedChildren) visit(child);
  };
  visit(tree.rootNode);
  tree.delete();
  return matches;
};
const before = inspect(original), after = inspect(source);
assert.equal(before.length, 2);
assert.equal(before.filter(m => m.revision_is_tail).length, 2);
assert.equal(after.length, 2);
assert(after.every(m => !m.revision_is_tail && m.tail_statement_has_semicolon));
// Bounded scan: tracked crates/**/tests/**/*.rs with both the heap guard and edit API.
const candidates = git('ls-files', 'crates').trim().split('\n').filter(p => p.includes('/tests/') && p.endsWith('.rs'))
  .filter(p => { const text = fs.readFileSync(path.join(root, p), 'utf8'); return text.includes('support::without_heap') && text.includes('.edit_controls('); });
const scan = candidates.map(p => ({ path: p, matches: inspect(fs.readFileSync(path.join(root, p), 'utf8')) }));
assert.equal(scan.flatMap(p => p.matches).filter(m => m.revision_is_tail).length, 0);
const coreBefore = inspect(coreOriginal), coreAfter = inspect(coreSource);
assert.equal(coreBefore.filter(m => m.revision_is_tail).length, 3);
assert.equal(coreAfter.filter(m => m.revision_is_tail).length, 0);
const pin = (p, first, last) => {
  const bytes = fs.readFileSync(path.join(root, p));
  return { path: p, blob: git('rev-parse', `HEAD:${p}`).trim(), sha256: sha256(bytes), first, last,
    span_sha256: sha256(bytes.toString().split('\n').slice(first - 1, last).join('\n') + '\n') };
};
const run = '/mnt/Windows11/DEV_WORKSPACE/kontra-runs/pi-integration-round6-successor-v2';
const failed = {
  'FROZEN-SOURCE.json': 'f4c465ff191d581684a56d4fee920ef2c22dbac0bd59077923702fbfbb19cf61',
  'combined-no-run.json': 'c555adbcf95063157db2ab83c8bb0123a1b765fbb7e7cf5a40fe901a9682531c',
  'combined-no-run.log': '6ca7ef6889999b16d2929b9709e405f997dda657f2cf2214948dece53c4c5c0e',
  'COMPILE-FIRST-RED.json': 'd6a7c51757dd3894593f644ca4381b8d7e9dfabbd629681aa4a3ea7f01993bc3',
  'COMPILE-FIRST-RED.log': 'd6059778986b2b007a4c70e3e9767868f838200927e453f21e402bc5d8299136',
  'COMPILE-RED-SOURCE-PIN.json': '2bdd9c7155d6d83b5c0bfa95c93a2c858473eabba732a9091a22239840cc871e'
};
for (const [p, digest] of Object.entries(failed)) assert.equal(sha256(fs.readFileSync(path.join(run, p))), digest);
const manifests = ['modulation-control-remap-fix', 'production-modulation-next'].map(p => {
  const file = `docs/audit-2026-10-10/${p}/source-manifest.json`;
  return { path: file, blob: git('rev-parse', `${base}:${file}`).trim(), sha256: sha256(fs.readFileSync(path.join(root, file))) };
});
const remap = JSON.parse(fs.readFileSync(path.join(root, manifests[0].path), 'utf8'));
const archives = remap.references.map(ref => {
  assert.equal(sha256(fs.readFileSync(ref.verified_archive)), ref.sha256);
  return { path: ref.verified_archive, sha256: ref.sha256, authority: 'SPEC_ONLY; native runtime UNKNOWN' };
});
const receipt = {
  status: 'SOURCE_READY_NOT_TYPECHECKED', base, source_commit: git('rev-parse', 'HEAD').trim(),
  target, original_blob: git('rev-parse', `${base}:${target}`).trim(), original_sha256: sha256(original),
  successor_blob: git('rev-parse', `HEAD:${target}`).trim(), successor_sha256: sha256(source),
  changed_lines: [647, 710], exact_five_semicolons_only: true, diff_check: 'PASS',
  bounded_additional_fix: { path: coreTarget, changed_lines: [1138, 1143, 1299],
    original_blob: git('rev-parse', `${base}:${coreTarget}`).trim(), original_sha256: sha256(coreOriginal),
    successor_blob: git('rev-parse', `HEAD:${coreTarget}`).trim(), successor_sha256: sha256(coreSource),
    closures_before: coreBefore, closures_after: coreAfter,
    authority: 'SOURCE_SIGNATURE_AND_AST_ONLY; not an additional frozen compiler diagnostic' },
  syntax_parser: { parser: parserFile, parser_sha256: sha256(fs.readFileSync(parserFile)),
    grammar: grammarFile, grammar_sha256: sha256(fs.readFileSync(grammarFile)), errors_before: 0, errors_after: 0,
    limitation: 'Tree-sitter AST is not Rust typechecking or execution' },
  closures_before: before, closures_after: after, bounded_scan: scan,
  signature_pins: [pin('crates/sampler-core/src/control.rs', 479, 487), pin('crates/sampler-core/tests/support/mod.rs', 42, 59)],
  fixture_pins: [pin(target, 631, 685), pin(target, 688, 735), pin(coreTarget, 1098, 1158), pin(coreTarget, 1210, 1302)],
  tests: names(source).map(name => ({ package: 'sampler-kontakt', target: 'production_modulation', name, status: 'NOT_RUN' })),
  failed_run: run, immutable_failed_receipts: failed,
  manifest_sha256: 'f4c465ff191d581684a56d4fee920ef2c22dbac0bd59077923702fbfbb19cf61',
  inherited_manifests: manifests, inherited_archives: archives,
  exact_patch: git('diff', '--no-ext-diff', '--unified=0', base, 'HEAD', '--', coreTarget, target),
  additional_test_filters: [
    'modulation_inputs_follow_identity_after_reorder_append_and_real_default_replacement',
    'schema_replacement_preserves_native_envelope_and_dsp_identity_consumers'
  ].map(name => ({ package: 'sampler-core', target: 'controls', name, status: 'NOT_RUN' })),
  native_runtime: 'UNKNOWN', cpu_ram_goal: 'UNACHIEVED',
  validation: 'Fresh root-owned combined validation required; current frozen aggregate remains RED',
  checker_sha256: sha256(fs.readFileSync(new URL(import.meta.url)))
};
console.log(JSON.stringify(receipt, null, 2));
parser.delete();
