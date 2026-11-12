/* Open issues and releases come straight from GitHub, so this page is current without a rebuild. */
(function () {
  const api = 'https://api.github.com/repos/DerpcatMusic/KONTRA/';
  const get = path => fetch(api + path, {signal: AbortSignal.timeout(8000)}).then(r => r.ok ? r.json() : Promise.reject(r.status));
  const day = iso => new Date(iso).toLocaleDateString(undefined, {year: 'numeric', month: 'short', day: 'numeric'});
  const row = (title, url, note) => {
    if (!url.startsWith('https://github.com/DerpcatMusic/KONTRA/')) return null;
    const div = document.createElement('div'), h = document.createElement('h3'), a = document.createElement('a'), s = document.createElement('span');
    div.className = 'sampler-row'; a.href = url; a.textContent = title; s.textContent = note;
    h.append(a); div.append(h, s);
    return div;
  };
  const fill = (id, rows) => { rows = rows.filter(Boolean); if (rows.length) document.getElementById(id).replaceChildren(...rows); };
  // The fallback links stay when GitHub cannot be reached.
  get('issues?state=open&per_page=30').then(issues => {
    issues = issues.filter(i => !i.pull_request);
    document.getElementById('issues-line').textContent = `${issues.length} open, newest first.`;
    fill('issues', issues.map(i => row(i.title, i.html_url, `#${i.number} · ${day(i.created_at)}`)));
  }).catch(() => {});
  get('releases?per_page=6').then(releases =>
    fill('releases', releases.map(r => row(r.name || r.tag_name, r.html_url, day(r.published_at))))).catch(() => {});
})();
