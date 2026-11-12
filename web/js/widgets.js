// Native interactive widgets for the reader. The desktop app renders the same
// `interactive` blocks; this file is the portable Canvas runtime.
//
// Canvas specs are JSON:
//   {"kind":"quadratic","a":1,"b":0,"c":0,"xMin":-4,"xMax":4,"yMin":-4,"yMax":8}
//   {"kind":"unitcircle"}
//   {"kind":"projectile","angle":45,"speed":14}

import { escapeHTML } from './docxml.js';

export function mountInteractive(block, book, lang) {
  if (block.kind === 'html') return mountHtml(block);
  return mountCanvas(block, book, lang);
}

export function disposeInteractive(root) {
  root.querySelectorAll('.widget').forEach(widget => widget.dispose?.());
}

function shell(block, lang, body) {
  const title = block.title || (lang === 'zh' ? '交互内容' : 'Interactive');
  const tag = block.kind === 'html' ? 'HTML' : 'Canvas';
  const fig = document.createElement('figure');
  fig.className = 'widget';
  fig.innerHTML = `
    <figcaption class="widget-head">
      <span class="widget-dot" aria-hidden="true"></span>
      <span class="widget-title">${escapeHTML(title)}</span>
      <span class="widget-tag">${tag}</span>
    </figcaption>`;
  fig.appendChild(body);
  return fig;
}

function mountHtml(block) {
  const frame = document.createElement('iframe');
  frame.className = 'widget-frame';
  frame.setAttribute('sandbox', 'allow-scripts');
  frame.setAttribute('title', block.title || 'HTML widget');
  frame.style.height = `${Math.max(140, block.height)}px`;
  const src = `<!doctype html><html><head><meta charset="utf-8">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' data: blob:; style-src 'unsafe-inline' data:; img-src data: blob:; font-src data:; media-src data: blob:; connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'"><style>
    html,body{margin:0;height:100%;font:14px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif;color:#22201c;background:#fff}
    body{display:grid;place-items:center;padding:16px;box-sizing:border-box}
    button{font:inherit;padding:7px 12px;border:1px solid #cfc8ba;border-radius:8px;background:#f7f3ea;cursor:pointer}
    button:hover{background:#efe8da}
  </style></head><body>${block.source || ''}</body></html>`;
  frame.srcdoc = src;
  return shell(block, document.documentElement.lang, frame);
}

function mountCanvas(block, book, lang) {
  let spec;
  try {
    spec = JSON.parse(block.source || '{}');
  } catch {
    spec = { kind: 'invalid' };
  }
  const body = document.createElement('div');
  body.className = 'widget-body';
  const wrap = document.createElement('div');
  wrap.className = 'widget-canvas-wrap';
  const canvas = document.createElement('canvas');
  canvas.className = 'widget-canvas';
  canvas.setAttribute('aria-label', block.title || 'Interactive canvas');
  wrap.appendChild(canvas);
  const controls = document.createElement('div');
  controls.className = 'widget-controls';
  body.append(wrap, controls);

  const accent = (book.theme && book.theme.accent) || '#b45309';
  const runtime = { canvas, wrap, controls, spec, accent, lang, book, redraw: null, stop: null };
  const fig = shell(block, lang === 'zh' ? 'zh' : 'en', body);
  fig.dataset.kind = spec.kind || 'invalid';

  let disposed = false;
  const frame = requestAnimationFrame(() => {
    if (disposed) return;
    const kind = String(spec.kind || '').toLowerCase();
    if (kind === 'quadratic') quadratic(runtime);
    else if (kind === 'unitcircle' || kind === 'unit-circle') unitCircle(runtime);
    else if (kind === 'projectile') projectile(runtime);
    else empty(runtime);
  });
  fig.dispose = () => { disposed = true; cancelAnimationFrame(frame); runtime.stop?.(); };
  return fig;
}

/* ---------- shared canvas plumbing ---------- */

function setupCanvas(rt, aspect = 0.56) {
  const { canvas, wrap } = rt;
  const ctx = canvas.getContext('2d');
  const resize = () => {
    const w = Math.max(260, wrap.clientWidth);
    const h = Math.round(w * aspect);
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    canvas.style.height = `${h}px`;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    rt.w = w;
    rt.h = h;
    rt.redraw?.();
  };
  const sizeObserver = new ResizeObserver(resize);
  sizeObserver.observe(wrap);
  const observer = new MutationObserver(() => rt.redraw?.());
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ['class', 'data-theme'] });
  rt.stop = () => { sizeObserver.disconnect(); observer.disconnect(); };
  resize();
  return ctx;
}

