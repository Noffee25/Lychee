/* Lychee design mockups: fictional sample library.
   Every artist, album, track and lyric here is invented for the mockups. */
window.LYCHEE = window.LYCHEE || {};

(function (L) {
  'use strict';

  // cover.style picks a generator in covers.js; cover.p is its palette.
  // accent: vivid colour pulled from the art (dynamic theming); tint: dark base colour.
  L.albums = [
    { id: 'tidal-memory', title: 'Tidal Memory', artist: 'Hollow Coast', year: 2024, genre: 'Dream Pop',
      format: 'FLAC', res: '24-bit / 96 kHz', short: '24/96', kbps: 2304, hires: true, tracks: 10, minutes: 43,
      code: 'HC-014', label: 'Driftwood Records', added: '2 days ago', plays: 412,
      accent: '#FF7A5C', tint: '#1B3A55',
      cover: { style: 'sun', seed: 7, p: ['#1B3A55', '#F3A27F', '#FFE6B8', '#FF6F61', '#173049'], label: 'HOLLOW COAST', labelColor: '#FFE6B8', labelTop: true } },
    { id: 'glasshouse', title: 'Glasshouse', artist: 'Mira Valen', year: 2023, genre: 'Art Pop',
      format: 'FLAC', res: '16-bit / 44.1 kHz', short: '16/44', kbps: 1411, hires: false, tracks: 11, minutes: 47,
      code: 'MV-003', label: 'Prism Audio', added: '1 week ago', plays: 288,
      accent: '#A98BFF', tint: '#2A2350',
      cover: { style: 'blob', seed: 12, p: ['#E9E4FF', '#B8A4FF', '#FF9EC4', '#7FD3FF', '#FFFFFF'], mark: 'ring', markColor: '#FFFFFF' } },
    { id: 'night-transit', title: 'Night Transit', artist: 'The Quiet Engines', year: 2022, genre: 'Synthwave',
      format: 'MP3', res: '320 kbps', short: '320k', kbps: 320, hires: false, tracks: 9, minutes: 41,
      code: 'QE-022', label: 'Afterhours', added: '3 weeks ago', plays: 951,
      accent: '#FF3E8A', tint: '#1A0B3D',
      cover: { style: 'synth', seed: 3, p: ['#0B0630', '#6A0F6E', '#FFD36E', '#FF3E8A', '#3EE6FF'] } },
    { id: 'low-light-radio', title: 'Low Light Radio', artist: 'Juniper Static', year: 2021, genre: 'Indie Rock',
      format: 'FLAC', res: '16-bit / 44.1 kHz', short: '16/44', kbps: 1411, hires: false, tracks: 12, minutes: 44,
      code: 'JS-108', label: 'Basement Tapes', added: '1 month ago', plays: 167,
      accent: '#E8B931', tint: '#2B2418',
      cover: { style: 'eq', seed: 21, p: ['#E8B931', '#C2482B', '#1E1E24', '#F3E9D2'], label: 'JUNIPER STATIC', labelColor: '#1E1E24', labelTop: true } },
    { id: 'golden-hour', title: 'Golden Hour Sessions', artist: 'Saffron Hills', year: 2020, genre: 'Soul',
      format: 'ALAC', res: '24-bit / 48 kHz', short: '24/48', kbps: 1960, hires: true, tracks: 8, minutes: 38,
      code: 'SH-201', label: 'Honeycomb', added: '1 month ago', plays: 230,
      accent: '#F2A541', tint: '#2E1C14',
      cover: { style: 'rings', seed: 5, p: ['#2E1C14', '#F2A541', '#E0762F', '#F6E7CB', '#8C4A2F'], cx: 128, cy: 276, step: 34, n: 11 } },
    { id: 'kaleidoscope', title: 'Kaleidoscope Theory', artist: 'Nils Arden', year: 2025, genre: 'Electronic',
      format: 'FLAC', res: '24-bit / 48 kHz', short: '24/48', kbps: 2116, hires: true, tracks: 10, minutes: 52,
      code: 'NA-011', label: 'Lattice', added: '2 months ago', plays: 344,
      accent: '#E63946', tint: '#1D3557',
      cover: { style: 'grid', seed: 44, p: ['#F1EBDD', '#E63946', '#1D3557', '#F4A261', '#2A9D8F'], n: 4 } },
    { id: 'neon-bloom', title: 'Neon Bloom', artist: 'Velvet Arcade', year: 2023, genre: 'Synth-pop',
      format: 'AAC', res: '256 kbps', short: '256k', kbps: 256, hires: false, tracks: 10, minutes: 39,
      code: 'VA-077', label: 'Afterhours', added: '2 months ago', plays: 512,
      accent: '#F72585', tint: '#17052E',
      cover: { style: 'orb', seed: 9, p: ['#0D0221', '#F72585', '#7209B7', '#4CC9F0'] } },
    { id: 'field-notes', title: 'Field Notes', artist: 'Aster & Pine', year: 2019, genre: 'Folk',
      format: 'FLAC', res: '16-bit / 44.1 kHz', short: '16/44', kbps: 1411, hires: false, tracks: 11, minutes: 46,
      code: 'AP-019', label: 'Woodgrain', added: '3 months ago', plays: 198,
      accent: '#7FA86A', tint: '#26362A',
      cover: { style: 'mountains', seed: 17, p: ['#F3E9D2', '#EBCBA4', '#E76F51', '#A3B18A', '#6F8F5E', '#3A5A40', '#263F2E'] } },
    { id: 'komorebi', title: 'Sunlight Through Leaves', artist: 'Komorebi', year: 2024, genre: 'Ambient',
      format: 'FLAC', res: '24-bit / 96 kHz', short: '24/96', kbps: 2470, hires: true, tracks: 7, minutes: 55,
      code: 'KM-004', label: 'Canopy', added: '3 months ago', plays: 276,
      accent: '#9BD14B', tint: '#1F3324',
      cover: { style: 'blob', seed: 31, p: ['#F5F3D7', '#C5E063', '#6BBF59', '#2A9D8F', '#FFE66D'], mark: 'dot', markColor: '#FFFDF0' } },
    { id: 'paper-moons', title: 'Paper Moons', artist: 'Marlowe Sun', year: 2018, genre: 'Singer-Songwriter',
      format: 'MP3', res: 'V0 ~245 kbps', short: 'V0', kbps: 245, hires: false, tracks: 12, minutes: 49,
      code: 'MS-002', label: 'Lamplight', added: '4 months ago', plays: 143,
      accent: '#D7263D', tint: '#22223B',
      cover: { style: 'crescent', seed: 2, p: ['#F1E9DA', '#22223B', '#D7263D', '#22223B'] } },
    { id: 'afterimage', title: 'Afterimage', artist: 'Delta Bloom', year: 2025, genre: 'Electronic',
      format: 'WAV', res: '24-bit / 48 kHz', short: '24/48', kbps: 2304, hires: true, tracks: 8, minutes: 42,
      code: 'DB-031', label: 'Lattice', added: '4 months ago', plays: 87,
      accent: '#FF6B35', tint: '#1A1214',
      cover: { style: 'halftone', seed: 13, p: ['#101014', '#FF6B35', '#2EC4B6'], label: 'AFTERIMAGE', labelColor: '#F4F4F4' } },
    { id: 'undertow', title: 'Undertow', artist: 'Coral Theory', year: 2022, genre: 'Post-Rock',
      format: 'FLAC', res: '24-bit / 44.1 kHz', short: '24/44', kbps: 1980, hires: true, tracks: 6, minutes: 51,
      code: 'CT-006', label: 'Deep End', added: '5 months ago', plays: 121,
      accent: '#48CAE4', tint: '#03045E',
      cover: { style: 'waves', seed: 23, p: ['#03045E', '#FF6F59', '#0077B6', '#48CAE4', '#CAF0F8'] } },
    { id: 'small-fires', title: 'Small Fires', artist: 'The Lantern Society', year: 2021, genre: 'Indie Folk',
      format: 'OGG', res: 'Vorbis q6', short: 'q6', kbps: 192, hires: false, tracks: 10, minutes: 37,
      code: 'LS-010', label: 'Woodgrain', added: '5 months ago', plays: 205,
      accent: '#F48C06', tint: '#2B1B10',
      cover: { style: 'sun', seed: 41, p: ['#140E0B', '#5A2A10', '#FFD166', '#E85D04', '#1E140E'] } },
    { id: 'wild-geometry', title: 'Wild Geometry', artist: 'Ivy Kestrel', year: 2024, genre: 'Alt R&B',
      format: 'FLAC', res: '24-bit / 48 kHz', short: '24/48', kbps: 2048, hires: true, tracks: 9, minutes: 36,
      code: 'IK-009', label: 'Prism Audio', added: '6 months ago', plays: 330,
      accent: '#06D6A0', tint: '#1B0B3A',
      cover: { style: 'grid', seed: 88, p: ['#1B0B3A', '#06D6A0', '#F15BB5', '#FEE440', '#9B5DE5'], n: 3 } },
    { id: 'monochrome-parade', title: 'Monochrome Parade', artist: 'Sable Echo', year: 2020, genre: 'Darkwave',
      format: 'FLAC', res: '16-bit / 44.1 kHz', short: '16/44', kbps: 1411, hires: false, tracks: 10, minutes: 45,
      code: 'SE-404', label: 'Basement Tapes', added: '7 months ago', plays: 98,
      accent: '#E5383B', tint: '#141414',
      cover: { style: 'stripes', seed: 64, p: ['#0E0E0E', '#F2F2F2', '#F2F2F2', '#6B6B6B', '#E5383B'], diag: true } },
    { id: 'cities-of-light', title: 'Cities of Light', artist: 'Polaris Avenue', year: 2023, genre: 'House',
      format: 'MP3', res: '320 kbps', short: '320k', kbps: 320, hires: false, tracks: 12, minutes: 68,
      code: 'PA-112', label: 'Afterhours', added: '8 months ago', plays: 404,
      accent: '#FFD166', tint: '#0B1D3A',
      cover: { style: 'skyline', seed: 52, p: ['#081530', '#3A2F6B', '#060D1A', '#FFD166', '#FF7AA2'] } },
    { id: 'satellites', title: 'Satellites', artist: 'Lumen Drift', year: 2025, genre: 'Ambient',
      format: 'Opus', res: '160 kbps', short: '160k', kbps: 160, hires: false, tracks: 8, minutes: 49,
      code: 'LD-008', label: 'Canopy', added: '9 months ago', plays: 66,
      accent: '#6FFFE9', tint: '#0B132B',
      cover: { style: 'orbits', seed: 77, p: ['#0B132B', '#C9F2FF', '#5BC0BE', '#F4F4F4', '#FFB4A2', '#6FFFE9'] } },
    { id: 'velour', title: 'Velour', artist: 'Oda Marsh', year: 2022, genre: 'Neo-Soul',
      format: 'FLAC', res: '24-bit / 96 kHz', short: '24/96', kbps: 2611, hires: true, tracks: 10, minutes: 44,
      code: 'OM-012', label: 'Honeycomb', added: '10 months ago', plays: 187,
      accent: '#E5989B', tint: '#3B0A12',
      cover: { style: 'blob', seed: 58, p: ['#2A060D', '#A3243B', '#E5989B', '#FFCDB2'], count: 1, mark: 'ring', markColor: '#FFCDB2' } },
  ];

  L.album = function (id) { return L.albums.find(a => a.id === id); };

  // Track lists for the albums the mockups open.
  L.tracklists = {
    'tidal-memory': [
      { n: 1, title: 'Low Tide Choir', len: 228, plays: 52, liked: false },
      { n: 2, title: 'Lantern Season', len: 245, plays: 61, liked: true },
      { n: 3, title: 'Saltwater Hymn', len: 252, plays: 88, liked: true },
      { n: 4, title: 'Glass Harbor', len: 211, plays: 40, liked: false },
      { n: 5, title: 'Undertow Letters', len: 302, plays: 37, liked: false },
      { n: 6, title: 'The Lighthouse Keeps Time', len: 267, plays: 45, liked: true },
      { n: 7, title: 'Seafoam Static', len: 196, plays: 29, liked: false },
      { n: 8, title: 'Northern Pier', len: 284, plays: 33, liked: false },
      { n: 9, title: 'All the Boats Come Home', len: 238, plays: 26, liked: false },
      { n: 10, title: 'Tidal Memory', len: 381, plays: 31, liked: true },
    ],
  };

  // Generic track names so any album can be "played" in the prototypes.
  const genericTitles = ['Opening', 'Afterglow', 'Static Bloom', 'Paper Planes', 'Midnight Signal', 'Quiet Hours',
    'Satellite Heart', 'Gold Leaf', 'Slow Motion', 'Weather Report', 'Northbound', 'Closing Credits'];
  L.tracksFor = function (album) {
    if (L.tracklists[album.id]) return L.tracklists[album.id];
    const out = [];
    for (let i = 0; i < Math.min(album.tracks, genericTitles.length); i++) {
      out.push({ n: i + 1, title: i === 0 ? album.title : genericTitles[(i + album.title.length) % genericTitles.length],
        len: 170 + ((i * 47 + album.title.length * 13) % 160), plays: 10 + ((i * 7 + album.year) % 60), liked: i % 4 === 1 });
    }
    return out;
  };

  // What is playing when a mockup opens.
  L.state = { albumId: 'tidal-memory', trackIndex: 2, position: 134, playing: true, shuffle: false, repeat: 'all', volume: 0.72 };

  // Synced lyrics (invented) for "Saltwater Hymn". t = seconds.
  L.lyrics = [
    { t: 12, text: 'Paper lanterns on the harbor wall' },
    { t: 18, text: 'Every window holding someone’s call' },
    { t: 25, text: 'I kept your letters in a tin of rain' },
    { t: 31, text: 'And read them backwards till they’re new again' },
    { t: 44, text: 'Oh, the tide comes in like it remembers' },
    { t: 50, text: 'Every name we wrote in the September sand' },
    { t: 57, text: 'Hold the light, hold the light' },
    { t: 63, text: 'Saltwater hymn in a borrowed key' },
    { t: 69, text: 'Singing you home, singing you back to me' },
    { t: 82, text: 'There’s a boat with no name in the bay' },
    { t: 88, text: 'Painted blue for the ones who stay' },
    { t: 95, text: 'And the gulls know the words by now' },
    { t: 101, text: 'So they carry the chorus somehow' },
    { t: 114, text: 'Oh, the tide comes in like it remembers' },
    { t: 120, text: 'Every promise that we buried in the sand' },
    { t: 128, text: 'Hold the light, hold the light' },
    { t: 134, text: 'Saltwater hymn in a borrowed key' },
    { t: 141, text: 'Singing you home, singing you back to me' },
    { t: 152, text: 'Wherever the current is taking you' },
    { t: 158, text: 'I’ll be the lantern you’re rowing to' },
    { t: 171, text: 'Hold the light' },
    { t: 178, text: 'Hold the light, hold the light' },
    { t: 186, text: 'Saltwater hymn, saltwater hymn' },
    { t: 205, text: 'Singing you home' },
  ];

  // Up Next: rest of the album, then tracks queued by hand.
  L.queue = [
    { title: 'Glass Harbor', albumId: 'tidal-memory', len: 211 },
    { title: 'Undertow Letters', albumId: 'tidal-memory', len: 302 },
    { title: 'The Lighthouse Keeps Time', albumId: 'tidal-memory', len: 267 },
    { title: 'Prism Room', albumId: 'glasshouse', len: 236, manual: true },
    { title: 'Night Bus Home', albumId: 'night-transit', len: 274, manual: true },
    { title: 'Amber Static', albumId: 'low-light-radio', len: 198, manual: true },
    { title: 'Golden Hour', albumId: 'golden-hour', len: 289, manual: true },
    { title: 'Lissajous', albumId: 'kaleidoscope', len: 331, manual: true },
  ];

  L.playlists = [
    { name: 'Favorites', count: 214, kind: 'liked', albums: ['tidal-memory', 'glasshouse', 'velour', 'neon-bloom'] },
    { name: 'Late Night Drive', count: 38, albums: ['night-transit', 'neon-bloom', 'cities-of-light', 'afterimage'] },
    { name: 'Focus Flow', count: 52, albums: ['komorebi', 'satellites', 'kaleidoscope', 'undertow'] },
    { name: 'Sunday Morning', count: 27, albums: ['field-notes', 'paper-moons', 'golden-hour', 'small-fires'] },
    { name: 'Rainy Day Soul', count: 31, albums: ['velour', 'golden-hour', 'wild-geometry', 'glasshouse'] },
  ];

  L.stats = { tracks: 2931, albums: 214, artists: 88, genres: 23, size: '412 GB', duration: '8 days 4 hrs' };

  // Helpers shared by every mockup.
  L.fmt = function (s) { s = Math.max(0, Math.round(s)); return Math.floor(s / 60) + ':' + String(s % 60).padStart(2, '0'); };
  L.params = new URLSearchParams(location.search);
  L.esc = function (s) { return String(s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c])); };

  // Deterministic pseudo-waveform: quiet intro, verses, louder choruses, fade-out.
  L.waveform = function (n, seed) {
    let a = (seed || 1) * 9301 + 49297;
    const rnd = () => ((a = (a * 9301 + 49297) % 233280) / 233280);
    const out = [];
    for (let i = 0; i < n; i++) {
      const x = i / (n - 1);
      let env = 0.35 + 0.25 * Math.sin(x * Math.PI * 5.2 - 1.2) + 0.25 * Math.sin(x * Math.PI * 2.1);
      env *= Math.min(1, x * 9) * Math.min(1, (1 - x) * 7);
      out.push(Math.max(0.08, Math.min(1, env * (0.65 + rnd() * 0.5))));
    }
    return out;
  };
})(window.LYCHEE);
