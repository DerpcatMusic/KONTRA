// Installed WASM grammar only. No Rust compilation/typechecking/execution.
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';
import { execFileSync } from 'node:child_process';
const modules = process.argv[2];
if (!modules) throw new Error('Pass existing Graft node_modules; do not install.');
const { Parser, Language } = await import(pathToFileURL(path.join(modules, 'web-tree-sitter/web-tree-sitter.js')));
await Parser.init();
const grammar = path.join(modules, 'tree-sitter-wasm/out/rust/tree-sitter-rust.wasm');
const parser = new Parser();
parser.setLanguage(await Language.load(grammar));
const digest = b => createHash('sha256').update(b).digest('hex');
const files = [
 'crates/sampler-core/src/lib.rs', 'crates/sampler-core/src/ops.rs', 'crates/sampler-core/src/waveform.rs',
 'crates/sampler-core/tests/waveform.rs',
 'crates/sampler-ksp/src/lib.rs', 'crates/sampler-ksp/src/eval.rs', 'crates/sampler-ksp/src/lower.rs',
 'crates/sampler-ksp/src/ui.rs', 'crates/sampler-ksp/src/waveform.rs',
 'crates/sampler-ksp/tests/ui.rs', 'crates/sampler-ksp/tests/waveform.rs',
 'crates/sampler-ksp/tests/ui_callbacks.rs', 'crates/sampler-ksp/tests/waveform_runtime.rs',
];
function parse(bytes) {
 const lines = bytes.toString('utf8').split('\n');
 const tree = parser.parse(bytes.toString('utf8')), errors = [];
 function walk(node) {
  if (node.type === 'ERROR' || node.isMissing) errors.push({type:node.type, text:node.text,
   start:node.startPosition, end:node.endPosition,
   context_sha256:digest(lines.slice(Math.max(0,node.startPosition.row-2),node.endPosition.row+3).join('\n'))});
  for (const child of node.children) walk(child);
 }
 walk(tree.rootNode); tree.delete(); return errors;
}
const base = 'd1fb71954431d858f4437d1fb8492db820d68f18';
const results = files.map(file => {
 const bytes = fs.readFileSync(file), errors = parse(bytes);
 let baselineErrors = [];
 if (errors.length) baselineErrors = parse(execFileSync('git',['show',`${base}:${file}`]));
 const signature = list => JSON.stringify(list.map(e => [e.type,e.text,e.context_sha256]));
 const unchangedGrammarErrors = errors.length > 0 && signature(errors) === signature(baselineErrors);
 return {file, sha256:digest(bytes), errors, baseline_errors:baselineErrors,
  unchanged_baseline_grammar_limitation:unchangedGrammarErrors,
  introduced_syntax_errors:errors.length && !unchangedGrammarErrors ? errors.length : 0};
});
console.log(JSON.stringify({kind:'WASM syntax ONLY, not typechecking/runtime/native evidence',
 grammar_sha256:digest(fs.readFileSync(grammar)), baseline_sha:base, rust_tests:'NOT_RUN', typechecks:'NOT_RUN', files:results},null,2));
if (results.some(r => r.introduced_syntax_errors)) process.exitCode = 1;
parser.delete();