function css(name, fallback) {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v || fallback;
}
function palette() {
  return {
    ink: css('--ink', '#24211c'),
    muted: css('--muted', '#7a7264'),
    line: css('--line', '#d8d0c0'),
    grid: css('--grid', '#e9e2d4'),
    paper: css('--paper', '#fbf8f1'),
    accent: css('--accent', '#b45309'),
    accentSoft: css('--accent-soft', 'rgba(180,83,9,.12)'),
  };
}

function slider(rt, label, value, min, max, step, format, onInput) {
  const row = document.createElement('label');
  row.className = 'widget-control';
  const out = document.createElement('output');
  const input = document.createElement('input');
  input.type = 'range';
  input.min = String(min); input.max = String(max); input.step = String(step); input.value = String(value);
  input.setAttribute('aria-label', label);
  const paint = () => {
    out.textContent = format(Number(input.value));
    onInput(Number(input.value));
  };
  input.addEventListener('input', paint);
  row.innerHTML = `<span>${escapeHTML(label)}</span>`;
  row.append(input, out);
  rt.controls.appendChild(row);
  return { input, paint, out };
}

function button(rt, label, onClick) {
  const b = document.createElement('button');
  b.type = 'button';
  b.className = 'widget-button';
  b.textContent = label;
  b.addEventListener('click', onClick);
  rt.controls.appendChild(b);
  return b;
}

function readout(rt) {
  const el = document.createElement('div');
  el.className = 'widget-readout';
  rt.controls.appendChild(el);
  return el;
}

function axes(ctx, rt, xMin, xMax, yMin, yMax, step = 1) {
  const p = palette();
  const { w, h } = rt;
  const pad = 26;
  const sx = (x) => pad + ((x - xMin) / (xMax - xMin)) * (w - pad * 2);
  const sy = (y) => h - pad - ((y - yMin) / (yMax - yMin)) * (h - pad * 2);
  ctx.clearRect(0, 0, w, h);
  ctx.fillStyle = p.paper;
  ctx.fillRect(0, 0, w, h);
  ctx.strokeStyle = p.grid;
  ctx.lineWidth = 1;
  const gx = niceStep(xMax - xMin, 6);
  for (let x = Math.ceil(xMin / gx) * gx; x <= xMax; x += gx) {
    ctx.beginPath(); ctx.moveTo(sx(x), sy(yMin)); ctx.lineTo(sx(x), sy(yMax)); ctx.stroke();
  }
  for (let y = Math.ceil(yMin / gx) * gx; y <= yMax; y += gx) {
    ctx.beginPath(); ctx.moveTo(sx(xMin), sy(y)); ctx.lineTo(sx(xMax), sy(y)); ctx.stroke();
  }
  ctx.strokeStyle = p.muted;
  ctx.lineWidth = 1.4;
  if (yMin <= 0 && yMax >= 0) { ctx.beginPath(); ctx.moveTo(sx(xMin), sy(0)); ctx.lineTo(sx(xMax), sy(0)); ctx.stroke(); }
  if (xMin <= 0 && xMax >= 0) { ctx.beginPath(); ctx.moveTo(sx(0), sy(yMin)); ctx.lineTo(sx(0), sy(yMax)); ctx.stroke(); }
  ctx.fillStyle = p.muted;
  ctx.font = '11px ui-sans-serif, system-ui, sans-serif';
  ctx.textAlign = 'center'; ctx.textBaseline = 'top';
  for (let x = Math.ceil(xMin / gx) * gx; x <= xMax; x += gx) {
    if (Math.abs(x) < 1e-9) continue;
    ctx.fillText(String(Math.round(x * 10) / 10), sx(x), sy(0) + 4);
  }
  ctx.textAlign = 'right'; ctx.textBaseline = 'middle';
  for (let y = Math.ceil(yMin / gx) * gx; y <= yMax; y += gx) {
    if (Math.abs(y) < 1e-9) continue;
    ctx.fillText(String(Math.round(y * 10) / 10), sx(0) - 5, sy(y));
  }
  return { sx, sy, pad };
}

