// Source-only WASM Rust grammar parsing; never compiles or executes Rust/KSP.
const fs = require('node:fs');
const crypto = require('node:crypto');
const { Parser, Language } = require('/home/derpcat/.npm-global/lib/node_modules/@nanonets/graft/node_modules/web-tree-sitter/web-tree-sitter.cjs');
const grammar = '/home/derpcat/.npm-global/lib/node_modules/@nanonets/graft/node_modules/tree-sitter-wasm/out/rust/tree-sitter-rust.wasm';
const root = process.argv[2];
(async () => {
  await Parser.init();
  const parser = new Parser();
  parser.setLanguage(await Language.load(grammar));
  const result = {kind: 'SOURCE_SYNTAX_ONLY_NOT_TYPECHECK_OR_EXECUTION', grammar_sha256: crypto.createHash('sha256').update(fs.readFileSync(grammar)).digest('hex'), files: []};
  for (const path of ['crates/sampler-ksp/src/eval.rs', 'crates/sampler-ksp/src/lower.rs', 'crates/sampler-ksp/tests/functions.rs']) {
    const bytes = fs.readFileSync(root + '/' + path);
    const tree = parser.parse(bytes.toString('utf8'));
    const errors = [], tests = [];
    function visit(n) {
      if (n.type === 'ERROR' || n.isMissing) errors.push({type: n.type, line: n.startPosition.row + 1, text: n.text.slice(0, 120)});
      if (n.type === 'function_item' && n.childForFieldName('name')?.text.startsWith('function_')) tests.push({name: n.childForFieldName('name').text, start_line: n.startPosition.row + 1, end_line: n.endPosition.row + 1});
      for (const c of n.namedChildren) visit(c);
    }
    visit(tree.rootNode);
    result.files.push({path, sha256: crypto.createHash('sha256').update(bytes).digest('hex'), has_error: tree.rootNode.hasError, errors, tests});
    tree.delete();
  }
  parser.delete();
  console.log(JSON.stringify(result, null, 2));
  if (result.files.some(f => f.has_error || f.errors.length)) process.exitCode = 1;
})();
