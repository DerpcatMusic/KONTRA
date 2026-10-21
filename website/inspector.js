(function () {
  'use strict';
  const supported = /\.(nki|nkm|nksn|uvip|ufs)$/i;
  const $ = id => document.getElementById(id);
  let files = [], instruments = [], programs = [], worker = null, timer = null, report = null, revision = '';
  let viewerReady = false, generation = 0;
  const form = $('inspection-form');
  const selected = $('inspection-instrument');
  const status = $('inspection-status');
  const error = $('inspection-error');
  const frame = $('inspection-viewer');
  const phaseNames = {
    opening: 'Opening instrument', snapshot_or_container: 'Reading snapshot or container',
    snapshot_base: 'Finding snapshot base', container: 'Reading container',
    program_metadata: 'Reading program metadata', source_frontend: 'Inspecting scripts',
    translate_and_initialize: 'Translating and initializing', ui_projection: 'Inspecting UI model',
    conversion_report_and_placeholder_plan: 'Checking converted structure',
    container_program_and_scripts: 'Reading bank programs and scripts', xml_parse_and_metadata: 'Reading program XML',
    translate: 'Translating instrument', ui_model_export: 'Exporting UI model', bank_directory: 'Reading bank directory',
  };
  function showError(message) { error.textContent = message; error.hidden = false; }
  function setBusy(busy) {
    $('run-inspection').disabled = busy;
    $('stop-inspection').hidden = !busy;
    selected.disabled = busy;
    $('bank-program').disabled = busy;
    $('multi-program').disabled = busy;
    $('snapshot-base').disabled = busy;
    form.setAttribute('aria-busy', String(busy));
  }
  function stop() {
    generation += 1;
    if (worker) worker.terminate();
    worker = null;
    clearTimeout(timer);
    setBusy(false);
  }
  async function buildIdentity() {
    const response = await fetch('inspector-build.json', { cache: 'no-store' });
    if (!response.ok) throw Error('The inspector engine is unavailable. Reload the page and try again.');
    const identity = await response.json();
    if (!/^[0-9a-f]{40}$/.test(identity.revision)) throw Error('The inspector build identity is invalid.');
    revision = identity.revision;
    showRevision(revision);
  }
  function showRevision(value) {
    $('inspection-build').textContent = `Engine ${value.slice(0, 7)}`;
    $('inspection-build').href = `https://github.com/DerpcatMusic/KONTRA/commit/${value}`;
  }
  function resetResult() {
    report = null;
    $('inspection-result').hidden = true;
    if (viewerReady) frame.contentWindow.postMessage({ type: 'kontra-inspection-clear' }, location.origin);
  }
  function instrumentChanged() {
    stop();
    resetResult();
    programs = [];
    $('bank-program').value = '';
    $('bank-programs').replaceChildren();
    $('multi-program').value = '1';
    const path = selected.value;
    $('bank-program-label').hidden = true;
    $('multi-program-label').hidden = !/\.nkm$/i.test(path);
    $('snapshot-base-label').hidden = !/\.nksn$/i.test(path);
    status.textContent = /\.ufs$/i.test(path) ? 'Inspect to read the bank’s program list.' : 'Ready to inspect.';
    error.hidden = true;
  }
  function openFiles(list) {
    stop();
    resetResult();
    try {
      const seen = new Set();
      const next = Array.from(list, file => {
        const path = file.webkitRelativePath || file.name;
        if (path.split('/').some(part => !part || part === '.' || part === '..') || path.includes('\\')) throw Error('A selected file has an unsupported path. Choose its containing folder instead.');
        if (seen.has(path)) throw Error('Two selected files have the same path. Choose a folder to preserve their locations.');
        seen.add(path);
        return { path, file };
      });
      if (next.length > 100000) throw Error('This selection exceeds 100,000 files. Choose a smaller library folder.');
      const options = next.filter(item => supported.test(item.path)).sort((a, b) => a.path.localeCompare(b.path, 'en', { numeric: true }));
      if (!options.length) throw Error('No NKI, NKM, NKSN, UVIP or UFS files were found. Choose another file or folder.');
      files = next;
      instruments = options;
      selected.replaceChildren(...options.map(item => new Option(item.path, item.path)));
      $('snapshot-base').replaceChildren(new Option('Find automatically', ''), ...options.filter(item => /\.nki$/i.test(item.path)).map(item => new Option(item.path, item.path)));
      form.hidden = false;
      $('clear-inspection').disabled = false;
      instrumentChanged();
      status.textContent = `${instruments.length} instrument${instruments.length === 1 ? '' : 's'} in ${files.length} selected file${files.length === 1 ? '' : 's'}. ${/\.ufs$/i.test(selected.value) ? 'Inspect to read the bank’s program list.' : 'Ready to inspect.'}`;
    } catch (failure) {
      files = []; instruments = []; programs = [];
      $('library-folder').value = $('library-files').value = '';
      selected.replaceChildren();
      form.hidden = true;
      showError(failure.message);
    }
  }
  function sendReport() {
    if (viewerReady && report) frame.contentWindow.postMessage({ type: 'kontra-inspection-report', report }, location.origin);
  }
  function showReport(value) {
    report = value;
    if (/^[0-9a-f]{40}$/.test(value.browser_engine?.revision)) showRevision(value.browser_engine.revision);
    $('inspection-result-title').textContent = selected.selectedOptions[0]?.textContent || 'Inspection';
    $('inspection-result').hidden = false;
    sendReport();
    const directory = value.metadata?.programs;
    if (/\.ufs$/i.test(selected.value) && Array.isArray(directory) && directory.every(p => typeof p === 'string')) {
      programs = directory;
      $('bank-program-label').hidden = false;
      updatePrograms();
      status.textContent = programs.length ? `Bank opened: ${programs.length} programs. Choose a program to inspect.` : 'The bank opened but no programs were found.';
      $('bank-program').focus();
    } else {
      $('inspection-result-title').tabIndex = -1;
      $('inspection-result-title').focus({ preventScroll: true });
      $('inspection-result').scrollIntoView({ block: 'start', behavior: 'instant' });
      status.textContent = value.status === 'failed' ? 'Inspection failed. See the recorded stage and diagnostics below.' : value.status === 'partial' ? 'Inspection completed with gaps. Review the results below.' : 'Inspection completed. Review load stages and UI below.';
    }
  }
  function updatePrograms() {
    const query = $('bank-program').value.toLocaleLowerCase();
    $('bank-programs').replaceChildren(...programs.filter(p => p.toLocaleLowerCase().includes(query)).slice(0, 200).map(p => new Option(p, p)));
  }
  async function inspect(event) {
    event.preventDefault();
    if (!selected.value || !files.length) return;
    if (typeof Worker !== 'function' || typeof WebAssembly !== 'object') { showError('This browser cannot run local inspection. WebAssembly and workers must be available.'); return; }
    stop(); resetResult(); error.hidden = true;
    let program = '';
    if (/\.nkm$/i.test(selected.value)) program = String(Number($('multi-program').value) - 1);
    if (/\.ufs$/i.test(selected.value) && programs.length) {
      program = $('bank-program').value;
      if (!programs.includes(program)) { showError('Choose a program from this bank before inspecting.'); $('bank-program').focus(); return; }
    }
    setBusy(true);
    const ticket = generation;
    status.textContent = 'Loading browser inspector…';
    try {
      await buildIdentity();
      // The user may clear/change files while the build identity is being fetched.
      if (ticket !== generation || !files.length) return;
      const active = new Worker(`inspector-worker.js?v=${revision}`, { type: 'module' });
      worker = active;
      timer = setTimeout(() => { if (worker === active) { stop(); status.textContent = 'Inspection stopped.'; showError('The inspection exceeded two minutes. Try a single instrument or a smaller library folder.'); } }, 120000);
      active.onmessage = ({ data }) => {
        if (worker !== active) return;
        if (data.type === 'progress') status.textContent = (phaseNames[data.phase] || 'Inspecting instrument') + '…';
        if (data.type === 'result') { stop(); showReport(data.report); }
        if (data.type === 'error') { stop(); status.textContent = 'Inspection stopped.'; showError(data.message); }
      };
      active.onerror = () => { if (worker === active) { stop(); status.textContent = 'The engine could not finish.'; showError('The browser inspector could not run. Reload and try a single instrument file.'); } };
      active.postMessage({ files, path: selected.value, program, base: /\.nksn$/i.test(selected.value) ? $('snapshot-base').value : '', revision });
    } catch (failure) { if (ticket !== generation) return; stop(); status.textContent = 'The engine could not start.'; showError(failure.message); }
  }
  $('choose-folder').onclick = () => $('library-folder').click();
  $('choose-files').onclick = () => $('library-files').click();
  $('library-folder').onchange = event => openFiles(event.target.files);
  $('library-files').onchange = event => openFiles(event.target.files);
  selected.onchange = instrumentChanged;
  $('bank-program').oninput = updatePrograms;
  form.onsubmit = inspect;
  $('stop-inspection').onclick = () => { stop(); status.textContent = 'Inspection stopped. Your files are still selected.'; };
  $('clear-inspection').onclick = () => {
    stop(); resetResult(); files = []; instruments = []; programs = [];
    selected.replaceChildren(); $('snapshot-base').replaceChildren(new Option('Find automatically', ''));
    $('bank-program').value = ''; $('bank-programs').replaceChildren();
    $('library-folder').value = $('library-files').value = '';
    form.hidden = true; error.hidden = true; $('clear-inspection').disabled = true;
    status.textContent = 'Files and inspection results cleared.';
    $('choose-folder').focus();
  };
  $('save-inspection').onclick = () => {
    if (!report) return;
    const url = URL.createObjectURL(new Blob([JSON.stringify(report, null, 2)], { type: 'application/json' }));
    const link = document.createElement('a'); link.href = url; link.download = 'kontra-inspection.json'; link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  window.addEventListener('message', event => {
    if (event.origin !== location.origin || event.source !== frame.contentWindow) return;
    if (event.data?.type === 'kontra-inspection-ready') { viewerReady = true; sendReport(); }
    if (event.data?.type === 'kontra-inspection-height' && Number.isFinite(event.data.height)) frame.style.height = `${Math.max(640, Math.min(18000, event.data.height))}px`;
  });
  buildIdentity().catch(() => { $('inspection-build').textContent = 'Engine build information'; });
})();
