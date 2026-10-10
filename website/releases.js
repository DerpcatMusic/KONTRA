/* GitHub releases are authoritative; downloads still work if its API is unavailable. */
(function () {
  const platform = navigator.userAgentData?.platform || navigator.platform || navigator.userAgent;
  const detected = /win/i.test(platform) ? 'windows' : /mac/i.test(platform) ? 'macos' : /linux/i.test(platform) && !/android/i.test(navigator.userAgent) ? 'linux' : null;
  if (detected) {
    const download = document.querySelector(`[data-platform="${detected}"]`);
    download.dataset.detected = 'true';
    download.querySelector('.detected').hidden = false;
  }
  fetch('https://api.github.com/repos/DerpcatMusic/KONTRA/releases/latest', {signal: AbortSignal.timeout(8000)})
    .then(response => {
      if (!response.ok) throw new Error('Release lookup unavailable');
      return response.json();
    })
    .then(release => {
      const url = new URL(release.html_url);
      if (url.origin !== 'https://github.com' || !url.pathname.startsWith('/DerpcatMusic/KONTRA/releases/')) throw new Error('Unexpected release URL');
      if (typeof release.tag_name !== 'string' || !release.tag_name) throw new Error('Missing release version');
      const label = document.querySelector('#release-version');
      label.textContent = release.tag_name;
      label.href = url.href;
      const date = new Date(release.published_at);
      if (!Number.isNaN(date.getTime())) document.querySelector('#release-date').textContent = date.toLocaleDateString(undefined, {year:'numeric',month:'short',day:'numeric'});
    })
    .catch(() => {
      document.querySelector('#release-date').textContent = 'See releases for the current version';
    });
})();
