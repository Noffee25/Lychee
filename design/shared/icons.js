/* Lychee design mockups: a small icon set (Feather-style, 24x24) and the Lychee logo.
   Usage: <i data-i="play"></i>, then LYCHEE.hydrateIcons(root, { stroke: 1.75 }).
   Per-element overrides: data-s="20" (size), data-w="1.5" (stroke width). */
(function (L) {
  'use strict';

  // Filled glyphs (transport). Drawn with fill and a round-joined stroke for soft corners.
  const FILLED = {
    play: '<path d="M7.6 5.3 18.6 12 7.6 18.7Z"/>',
    pause: '<rect x="6.3" y="5.2" width="3.4" height="13.6" rx=".5"/><rect x="14.3" y="5.2" width="3.4" height="13.6" rx=".5"/>',
    prev: '<path d="M18.2 5.8 9.6 12l8.6 6.2Z"/><rect x="5.3" y="5.6" width="1.4" height="12.8" rx=".4"/>',
    next: '<path d="M5.8 5.8 14.4 12l-8.6 6.2Z"/><rect x="17.3" y="5.6" width="1.4" height="12.8" rx=".4"/>',
    rew: '<path d="M11.2 6.6 4.2 12l7 5.4Z"/><path d="M19.8 6.6 12.8 12l7 5.4Z"/>',
    ff: '<path d="M12.8 6.6l7 5.4-7 5.4Z"/><path d="M4.2 6.6l7 5.4-7 5.4Z"/>',
    stop: '<rect x="6.2" y="6.2" width="11.6" height="11.6" rx="1"/>',
    heartFill: '<path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/>',
  };

  // Stroked glyphs.
  const STROKE = {
    shuffle: '<polyline points="16 3 21 3 21 8"/><line x1="4" y1="20" x2="21" y2="3"/><polyline points="21 16 21 21 16 21"/><line x1="15" y1="15" x2="21" y2="21"/><line x1="4" y1="4" x2="9" y2="9"/>',
    repeat: '<polyline points="17 1 21 5 17 9"/><path d="M3 11V9a4 4 0 0 1 4-4h14"/><polyline points="7 23 3 19 7 15"/><path d="M21 13v2a4 4 0 0 1-4 4H3"/>',
    repeat1: '<polyline points="17 1 21 5 17 9"/><path d="M3 11V9a4 4 0 0 1 4-4h14"/><polyline points="7 23 3 19 7 15"/><path d="M21 13v2a4 4 0 0 1-4 4H3"/><path d="M11 10h1.2v4.4"/>',
    volume: '<polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><path d="M19.07 4.93a10 10 0 0 1 0 14.14M15.54 8.46a5 5 0 0 1 0 7.07"/>',
    volume1: '<polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><path d="M15.54 8.46a5 5 0 0 1 0 7.07"/>',
    mute: '<polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><line x1="23" y1="9" x2="17" y2="15"/><line x1="17" y1="9" x2="23" y2="15"/>',
    heart: '<path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/>',
    search: '<circle cx="11" cy="11" r="7.5"/><line x1="21" y1="21" x2="16.4" y2="16.4"/>',
    home: '<path d="M3 10.2 12 3l9 7.2V20a1.5 1.5 0 0 1-1.5 1.5H4.5A1.5 1.5 0 0 1 3 20z"/><path d="M9.5 21.5v-7h5v7"/>',
    disc: '<circle cx="12" cy="12" r="9.5"/><circle cx="12" cy="12" r="2.6"/>',
    user: '<path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/>',
    users: '<path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M23 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/>',
    music: '<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>',
    tag: '<path d="M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82z"/><line x1="7" y1="7" x2="7.01" y2="7"/>',
    folder: '<path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>',
    clock: '<circle cx="12" cy="12" r="9.5"/><polyline points="12 6.5 12 12 15.5 14"/>',
    plus: '<line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>',
    minus: '<line x1="5" y1="12" x2="19" y2="12"/>',
    more: '<circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/><circle cx="5" cy="12" r="1"/>',
    chevL: '<polyline points="15 18 9 12 15 6"/>',
    chevR: '<polyline points="9 18 15 12 9 6"/>',
    chevD: '<polyline points="6 9 12 15 18 9"/>',
    chevU: '<polyline points="18 15 12 9 6 15"/>',
    arrowL: '<line x1="19" y1="12" x2="5" y2="12"/><polyline points="12 19 5 12 12 5"/>',
    arrowR: '<line x1="5" y1="12" x2="19" y2="12"/><polyline points="12 5 19 12 12 19"/>',
    expand: '<polyline points="15 3 21 3 21 9"/><polyline points="9 21 3 21 3 15"/><line x1="21" y1="3" x2="14" y2="10"/><line x1="3" y1="21" x2="10" y2="14"/>',
    shrink: '<polyline points="4 14 10 14 10 20"/><polyline points="20 10 14 10 14 4"/><line x1="14" y1="10" x2="21" y2="3"/><line x1="3" y1="21" x2="10" y2="14"/>',
    x: '<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>',
    square: '<rect x="5" y="5" width="14" height="14" rx="1.5"/>',
    settings: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>',
    grid: '<rect x="3.5" y="3.5" width="7" height="7" rx="1.5"/><rect x="13.5" y="3.5" width="7" height="7" rx="1.5"/><rect x="13.5" y="13.5" width="7" height="7" rx="1.5"/><rect x="3.5" y="13.5" width="7" height="7" rx="1.5"/>',
    list: '<line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/><line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/>',
    queue: '<path d="M3 6h12M3 12h12M3 18h7"/><path d="M19 18V8l3-1"/><circle cx="17" cy="18" r="2"/>',
    lyrics: '<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/><path d="M8 8.5h8M8 12.5h5"/>',
    mic: '<path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><line x1="12" y1="19" x2="12" y2="22"/>',
    mini: '<rect x="2.5" y="4.5" width="19" height="15" rx="2"/><rect x="12" y="11" width="7" height="6" rx="1"/>',
    device: '<rect x="5" y="2.5" width="14" height="19" rx="2.5"/><circle cx="12" cy="14" r="3.5"/><line x1="12" y1="6.5" x2="12.01" y2="6.5"/>',
    sliders: '<line x1="4" y1="21" x2="4" y2="14"/><line x1="4" y1="10" x2="4" y2="3"/><line x1="12" y1="21" x2="12" y2="12"/><line x1="12" y1="8" x2="12" y2="3"/><line x1="20" y1="21" x2="20" y2="16"/><line x1="20" y1="12" x2="20" y2="3"/><line x1="1" y1="14" x2="7" y2="14"/><line x1="9" y1="8" x2="15" y2="8"/><line x1="17" y1="16" x2="23" y2="16"/>',
    wave: '<path d="M2 12h1.5M6 8.5v7M10 4.5v15M14 7.5v9M18 10v4M21.5 12H22"/>',
    sparkle: '<path d="M12 3.5l1.9 5 5 1.9-5 1.9-1.9 5-1.9-5-5-1.9 5-1.9z"/><path d="M19 15.5l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z"/>',
    refresh: '<polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/>',
    check: '<polyline points="20 6 9 17 4 12"/>',
    grip: '<circle cx="9" cy="6" r=".9"/><circle cx="15" cy="6" r=".9"/><circle cx="9" cy="12" r=".9"/><circle cx="15" cy="12" r=".9"/><circle cx="9" cy="18" r=".9"/><circle cx="15" cy="18" r=".9"/>',
    sort: '<path d="M7 4v16M3.5 16.5 7 20l3.5-3.5M17 20V4M13.5 7.5 17 4l3.5 3.5"/>',
    filter: '<polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3"/>',
    info: '<circle cx="12" cy="12" r="9.5"/><line x1="12" y1="16" x2="12" y2="11.5"/><line x1="12" y1="8" x2="12.01" y2="8"/>',
    sidebar: '<rect x="3" y="3.5" width="18" height="17" rx="2.5"/><line x1="9.5" y1="3.5" x2="9.5" y2="20.5"/>',
    radio: '<circle cx="12" cy="12" r="2"/><path d="M16.24 7.76a6 6 0 0 1 0 8.49m-8.48-.01a6 6 0 0 1 0-8.49m11.31-2.82a10 10 0 0 1 0 14.14m-14.14 0a10 10 0 0 1 0-14.14"/>',
    cmd: '<path d="M18 3a3 3 0 0 0-3 3v12a3 3 0 0 0 3 3 3 3 0 0 0 3-3 3 3 0 0 0-3-3H6a3 3 0 0 0-3 3 3 3 0 0 0 3 3 3 3 0 0 0 3-3V6a3 3 0 0 0-3-3 3 3 0 0 0-3 3 3 3 0 0 0 3 3h12a3 3 0 0 0 3-3 3 3 0 0 0-3-3z"/>',
    layers: '<polygon points="12 2 2 7 12 12 22 7 12 2"/><polyline points="2 17 12 22 22 17"/><polyline points="2 12 12 17 22 12"/>',
    star: '<polygon points="12 2.5 14.94 8.46 21.5 9.41 16.75 14.04 17.87 20.58 12 17.5 6.13 20.58 7.25 14.04 2.5 9.41 9.06 8.46 12 2.5"/>',
    sun: '<circle cx="12" cy="12" r="4.5"/><path d="M12 1.5v2M12 20.5v2M4.2 4.2l1.4 1.4M18.4 18.4l1.4 1.4M1.5 12h2M20.5 12h2M4.2 19.8l1.4-1.4M18.4 5.6l1.4-1.4"/>',
    moon: '<path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/>',
    activity: '<polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/>',
    download: '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/>',
    plusSquare: '<rect x="3" y="3" width="18" height="18" rx="2.5"/><line x1="12" y1="8" x2="12" y2="16"/><line x1="8" y1="12" x2="16" y2="12"/>',
    history: '<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><polyline points="3 3 3 8 8 8"/><polyline points="12 7 12 12 15 14"/>',
    playNext: '<path d="M3 6h12M3 12h8M3 18h8"/><path d="M15 12.5v6.5l5-3.25z"/>',
  };

  L.icon = function (name, opts) {
    opts = opts || {};
    const size = opts.size || 20, sw = opts.stroke || 1.75;
    if (FILLED[name]) {
      return `<svg class="ic ic-${name}" width="${size}" height="${size}" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="${opts.filledStroke || 1.6}" stroke-linejoin="round" aria-hidden="true">${FILLED[name]}</svg>`;
    }
    const body = STROKE[name] || STROKE.info;
    return `<svg class="ic ic-${name}" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;
  };

  L.hydrateIcons = function (root, opts) {
    (root || document).querySelectorAll('i[data-i]').forEach(el => {
      const o = Object.assign({}, opts);
      if (el.dataset.s) o.size = +el.dataset.s;
      if (el.dataset.w) o.stroke = +el.dataset.w;
      const tmp = document.createElement('span');
      tmp.innerHTML = L.icon(el.dataset.i, o);
      const svg = tmp.firstChild;
      if (el.className) svg.classList.add(...el.className.split(/\s+/).filter(Boolean));
      el.replaceWith(svg);
    });
  };

  // Logo: a peeled lychee that doubles as a vinyl record.
  // Red skin = record with faint grooves, pearl flesh = label, seed = spindle, plus a leaf.
  let uid = 0;
  L.logo = function (opts) {
    opts = opts || {};
    const size = opts.size || 28, id = 'lz' + (++uid);
    if (opts.mono) {
      const c = opts.color || 'currentColor', bg = opts.bg || 'transparent';
      return `<svg width="${size}" height="${size}" viewBox="0 0 32 32" aria-label="Lychee">
        <path d="M19.6 7.4c1.2-3.3 4.3-5 7.6-4.6-.4 3.3-3.3 5.6-6.6 5.4" fill="${c}"/>
        <circle cx="15" cy="17.5" r="12" fill="${c}"/>
        <circle cx="15" cy="17.5" r="5.2" fill="${bg === 'transparent' ? '#fff' : bg}"/>
        <circle cx="15" cy="17.5" r="1.5" fill="${c}"/></svg>`;
    }
    return `<svg width="${size}" height="${size}" viewBox="0 0 32 32" aria-label="Lychee">
      <defs>
        <radialGradient id="${id}s" cx="32%" cy="28%" r="80%"><stop offset="0" stop-color="#FF9AAE"/><stop offset=".5" stop-color="#F0435F"/><stop offset="1" stop-color="#A50E35"/></radialGradient>
        <radialGradient id="${id}f" cx="40%" cy="35%" r="70%"><stop offset="0" stop-color="#FFFFFF"/><stop offset="1" stop-color="#F3E6DF"/></radialGradient>
        <linearGradient id="${id}l" x1="0" y1="1" x2="1" y2="0"><stop offset="0" stop-color="#4FA65B"/><stop offset="1" stop-color="#A6E38A"/></linearGradient>
      </defs>
      <path d="M19.6 7.4c1.2-3.3 4.3-5 7.6-4.6-.4 3.3-3.3 5.6-6.6 5.4" fill="url(#${id}l)"/>
      <circle cx="15" cy="17.5" r="12" fill="url(#${id}s)"/>
      <g fill="none" stroke="#fff" stroke-opacity=".16" stroke-width=".6">
        <circle cx="15" cy="17.5" r="10.2"/><circle cx="15" cy="17.5" r="8.6"/><circle cx="15" cy="17.5" r="7"/></g>
      <circle cx="15" cy="17.5" r="5.2" fill="url(#${id}f)"/>
      <circle cx="15" cy="17.5" r="1.5" fill="#4A2419"/></svg>`;
  };
})(window.LYCHEE);
