/* Lychee design mockups: generated placeholder album covers.
   Every cover is drawn as SVG from a seed and a palette, so the mockups contain no real
   (copyrighted) artwork. LYCHEE.coverURL(album) returns a data: URL usable in <img> or CSS. */
(function (L) {
  'use strict';

  function rng(seed) { // mulberry32
    let a = (seed * 2654435761) >>> 0;
    return function () {
      a = (a + 0x6D2B79F5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }
  function hexToRgb(h) {
    h = h.replace('#', '');
    if (h.length === 3) h = h.split('').map(c => c + c).join('');
    const n = parseInt(h, 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  }
  function rgbToHex(r, g, b) {
    return '#' + [r, g, b].map(v => Math.round(Math.max(0, Math.min(255, v))).toString(16).padStart(2, '0')).join('');
  }
  function mix(a, b, t) {
    const A = hexToRgb(a), B = hexToRgb(b);
    return rgbToHex(A[0] + (B[0] - A[0]) * t, A[1] + (B[1] - A[1]) * t, A[2] + (B[2] - A[2]) * t);
  }
  function ramp(stops, t) {
    if (stops.length === 1) return stops[0];
    const x = Math.max(0, Math.min(1, t)) * (stops.length - 1);
    const i = Math.min(stops.length - 2, Math.floor(x));
    return mix(stops[i], stops[i + 1], x - i);
  }
  const f = n => +n.toFixed(1);
  const lin = (id, a, b, x2 = 0, y2 = 1) =>
    `<linearGradient id="${id}" x1="0" y1="0" x2="${x2}" y2="${y2}"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient>`;
  const blur = (id, sd) => `<filter id="${id}" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="${sd}"/></filter>`;

  const STYLES = {
    // Soft sunset over water with a striped reflection.
    sun(r, p) {
      const [skyA, skyB, sunA, sunB, sea] = p;
      const hz = 258, cy = 200, R = 94;
      let bars = '';
      for (let i = 0; i < 9; i++) {
        const y = hz + 12 + i * 13 + i * i * 0.9;
        const w = 2 * R * (1 - i * 0.09) * (0.82 + r() * 0.18);
        const h = Math.max(1.4, 6 - i * 0.5);
        bars += `<rect x="${f(200 - w / 2 + (r() - 0.5) * 10)}" y="${f(y)}" width="${f(w)}" height="${f(h)}" rx="${f(h / 2)}" fill="${mix(sunB, sunA, i / 9)}" opacity="${f(0.9 - i * 0.08)}"/>`;
      }
      return `<defs>${lin('sky', skyA, skyB)}${lin('sun', sunA, sunB)}${lin('sea', sea, skyA)}${blur('glow', 30)}
          <clipPath id="above"><rect width="400" height="${hz}"/></clipPath></defs>
        <rect width="400" height="400" fill="url(#sky)"/>
        <circle cx="200" cy="${cy}" r="${R + 34}" fill="${sunB}" opacity=".5" filter="url(#glow)" clip-path="url(#above)"/>
        <circle cx="200" cy="${cy}" r="${R}" fill="url(#sun)" clip-path="url(#above)"/>
        <rect y="${hz}" width="400" height="${400 - hz}" fill="url(#sea)"/>
        <rect y="${hz}" width="400" height="1.2" fill="${sunA}" opacity=".35"/>${bars}`;
    },

    // Retro synth sun with cut stripes over a perspective grid.
    synth(r, p) {
      const [skyA, skyB, sunA, sunB, grid] = p;
      const hz = 250;
      let cuts = '';
      for (let i = 0; i < 7; i++) cuts += `<rect x="0" y="${f(178 + i * 11 + i * i * 0.7)}" width="400" height="${f(1.6 + i * 1.3)}" fill="#000"/>`;
      let lines = '';
      for (let i = 1; i <= 11; i++) { const t = i / 11; const y = hz + (400 - hz) * t * t; lines += `<line x1="0" y1="${f(y)}" x2="400" y2="${f(y)}"/>`; }
      for (let i = -10; i <= 10; i++) lines += `<line x1="${f(200 + i * 7)}" y1="${hz}" x2="${f(200 + i * 64)}" y2="400"/>`;
      return `<defs>${lin('sky', skyA, skyB)}${lin('sun', sunA, sunB)}${blur('glow', 26)}
          <linearGradient id="floor" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${skyB}"/><stop offset="1" stop-color="${skyA}"/></linearGradient>
          <mask id="cut"><rect width="400" height="400" fill="#fff"/>${cuts}</mask>
          <filter id="lg" x="-10%" y="-10%" width="120%" height="120%"><feGaussianBlur stdDeviation="1.8" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter></defs>
        <rect width="400" height="400" fill="url(#sky)"/>
        <circle cx="200" cy="168" r="124" fill="${sunB}" opacity=".38" filter="url(#glow)"/>
        <circle cx="200" cy="168" r="94" fill="url(#sun)" mask="url(#cut)"/>
        <rect y="${hz}" width="400" height="${400 - hz}" fill="url(#floor)"/>
        <g stroke="${grid}" stroke-width="1.5" opacity=".8" filter="url(#lg)">${lines}</g>`;
    },

    // Mesh-gradient blobs, optionally with a crisp focal mark.
    blob(r, p, o) {
      const [bg, ...cs] = p;
      let circles = '';
      cs.forEach((c, i) => {
        for (let k = 0; k < (o.count || 2); k++) {
          circles += `<circle cx="${f(30 + r() * 340)}" cy="${f(30 + r() * 340)}" r="${f(80 + r() * 90)}" fill="${c}" opacity="${f(0.7 + r() * 0.3)}"/>`;
        }
      });
      let mark = '';
      if (o.mark === 'ring') mark = `<circle cx="200" cy="200" r="78" fill="none" stroke="${o.markColor}" stroke-width="2.5" opacity=".9"/>`;
      if (o.mark === 'dot') mark = `<circle cx="272" cy="132" r="30" fill="${o.markColor}" opacity=".95"/>`;
      return `<defs>${blur('b', 46)}</defs><rect width="400" height="400" fill="${bg}"/><g filter="url(#b)">${circles}</g>${mark}`;
    },

    // Bauhaus tiles: quarter circles, half discs, triangles, dots.
    grid(r, p, o) {
      const [bg, ...cs] = p;
      const n = o.n || 4, s = 400 / n;
      const pick = () => cs[Math.floor(r() * cs.length)];
      let tiles = '';
      for (let y = 0; y < n; y++) for (let x = 0; x < n; x++) {
        if (r() < 0.12) continue;
        const c = pick(); let c2 = pick(); if (c2 === c) c2 = bg;
        const rot = Math.floor(r() * 4) * 90;
        let sh;
        switch (Math.floor(r() * 6)) {
          case 0: sh = `<rect width="${s}" height="${s}" fill="${c}"/><circle cx="${s / 2}" cy="${s / 2}" r="${f(s * 0.28)}" fill="${c2}"/>`; break;
          case 1: sh = `<path d="M0 0H${f(s)}A${f(s)} ${f(s)} 0 0 1 0 ${f(s)}Z" fill="${c}"/>`; break;
          case 2: sh = `<circle cx="${s / 2}" cy="${s / 2}" r="${f(s * 0.42)}" fill="${c}"/>`; break;
          case 3: sh = `<path d="M0 ${f(s)}A${f(s / 2)} ${f(s / 2)} 0 0 1 ${f(s)} ${f(s)}Z" fill="${c}"/>`; break;
          case 4: sh = `<path d="M0 0L${f(s)} ${f(s)}H0Z" fill="${c}"/>`; break;
          default: sh = `<rect width="${s}" height="${f(s / 2)}" fill="${c}"/><rect y="${f(s / 2)}" width="${s}" height="${f(s / 2)}" fill="${c2}"/>`;
        }
        tiles += `<g transform="translate(${f(x * s)} ${f(y * s)}) rotate(${rot} ${f(s / 2)} ${f(s / 2)})">${sh}</g>`;
      }
      return `<rect width="400" height="400" fill="${bg}"/>${tiles}`;
    },

    // Stacked wave bands.
    waves(r, p) {
      const [bg, ...cs] = p;
      const n = 10;
      let out = `<rect width="400" height="400" fill="${bg}"/>`;
      for (let i = 0; i < n; i++) {
        const y0 = 70 + i * 34, amp = 10 + r() * 16, ph = r() * Math.PI * 2, fr = 1 + r() * 1.2;
        let d = `M0 ${f(y0)}`;
        for (let x = 0; x <= 400; x += 8) d += `L${x} ${f(y0 + Math.sin((x / 400) * Math.PI * 2 * fr + ph) * amp)}`;
        out += `<path d="${d}L400 400L0 400Z" fill="${ramp(cs, i / (n - 1))}"/>`;
      }
      return out;
    },

    // Concentric rings around an off-centre point.
    rings(r, p, o) {
      const [bg, ...cs] = p;
      const cx = o.cx ?? 200, cy = o.cy ?? 200, n = o.n ?? 10, step = o.step ?? 30;
      let out = `<rect width="400" height="400" fill="${bg}"/>`;
      for (let i = n; i >= 1; i--) out += `<circle cx="${cx}" cy="${cy}" r="${i * step}" fill="${cs[i % cs.length]}"/>`;
      return out + `<circle cx="${cx}" cy="${cy}" r="9" fill="${bg}"/>`;
    },

    // Thin orbits, small planets and a starfield.
    orbits(r, p) {
      const [bg, line, ...pl] = p;
      let out = `<rect width="400" height="400" fill="${bg}"/>`;
      for (let i = 0; i < 80; i++) out += `<circle cx="${f(r() * 400)}" cy="${f(r() * 400)}" r="${f(0.4 + r() * 1.1)}" fill="#fff" opacity="${f(0.2 + r() * 0.6)}"/>`;
      const tilt = -18, rad = tilt * Math.PI / 180, cx = 200, cy = 214;
      out += `<g fill="none" stroke="${line}" stroke-opacity=".5" stroke-width="1.2">`;
      for (let i = 1; i <= 4; i++) out += `<ellipse cx="${cx}" cy="${cy}" rx="${40 + i * 38}" ry="${14 + i * 13}" transform="rotate(${tilt} ${cx} ${cy})"/>`;
      out += `</g><circle cx="${cx}" cy="${cy}" r="24" fill="${pl[0]}"/>`;
      for (let i = 1; i <= 4; i++) {
        const a = r() * Math.PI * 2, rx = 40 + i * 38, ry = 14 + i * 13;
        const x = rx * Math.cos(a), y = ry * Math.sin(a);
        out += `<circle cx="${f(cx + x * Math.cos(rad) - y * Math.sin(rad))}" cy="${f(cy + x * Math.sin(rad) + y * Math.cos(rad))}" r="${f(3 + r() * 6)}" fill="${pl[i % pl.length]}"/>`;
      }
      return out;
    },

    // Two offset halftone fields, like a risograph print.
    halftone(r, p) {
      const [bg, c1, c2] = p;
      const n = 25, s = 400 / n;
      let a = '', b = '';
      for (let y = 0; y < n; y++) for (let x = 0; x < n; x++) {
        const X = x * s + s / 2, Y = y * s + s / 2;
        const d1 = Math.hypot(X - 150, Y - 150) / 250, d2 = Math.hypot(X - 262, Y - 258) / 230;
        const r1 = Math.max(0, 1 - d1) ** 1.3 * s * 0.56, r2 = Math.max(0, 1 - d2) ** 1.5 * s * 0.5;
        if (r1 > 0.5) a += `<circle cx="${f(X)}" cy="${f(Y)}" r="${f(r1)}"/>`;
        if (r2 > 0.5) b += `<circle cx="${f(X + s / 2)}" cy="${f(Y + s / 2)}" r="${f(r2)}"/>`;
      }
      return `<rect width="400" height="400" fill="${bg}"/><g fill="${c1}">${a}</g><g fill="${c2}" style="mix-blend-mode:screen">${b}</g>`;
    },

    // Layered ridgelines under a low sun.
    mountains(r, p) {
      const [skyA, skyB, sun, ...ridges] = p;
      let out = `<defs>${lin('sky', skyA, skyB)}</defs><rect width="400" height="400" fill="url(#sky)"/><circle cx="276" cy="132" r="40" fill="${sun}"/>`;
      ridges.forEach((c, k) => {
        const base = 214 + k * 44, amp = 64 - k * 10, ph = r() * 10, ph2 = r() * 10;
        let d = `M0 400L0 ${f(base)}`;
        for (let x = 0; x <= 400; x += 8) {
          const y = base - amp * (0.55 * Math.abs(Math.sin(x / 90 + ph)) + 0.3 * Math.sin(x / 37 + ph2) + 0.15 * r());
          d += `L${x} ${f(y)}`;
        }
        out += `<path d="${d}L400 400Z" fill="${c}"/>`;
      });
      return out;
    },

    // City skyline at night with lit windows.
    skyline(r, p) {
      const [skyA, skyB, bld, win, glow] = p;
      let out = `<defs>${lin('sky', skyA, skyB)}${blur('gl', 34)}</defs><rect width="400" height="400" fill="url(#sky)"/>
        <ellipse cx="200" cy="340" rx="280" ry="100" fill="${glow}" opacity=".42" filter="url(#gl)"/>
        <circle cx="300" cy="86" r="22" fill="#FFF4D6" opacity=".9"/>`;
      let x = -8, b = '', w = '';
      while (x < 408) {
        const bw = 26 + r() * 42, bh = 96 + r() * 180, top = 400 - bh;
        b += `<rect x="${f(x)}" y="${f(top)}" width="${f(bw)}" height="${f(bh)}"/>`;
        for (let wy = top + 10; wy < 394; wy += 11) for (let wx = x + 5; wx < x + bw - 6; wx += 8) if (r() < 0.26) w += `<rect x="${f(wx)}" y="${f(wy)}" width="3.2" height="4.6"/>`;
        x += bw + 2;
      }
      return out + `<g fill="${bld}">${b}</g><g fill="${win}" opacity=".9">${w}</g>`;
    },

    // Crescent moon, small sun, star dust and a printed frame.
    crescent(r, p) {
      const [bg, moon, sun, ink] = p;
      let stars = '';
      for (let i = 0; i < 28; i++) stars += `<circle cx="${f(34 + r() * 332)}" cy="${f(34 + r() * 250)}" r="${f(0.8 + r() * 1.5)}"/>`;
      return `<defs><mask id="m"><rect width="400" height="400" fill="#fff"/><circle cx="240" cy="170" r="104" fill="#000"/></mask></defs>
        <rect width="400" height="400" fill="${bg}"/><g fill="${ink}" opacity=".5">${stars}</g>
        <circle cx="190" cy="200" r="118" fill="${moon}" mask="url(#m)"/>
        <circle cx="292" cy="294" r="24" fill="${sun}"/>
        <rect x="18" y="18" width="364" height="364" fill="none" stroke="${moon}" stroke-width="1.5"/>`;
    },

    // A glowing orb with faint halo rings.
    orb(r, p) {
      const [bg, a, b, c] = p;
      return `<defs><radialGradient id="o" cx="38%" cy="32%" r="72%"><stop offset="0" stop-color="${c}"/><stop offset=".38" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></radialGradient>${blur('bl', 38)}</defs>
        <rect width="400" height="400" fill="${bg}"/>
        <circle cx="200" cy="205" r="150" fill="${a}" opacity=".55" filter="url(#bl)"/>
        <circle cx="200" cy="205" r="116" fill="url(#o)"/>
        <circle cx="200" cy="205" r="158" fill="none" stroke="${c}" stroke-opacity=".35"/>
        <circle cx="200" cy="205" r="180" fill="none" stroke="${c}" stroke-opacity=".16"/>`;
    },

    // A big sun above a bar-graph skyline.
    eq(r, p) {
      const [bg, sun, bar, bar2] = p;
      const n = 14, w = 400 / n;
      let bars = '';
      for (let i = 0; i < n; i++) {
        const h = 50 + Math.abs(Math.sin(i * 0.8 + r() * 0.6)) * 150 * (0.55 + r() * 0.45);
        bars += `<rect x="${f(i * w + 3)}" y="${f(400 - h)}" width="${f(w - 6)}" height="${f(h)}" fill="${i % 3 === 1 ? bar2 : bar}"/>`;
      }
      return `<rect width="400" height="400" fill="${bg}"/><circle cx="200" cy="150" r="106" fill="${sun}"/>${bars}`;
    },

    // Diagonal or vertical bands.
    stripes(r, p, o) {
      const [bg, ...cs] = p;
      let bands = '', pos = -420;
      while (pos < 820) {
        const w = 10 + r() * 44;
        if (r() > 0.3) bands += `<rect x="${f(pos)}" y="-300" width="${f(w)}" height="1000" fill="${cs[Math.floor(r() * cs.length)]}"/>`;
        pos += w + 6 + r() * 26;
      }
      return `<rect width="400" height="400" fill="${bg}"/><g transform="rotate(${o.diag ? -35 : 0} 200 200)">${bands}</g>`;
    },
  };

  function noise(opacity) {
    return `<defs><filter id="grain" x="0" y="0" width="100%" height="100%">
        <feTurbulence type="fractalNoise" baseFrequency="0.9" numOctaves="2" seed="4" stitchTiles="stitch"/>
        <feColorMatrix type="saturate" values="0"/></filter></defs>
      <rect width="400" height="400" filter="url(#grain)" opacity="${opacity}" style="mix-blend-mode:overlay"/>`;
  }

  function coverSVG(album) {
    const c = album.cover;
    const gen = STYLES[c.style] || STYLES.blob;
    const body = gen(rng(c.seed || 1), c.p, c);
    const label = c.label
      ? `<text x="26" y="${c.labelTop ? 46 : 378}" font-family="Noto Sans, Helvetica, Arial, sans-serif" font-size="13" font-weight="700" letter-spacing="3.2" fill="${c.labelColor || '#fff'}" opacity=".92">${L.esc(c.label)}</text>`
      : '';
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 400" width="400" height="400">${body}${label}${noise(c.grain ?? 0.28)}</svg>`;
  }

  const cache = new Map();
  L.coverSVG = coverSVG;
  L.coverURL = function (album) {
    if (typeof album === 'string') album = L.album(album);
    if (!cache.has(album.id)) cache.set(album.id, 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(coverSVG(album)));
    return cache.get(album.id);
  };
  L.color = { mix, hexToRgb, rgbToHex, ramp };
})(window.LYCHEE);
