/* The complete table is published in HTML; JavaScript adds filtering and paging. */
(function () {
  'use strict';
  const order = { done: 0, partial: 1, todo: 2 };
  function selectRows(rows, filters, sort, direction) {
    const words = filters.query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
    return rows.filter(row =>
      (!filters.product || row.product === filters.product) &&
      (!filters.area || row.area === filters.area) &&
      (!filters.status || row.status === filters.status) &&
      words.every(word => [row.name, row.product, row.area, row.note, row.label, row.proof].join(' ').toLocaleLowerCase().includes(word))
    ).sort((a, b) => {
      const primary = sort === 'status' ? order[a.status] - order[b.status] : a[sort].localeCompare(b[sort], 'en', { numeric: true, sensitivity: 'base' });
      return direction * primary || a.name.localeCompare(b.name, 'en', { numeric: true }) || a.id.localeCompare(b.id);
    });
  }
  if (typeof module !== 'undefined') module.exports = { selectRows };
  if (typeof document === 'undefined') return;

  const table = document.getElementById('feature-table');
  if (!table) return;
  const form = document.getElementById('feature-filters');
  const data = JSON.parse(document.getElementById('compatibility-data').textContent);
  const search = document.getElementById('feature-search');
  const product = document.getElementById('feature-product');
  const area = document.getElementById('feature-area');
  const status = document.getElementById('feature-status');
  const pagination = document.getElementById('feature-pagination');
  const previous = document.getElementById('feature-prev');
  const next = document.getElementById('feature-next');
  const tbody = table.tBodies[0];
  const pageSize = 25;
  let sort = 'name', direction = 1, page = 0;
  function cell(tag, value, className) {
    const element = document.createElement(tag);
    element.textContent = value;
    if (className) element.className = className;
    return element;
  }
  function renderRow(row) {
    const tr = document.createElement('tr');
    const title = cell('th', row.name);
    title.scope = 'row';
    const support = cell('td', row.label, `status-${row.status}`);
    if (row.status === 'partial' && row.proof) support.append(cell('small', row.proof, 'proof-note'));
    const source = cell('td', '');
    if (row.source) {
      const link = cell('a', 'View');
      link.href = row.source;
      link.setAttribute('aria-label', `Source for ${row.name}`);
      source.append(link);
    } else source.textContent = '—';
    tr.append(title, support, cell('td', row.product), cell('td', row.area), cell('td', row.note, 'feature-note'), source);
    return tr;
  }
  function render() {
    const rows = selectRows(data.rows, { query: search.value, product: product.value, area: area.value, status: status.value }, sort, direction);
    const pages = Math.max(1, Math.ceil(rows.length / pageSize));
    page = Math.min(page, pages - 1);
    const start = page * pageSize;
    tbody.replaceChildren(...rows.slice(start, start + pageSize).map(renderRow));
    document.getElementById('feature-results').textContent = rows.length ? `${start + 1}–${Math.min(start + pageSize, rows.length)} of ${rows.length} features` : '0 features';
    document.getElementById('feature-empty').hidden = rows.length !== 0;
    pagination.hidden = rows.length <= pageSize;
    document.getElementById('feature-page').textContent = `Page ${page + 1} of ${pages}`;
    previous.disabled = page === 0;
    next.disabled = page === pages - 1;
    table.querySelectorAll('thead th').forEach(th => th.removeAttribute('aria-sort'));
    table.querySelector(`[data-sort="${sort}"]`).closest('th').setAttribute('aria-sort', direction === 1 ? 'ascending' : 'descending');
    // Current categories stay relevant to the selected sampler.
    for (const option of area.options) {
      option.hidden = Boolean(option.value && product.value && !data.rows.some(row => row.product === product.value && row.area === option.value));
    }
    const url = new URL(location.href);
    for (const [key, value] of Object.entries({ sampler: product.value, category: area.value, status: status.value, q: search.value })) {
      if (value) url.searchParams.set(key, value); else url.searchParams.delete(key);
    }
    history.replaceState(null, '', url);
  }
  function restore() {
    const params = new URLSearchParams(location.search);
    for (const [control, key] of [[product, 'sampler'], [area, 'category'], [status, 'status']]) {
      control.value = params.get(key) || '';
    }
    search.value = params.get('q') || '';
    page = 0;
    render();
  }
  form.addEventListener('submit', event => event.preventDefault());
  form.addEventListener('input', event => {
    if (event.target === product && area.selectedOptions[0]?.hidden) area.value = '';
    if (event.target === product && product.value && area.value && !data.rows.some(row => row.product === product.value && row.area === area.value)) area.value = '';
    page = 0;
    render();
  });
  form.addEventListener('reset', () => {
    search.value = product.value = area.value = status.value = '';
    page = 0;
    // Prevent the native reset from restoring values after render reads them.
    queueMicrotask(render);
  });
  document.getElementById('feature-clear').addEventListener('click', () => { form.reset(); search.focus(); });
  table.querySelectorAll('[data-sort]').forEach(button => {
    button.disabled = false;
    button.addEventListener('click', () => {
      direction = sort === button.dataset.sort ? -direction : 1;
      sort = button.dataset.sort;
      page = 0;
      render();
    });
  });
  function turnPage(offset) {
    page += offset;
    render();
    document.querySelector('.table-result-line').scrollIntoView({ block: 'start' });
  }
  previous.addEventListener('click', () => turnPage(-1));
  next.addEventListener('click', () => turnPage(1));
  window.addEventListener('popstate', restore);
  form.hidden = false;
  restore();
})();
