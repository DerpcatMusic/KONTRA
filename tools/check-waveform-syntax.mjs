// No-build Rust syntax check using an already installed WASM grammar.
// Usage: node tools/check-waveform-syntax.mjs /absolute/path/to/graft/node_modules
// This does not type-check, compile, or execute Rust or KSP.
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';

const modules = process.argv[2];
if (!modules) throw new Error('Pass the existing Graft node_modules path; do not install dependencies.');
const { Parser, Language } = await import(pathToFileURL(path.join(modules, 'web-tree-sitter/web-tree-sitter.js')));
await Parser.init();
const grammar = path.join(modules, 'tree-sitter-wasm/out/rust/tree-sitter-rust.wasm');
const language = await Language.load(grammar);
const parser = new Parser();
parser.setLanguage(language);
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const files = [
  'crates/sampler-ksp/src/ui.rs',
  'crates/sampler-ksp/tests/ui.rs',
  'crates/sampler-ksp/tests/waveform.rs',
  'crates/sampler-ksp/tests/ui_callbacks.rs',
];
const results = files.map(file => {
  const bytes = fs.readFileSync(file);
  const tree = parser.parse(bytes.toString('utf8'));
  const errors = [];
  function visit(node) {
    if (node.type === 'ERROR' || node.isMissing) {
      errors.push({ type: node.type, start: node.startPosition, end: node.endPosition });
    }
    for (const child of node.children) visit(child);
  }
  visit(tree.rootNode);
  tree.delete();
  return { file, sha256: sha256(bytes), errors };
});
console.log(JSON.stringify({
  kind: 'WASM syntax parsing ONLY; not Rust type checking or product execution',
  grammar_sha256: sha256(fs.readFileSync(grammar)),
  rust_tests: 'NOT_RUN',
  files: results,
}, null, 2));
if (results.some(result => result.errors.length)) process.exitCode = 1;
parser.delete();
