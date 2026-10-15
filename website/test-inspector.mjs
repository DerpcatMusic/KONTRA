/* Execute the actual browser binary against authored repository fixtures. */
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';

const site = resolve(process.argv[2]);
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const { default: createInspector } = await import(pathToFileURL(resolve(site, 'inspector-engine.mjs')));
async function inspect(path, contents, args = []) {
  let text = '';
  const module = await createInspector({ noInitialRun: true,
    preRun: module => {
      module.FS.mkdir('/library');
      module.FS.writeFile('/library/' + path, contents);
      module.ENV.KONTRA_SCAN_ACTIVE = '1';
      module.ENV.KONTRA_INSPECT_READ_ONLY_INIT = '1';
      module.ENV.KONTRA_UVI_AUDIT_SEED = '0';
    },
    print: line => { text += line + '\n'; }, printErr: () => {},
  });
  const exit = module.callMain(['/library/' + path, '--initialize', ...args]);
  const report = JSON.parse(text.trim());
  assert.equal(report.format, 'kontra-library-inspector');
  assert.equal(report.schema, 1);
  assert.equal(exit, report.status === 'failed' ? 1 : 0, 'exit status matches the recorded inspection');
  process.exitCode = 0; // Expected failed inspections do not fail this test runner.
  assert.match(report.browser_engine.revision, /^[a-f0-9]{40}$/);
  assert.equal(report.evidence.audio, 'not_run');
  assert.equal(report.evidence.native_equivalence, 'unknown');
  return report;
}
const ui = await inspect('Authored.uvip', await readFile(resolve(root, 'tools/fixtures/offline-ui.uvip')));
assert.notEqual(ui.status, 'failed', JSON.stringify(ui.diagnostics));
assert.ok(ui.ui.interfaces.length > 0, 'real initialization exports a UI model');
assert.ok(ui.ui.interfaces.some(face => face.widgets.length > 0), 'authored UI widgets survive the browser engine');
assert.ok(ui.ui.interfaces.some(face => face.widgets.some(widget => widget.text === 'Inspector verification fixture' && widget.rect.width === 280)));
assert.ok(ui.ui.interfaces.some(face => face.widgets.some(widget => widget.name === 'Hidden authored label' && widget.visible === false)));
assert.ok(ui.scripts.every(script => !('source' in script)), 'source text is not implicitly included');
const kontakt = await inspect('Calibration.nki', await readFile(resolve(root, 'tools/kontakt-reference/scenarios/calibration.nki')));
assert.equal(kontakt.item.kind, 'kontakt');
assert.ok(kontakt.stages.some(stage => stage.name === 'container' && stage.status === 'ok'), 'valid Kontakt container passes the real parser');
const malformed = await inspect('Broken.nki', new Uint8Array([1, 2, 3, 4]));
assert.equal(malformed.status, 'failed', 'invalid Kontakt input must fail');
assert.ok(malformed.stages.some(stage => stage.status === 'failed'));
const fault = await inspect('Fault.uvip', new TextEncoder().encode('<Program><EventProcessors><ScriptProcessor><script><![CDATA[function onInit() error("authored fixture failure") end]]></script></ScriptProcessor></EventProcessors></Program>'));
assert.equal(fault.status, 'failed', 'script initialization faults must remain failures');
assert.ok(fault.diagnostics.length > 0);
assert.ok(fault.stages.some(stage => stage.name === 'initialize' && stage.status === 'failed'), 'the actual initializer retains the fault');
console.log(`Browser engine: Kontakt parsing, authored UVI UI (${ui.ui.interfaces.reduce((n, face) => n + face.widgets.length, 0)} widgets), invalid Kontakt, retained script faults and evidence boundaries passed.`);
