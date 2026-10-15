/* Each inspection gets a fresh worker. Selected files are mounted read-only. */

self.onmessage = async ({ data }) => {
  try {
    if (!/^[0-9a-f]{40}$/.test(data.revision)) throw Error('Invalid inspector build.');
    const { default: createInspector } = await import(`./inspector-engine.mjs?v=${data.revision}`);
    let output = '', outputBytes = 0;
    const engine = await createInspector({
      noInitialRun: true,
      locateFile: name => new URL(name, import.meta.url).href + `?v=${data.revision}`,
      print: line => {
        outputBytes += line.length;
        if (outputBytes > 64 * 1024 * 1024) throw Error('The report exceeded the 64 MiB browser limit.');
        output += line + '\n';
      },
      printErr: line => {
        if (!line.startsWith('INSPECT_PROGRESS ')) return;
        try {
          const progress = JSON.parse(line.slice('INSPECT_PROGRESS '.length));
          if (typeof progress.phase === 'string') postMessage({ type: 'progress', phase: progress.phase });
        } catch { /* Raw diagnostics can contain library data; keep them local to the engine. */ }
      },
    });
    engine.FS.mkdir('/library');
    engine.FS.mount(engine.WORKERFS, {
      blobs: data.files.map(({ path, file }) => ({ name: path, data: file })),
    }, '/library');
    engine.ENV.KONTRA_SCAN_ACTIVE = '1';
    engine.ENV.KONTRA_INSPECT_READ_ONLY_INIT = '1';
    engine.ENV.KONTRA_UVI_AUDIT_SEED = '0';
    postMessage({ type: 'progress', phase: 'opening' });
    const args = ['/library/' + data.path, '--initialize'];
    if (data.program !== '') args.push('--program', data.program);
    if (data.base) args.push('--base', '/library/' + data.base);
    engine.callMain(args);
    const report = JSON.parse(output.trim());
    if (report.format !== 'kontra-library-inspector' || report.schema !== 1) throw Error('The engine returned an unsupported report.');
    postMessage({ type: 'result', report });
  } catch (error) {
    // No authored payload or source text is emitted in this error channel.
    postMessage({ type: 'error', message: error instanceof WebAssembly.RuntimeError ? 'The browser engine stopped. Try a single instrument or a smaller selection.' : String(error?.message || 'The browser engine could not start.') });
  }
};