function niceStep(range, target) {
  const raw = range / target;
  const mag = Math.pow(10, Math.floor(Math.log10(raw)));
  const norm = raw / mag;
  return (norm >= 5 ? 5 : norm >= 2 ? 2 : 1) * mag;
}

function token(hex, alpha) {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex);
  if (!m) return hex;
  const n = parseInt(m[1], 16);
  return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${alpha})`;
}

/* ---------- quadratic explorer ---------- */

function quadratic(rt) {
  const spec = rt.spec;
  let a = num(spec.a, 1), b = num(spec.b, 0), c = num(spec.c, 0);
  const xMin = num(spec.xMin, -4), xMax = num(spec.xMax, 4), yMin = num(spec.yMin, -4), yMax = num(spec.yMax, 8);
  const ctx = setupCanvas(rt);
  const out = readout(rt);

  const draw = () => {
    const p = palette();
    const { sx, sy } = axes(ctx, rt, xMin, xMax, yMin, yMax);
    const f = (x) => a * x * x + b * x + c;
    ctx.strokeStyle = p.accent;
    ctx.lineWidth = 2.4;
    ctx.beginPath();
    let started = false;
    for (let i = 0; i <= 320; i++) {
      const x = xMin + ((xMax - xMin) * i) / 320;
      const y = f(x);
      if (y < yMin - 40 || y > yMax + 40) { started = false; continue; }
      if (!started) { ctx.moveTo(sx(x), sy(y)); started = true; } else ctx.lineTo(sx(x), sy(y));
    }
    ctx.stroke();

    const vx = a === 0 ? 0 : -b / (2 * a);
    const vy = f(vx);
    if (Number.isFinite(vx) && vy >= yMin && vy <= yMax) {
      ctx.fillStyle = p.accent;
      ctx.beginPath(); ctx.arc(sx(vx), sy(vy), 4.5, 0, Math.PI * 2); ctx.fill();
      ctx.fillStyle = p.ink;
      ctx.font = '12px ui-sans-serif, system-ui, sans-serif';
      ctx.textAlign = 'left'; ctx.textBaseline = 'bottom';
      ctx.fillText(`顶点 (${vx.toFixed(2)}, ${vy.toFixed(2)})`, Math.min(sx(vx) + 8, rt.w - 150), sy(vy) - 6);
    }
    const disc = b * b - 4 * a * c;
    const eq = `${a === 1 ? '' : a === -1 ? '−' : a.toFixed(1)}x² ${b < 0 ? '−' : '+'} ${Math.abs(b).toFixed(1)}x ${c < 0 ? '−' : '+'} ${Math.abs(c).toFixed(1)}`;
    out.innerHTML = `<span class="mono">y = ${escapeHTML(eq.replace('−', '-'))}</span><span>Δ = ${disc.toFixed(2)} · ${disc > 0 ? (rt.lang === 'zh' ? '两个实根' : 'two real roots') : disc === 0 ? (rt.lang === 'zh' ? '一个实根' : 'one real root') : rt.lang === 'zh' ? '无实根' : 'no real roots'}</span>`;
  };
  rt.redraw = draw;

  slider(rt, rt.lang === 'zh' ? '二次项 a' : 'a', a, -3, 3, 0.1, (v) => v.toFixed(1), (v) => { a = v; draw(); });
  slider(rt, rt.lang === 'zh' ? '一次项 b' : 'b', b, -6, 6, 0.1, (v) => v.toFixed(1), (v) => { b = v; draw(); });
  slider(rt, rt.lang === 'zh' ? '常数项 c' : 'c', c, -6, 6, 0.1, (v) => v.toFixed(1), (v) => { c = v; draw(); });
  button(rt, rt.lang === 'zh' ? '重置' : 'Reset', () => {
    a = num(spec.a, 1); b = num(spec.b, 0); c = num(spec.c, 0);
    [...rt.controls.querySelectorAll('input[type=range]')].forEach((input, i) => { input.value = String([a, b, c][i]); });
    draw();
  });
  draw();
}

/* ---------- unit circle ---------- */

function unitCircle(rt) {
  let deg = 35;
  const ctx = setupCanvas(rt, 0.72);
  const out = readout(rt);
  const draw = () => {
    const p = palette();
    const { w, h } = rt;
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = p.paper; ctx.fillRect(0, 0, w, h);
    const cx = w * 0.42, cy = h * 0.5, r = Math.min(w, h) * 0.31;
    ctx.strokeStyle = p.grid; ctx.lineWidth = 1;
    for (let i = -2; i <= 2; i++) {
      ctx.beginPath(); ctx.moveTo(cx + (i * r) / 1, cy - r * 1.3); ctx.lineTo(cx + (i * r) / 1, cy + r * 1.3); ctx.stroke();
      ctx.beginPath(); ctx.moveTo(cx - r * 1.3, cy + (i * r) / 1); ctx.lineTo(cx + r * 1.3, cy + (i * r) / 1); ctx.stroke();
    }
    ctx.strokeStyle = p.muted; ctx.lineWidth = 1.4;
    ctx.beginPath(); ctx.moveTo(cx - r * 1.3, cy); ctx.lineTo(cx + r * 1.3, cy); ctx.stroke();
    ctx.beginPath(); ctx.moveTo(cx, cy - r * 1.3); ctx.lineTo(cx, cy + r * 1.3); ctx.stroke();
    ctx.strokeStyle = p.ink; ctx.lineWidth = 2;
    ctx.beginPath(); ctx.arc(cx, cy, r, 0, Math.PI * 2); ctx.stroke();

    const rad = (deg * Math.PI) / 180;
    const x = Math.cos(rad), y = Math.sin(rad);
    const px = cx + x * r, py = cy - y * r;
    ctx.fillStyle = p.accentSoft;
    ctx.beginPath(); ctx.moveTo(cx, cy); ctx.arc(cx, cy, r, 0, -rad, true); ctx.closePath(); ctx.fill();
    ctx.strokeStyle = p.accent; ctx.lineWidth = 2;
    ctx.beginPath(); ctx.moveTo(cx, cy); ctx.lineTo(px, py); ctx.stroke();
    ctx.setLineDash([4, 4]);
    ctx.strokeStyle = p.muted; ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(px, py); ctx.lineTo(px, cy); ctx.stroke();
    ctx.beginPath(); ctx.moveTo(px, py); ctx.lineTo(cx, py); ctx.stroke();
    ctx.setLineDash([]);
    ctx.fillStyle = p.accent;
    ctx.beginPath(); ctx.arc(px, py, 5.5, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = p.ink;
    ctx.font = '12px ui-sans-serif, system-ui, sans-serif';
    ctx.textAlign = 'left'; ctx.textBaseline = 'middle';
    ctx.fillText(`sin ${y.toFixed(3)}`, cx + r * 0.02, py - 12);
    ctx.fillText(`cos ${x.toFixed(3)}`, px + 8, cy + 14);
    out.innerHTML = `<span class="mono">θ = ${deg}° = ${(rad).toFixed(3)} rad</span><span>sin² + cos² = ${(x * x + y * y).toFixed(3)}</span>`;
  };
  rt.redraw = draw;
  slider(rt, rt.lang === 'zh' ? '角度 θ' : 'Angle θ', deg, 0, 360, 1, (v) => `${v}°`, (v) => { deg = v; draw(); });
  draw();
}

/* ---------- projectile lab ---------- */

function projectile(rt) {
  let angle = num(rt.spec.angle, 45), speed = num(rt.spec.speed, 14);
  let t = 0, playing = false, raf = 0, started = 0;
  const g = 9.8;
  const ctx = setupCanvas(rt, 0.54);
  const stopObservers = rt.stop;
  rt.stop = () => { playing = false; cancelAnimationFrame(raf); stopObservers(); };
  const out = readout(rt);
  const duration = () => (2 * speed * Math.sin((angle * Math.PI) / 180)) / g;
  const world = () => {
    const a = (angle * Math.PI) / 180;
    const range = (speed * speed * Math.sin(2 * a)) / g;
    const hmax = (speed * speed * Math.sin(a) * Math.sin(a)) / (2 * g);
    return { range: Math.max(4, range), hmax: Math.max(1, hmax) };
  };
  const draw = () => {
    const p = palette();
    const { w, h } = rt;
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = p.paper; ctx.fillRect(0, 0, w, h);
    const { range, hmax } = world();
    const xMax = Math.max(5, range * 1.12), yMax = Math.max(2, hmax * 1.25);
    const pad = 28;
    const sx = (x) => pad + (x / xMax) * (w - pad * 2);
    const sy = (y) => h - pad - (y / yMax) * (h - pad * 2);
    ctx.strokeStyle = p.grid; ctx.lineWidth = 1;
    for (let x = 1; x <= xMax; x++) { ctx.beginPath(); ctx.moveTo(sx(x), sy(0)); ctx.lineTo(sx(x), sy(yMax)); ctx.stroke(); }
    for (let y = 1; y <= yMax; y++) { ctx.beginPath(); ctx.moveTo(sx(0), sy(y)); ctx.lineTo(sx(xMax), sy(y)); ctx.stroke(); }
    ctx.strokeStyle = p.muted; ctx.lineWidth = 1.4;
    ctx.beginPath(); ctx.moveTo(sx(0), sy(0)); ctx.lineTo(sx(xMax), sy(0)); ctx.stroke();
    ctx.beginPath(); ctx.moveTo(sx(0), sy(0)); ctx.lineTo(sx(0), sy(yMax)); ctx.stroke();

    const rad = (angle * Math.PI) / 180;
    const flight = duration();
    const now = Math.min(t, flight);
    const x = speed * Math.cos(rad) * now;
    const y = speed * Math.sin(rad) * now - 0.5 * g * now * now;
    ctx.strokeStyle = p.accent; ctx.lineWidth = 2.2;
    ctx.beginPath();
    for (let i = 0; i <= 120; i++) {
      const tt = (flight * i) / 120;
      const xx = speed * Math.cos(rad) * tt;
      const yy = speed * Math.sin(rad) * tt - 0.5 * g * tt * tt;
      if (i === 0) ctx.moveTo(sx(xx), sy(yy)); else ctx.lineTo(sx(xx), sy(yy));
    }
    ctx.stroke();
    ctx.fillStyle = p.accent;
    ctx.beginPath(); ctx.arc(sx(x), sy(Math.max(0, y)), 6, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = p.ink;
    ctx.font = '12px ui-sans-serif, system-ui, sans-serif';
    ctx.textAlign = 'left'; ctx.textBaseline = 'bottom';
    ctx.fillText(`x ${x.toFixed(2)} m · y ${Math.max(0, y).toFixed(2)} m`, Math.min(sx(x) + 10, w - 170), sy(Math.max(0, y)) - 8);
    out.innerHTML = `<span>${rt.lang === 'zh' ? '射程' : 'Range'} ${range.toFixed(1)} m</span><span>${rt.lang === 'zh' ? '最大高度' : 'Apex'} ${hmax.toFixed(1)} m</span><span>${rt.lang === 'zh' ? '飞行' : 'Flight'} ${flight.toFixed(2)} s</span>`;
  };
  rt.redraw = draw;
  const tick = (now) => {
    if (!playing) return;
    if (!started) started = now;
    t = Math.min((now - started) / 1000, duration());
    if (t >= duration()) playing = false;
    draw();
    if (playing) raf = requestAnimationFrame(tick);
    else play.textContent = rt.lang === 'zh' ? '再发射' : 'Launch again';
  };
  const play = button(rt, rt.lang === 'zh' ? '发射' : 'Launch', () => {
    cancelAnimationFrame(raf);
    t = 0; started = 0; playing = true; play.textContent = rt.lang === 'zh' ? '发射中…' : 'Flying…';
    raf = requestAnimationFrame(tick);
  });
  slider(rt, rt.lang === 'zh' ? '发射角' : 'Angle', angle, 10, 80, 1, (v) => `${v}°`, (v) => { angle = v; if (!playing) { t = 0; play.textContent = rt.lang === 'zh' ? '发射' : 'Launch'; draw(); } });
  slider(rt, rt.lang === 'zh' ? '初速度' : 'Speed', speed, 4, 24, 0.5, (v) => `${v.toFixed(1)} m/s`, (v) => { speed = v; if (!playing) { t = 0; draw(); } });
  draw();
}

function empty(rt) {
  const ctx = setupCanvas(rt, 0.5);
  const draw = () => {
    const p = palette();
    ctx.clearRect(0, 0, rt.w, rt.h);
    ctx.fillStyle = p.paper; ctx.fillRect(0, 0, rt.w, rt.h);
    ctx.fillStyle = p.muted;
    ctx.font = '13px ui-sans-serif, system-ui, sans-serif';
    ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
    ctx.fillText(rt.lang === 'zh' ? '这个画布还没有运行时' : 'No runtime for this canvas yet', rt.w / 2, rt.h / 2);
  };
  rt.redraw = draw;
  draw();
}

function num(v, fallback) {
  const n = Number(v);
  return Number.isFinite(n) ? n : fallback;
}
