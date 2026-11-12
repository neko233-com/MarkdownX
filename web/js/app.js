// MarkdownX reader shell: bookshelf, chapter navigation, rendering and
// language/theme persistence. No framework, no build step.

import { readBookFile, normalizeBook, assetURL, pick } from './mdxb.js';
import { disposeInteractive, mountInteractive } from './widgets.js';
import { color, escapeHTML } from './docxml.js';

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];
const store = {
  get(key, fallback) { try { return localStorage.getItem(key) ?? fallback; } catch { return fallback; } },
  set(key, value) { try { localStorage.setItem(key, value); } catch { /* private mode */ } },
};

const T = {
  zh: {
    chapter: '章', chapterOf: (i, n) => `第 ${i} 章 / 共 ${n} 章`, min: '分钟阅读',
    prev: '上一章', next: '下一章', open: '打开一本书', close: '关闭', contents: '目录',
    lang: 'EN', interactive: '交互内容', imageMissing: '图片未嵌入', copy: '复制', copied: '已复制',
    loading: '正在打开…', error: '打不开这本书', empty: '这本书还没有内容。',
    back: '返回书架', theme: '切换明暗',
  },
  en: {
    chapter: 'Chapter', chapterOf: (i, n) => `Chapter ${i} of ${n}`, min: 'min read',
    prev: 'Previous', next: 'Next', open: 'Open a book', close: 'Close', contents: 'Contents',
    lang: '中文', interactive: 'Interactive', imageMissing: 'Image not embedded', copy: 'Copy', copied: 'Copied',
    loading: 'Opening…', error: 'Could not open this book', empty: 'This book has no content yet.',
    back: 'Back to library', theme: 'Toggle theme',
  },
};

const state = {
  book: null,
  chapterIndex: 0,
  lang: store.get('mx-lang', 'zh'),
  theme: store.get('mx-theme', 'light'),
  examples: [],
};

function t(key, ...args) {
  const v = T[state.lang]?.[key] ?? T.en[key] ?? key;
  return typeof v === 'function' ? v(...args) : v;
}

/* ---------- theme ---------- */

function applyTheme(theme) {
  state.theme = theme === 'dark' ? 'dark' : 'light';
  document.documentElement.dataset.theme = state.theme;
  store.set('mx-theme', state.theme);
  const meta = $('meta[name="theme-color"]');
  if (meta) meta.content = state.theme === 'dark' ? '#171512' : '#fbf8f1';
  for (const b of [$('#landing-theme'), $('#reader-theme')]) {
    if (b) b.setAttribute('aria-pressed', state.theme === 'dark' ? 'true' : 'false');
  }
}

function applyBookTheme(book) {
  const accent = (book.theme && book.theme.accent) || '#b45309';
  document.documentElement.style.setProperty('--accent', accent);
  document.documentElement.style.setProperty('--accent-ink', shade(accent, state.theme === 'dark' ? 32 : -26));
  document.documentElement.style.setProperty('--accent-soft', rgba(accent, state.theme === 'dark' ? .16 : .11));
  document.documentElement.style.setProperty('--cover', accent);
  const cover = book.cover ? assetURL(book, book.cover) : '';
  const tocCover = $('#toc-cover');
  tocCover.innerHTML = '';
  if (cover) {
    tocCover.style.backgroundImage = `url("${cover.replace(/"/g, '%22')}")`;
    tocCover.style.background = `center / cover no-repeat url("${cover.replace(/"/g, '%22')}")`;
  } else {
    tocCover.style.backgroundImage = '';
    tocCover.style.background = `linear-gradient(135deg, ${accent}, ${shade(accent, -28)})`;
    const glyph = document.createElement('span');
    glyph.className = 'cover-glyph';
    glyph.textContent = 'M';
    tocCover.appendChild(glyph);
  }
}

function rgba(hex, a) {
  const n = parseInt(hex.replace('#', ''), 16);
  if (!Number.isFinite(n)) return `rgba(180,83,9,${a})`;
  return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${a})`;
}
function shade(hex, amount) {
  const n = parseInt(hex.replace('#', ''), 16);
  if (!Number.isFinite(n)) return hex;
  const clamp = (v) => Math.max(0, Math.min(255, v));
  const r = clamp((n >> 16) + amount), g = clamp(((n >> 8) & 255) + amount), b = clamp((n & 255) + amount);
  return `#${((r << 16) | (g << 8) | b).toString(16).padStart(6, '0')}`;
}

/* ---------- language ---------- */

function bookTitle() { return pick(state.book?.titleRaw, state.lang) || state.book?.title || ''; }
function bookSubtitle() { return pick(state.book?.subtitleRaw, state.lang) || state.book?.subtitle || ''; }
function chapterTitle(ch) { return pick(ch.titleRaw, state.lang) || ch.title || ''; }

function applyLang(lang) {
  state.lang = String(lang).toLowerCase().startsWith('en') ? 'en' : 'zh';
  store.set('mx-lang', state.lang);
  document.documentElement.lang = state.lang === 'zh' ? 'zh-CN' : 'en';
  const btn = $('#lang-toggle');
  if (btn) btn.textContent = t('lang');
  if (state.book) render();
}

/* ---------- examples ---------- */

async function loadExamples() {
  const grid = $('#example-grid');
  const note = $('#examples-note');
  try {
    const res = await fetch('../examples/manifest.json', { cache: 'no-cache' });
    if (!res.ok) throw new Error(String(res.status));
    const manifest = await res.json();
    state.examples = Array.isArray(manifest.books) ? manifest.books : [];
    if (!state.examples.length) throw new Error('empty');
    grid.innerHTML = '';
    state.examples.forEach((ex, i) => grid.appendChild(exampleCard(ex, i)));
    note.innerHTML = `示例是普通的 <code>.mdxb</code> 文件；也可以把自己的书拖进这个页面。`;
  } catch {
    grid.innerHTML = '';
    for (let i = 0; i < 3; i++) grid.appendChild(placeholderCard(i));
    note.innerHTML = state.lang === 'zh'
      ? `示例需要在本地服务器中打开：在仓库根目录运行 <code>python -m http.server 8765</code>，再访问 <code>http://localhost:8765/web/</code>。`
      : `Examples need a local server: run <code>python -m http.server 8765</code> at the repository root, then open <code>http://localhost:8765/web/</code>.`;
  }
}

function exampleCard(ex, index) {
  const meta = ex[state.lang] || ex.zh || ex.en || {};
  const btn = document.createElement('button');
  btn.type = 'button';
  btn.className = 'example-card';
  btn.style.setProperty('--cover', ex.cover?.bg || '#2f5d54');
  btn.style.setProperty('--cover-ink', ex.cover?.ink || '#ffffff');
  btn.innerHTML = `
    <div class="example-cover">
      <span class="ex-kicker">${escapeHTML(ex.kicker || 'MarkdownX Book')}</span>
      <strong>${escapeHTML(meta.title || ex.file)}</strong>
      <span class="ex-glyph" aria-hidden="true">${escapeHTML(ex.cover?.glyph || 'M')}</span>
      <span class="example-arrow" aria-hidden="true">↗</span>
    </div>
    <div class="example-body">
      <p>${escapeHTML(meta.blurb || '')}</p>
      <div class="example-tags">${(meta.tags || []).map((x) => `<span class="tag">${escapeHTML(x)}</span>`).join('')}</div>
    </div>`;
  btn.addEventListener('click', () => loadExample(ex, btn));
  return btn;
}

function placeholderCard(i) {
  const el = document.createElement('div');
  el.className = 'example-card';
  el.style.setProperty('--cover', ['#2f5d54', '#7c3f2b', '#3e4a6b'][i]);
  const titles = state.lang === 'zh' ? ['二次函数实验室', '抛物线物理', '双语排版样本'] : ['Quadratic Lab', 'Projectile Physics', 'Bilingual Type'];
  el.innerHTML = `<div class="example-cover"><span class="ex-kicker">MarkdownX Book</span><strong>${titles[i]}</strong><span class="ex-glyph">${['ƒ', '→', 'Aa'][i]}</span></div><div class="example-body"><p>${state.lang === 'zh' ? '用本地服务器打开 web/ 后即可阅读。' : 'Serve the web/ folder locally to read this sample.'}</p><div class="example-tags"><span class="tag">.mdxb</span></div></div>`;
  return el;
}

async function loadExample(ex, button) {
  if (button) { button.disabled = true; button.classList.add('loading'); }
  try {
    const res = await fetch(`../examples/${ex.file}`, { cache: 'no-cache' });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const raw = await res.json();
    openBook(normalizeBook(raw));
  } catch (err) {
    toast(`${t('error')}: ${err.message}`);
  } finally {
    if (button) { button.disabled = false; button.classList.remove('loading'); }
  }
}

/* ---------- opening / closing ---------- */

function openBook(book) {
  state.book = book;
  state.lang = book.languages?.some((l) => String(l).toLowerCase().startsWith('en')) && state.lang === 'en' ? 'en' : state.lang;
  const saved = Number(store.get(`mx-progress:${book.id || book.title}`, '0'));
  state.chapterIndex = Number.isInteger(saved) && saved >= 0 && saved < book.chapters.length ? saved : 0;
  applyTheme(state.theme);
  applyBookTheme(book);
  applyLang(state.lang);
  $('#landing').hidden = true;
  $('#reader').hidden = false;
  document.body.classList.add('reading');
  document.title = `${bookTitle()} · MarkdownX`;
  render();
  history.replaceState(null, '', '#read');
  window.scrollTo({ top: 0, behavior: 'instant' });
  $('#chapter').focus({ preventScroll: true });
}

function closeBook() {
  disposeInteractive($('#chapter'));
  $('#chapter').replaceChildren();
  state.book = null;
  $('#reader').hidden = true;
  $('#landing').hidden = false;
  document.body.classList.remove('reading');
  document.title = 'MarkdownX — 会动的书';
  document.documentElement.style.removeProperty('--accent');
  document.documentElement.style.removeProperty('--accent-ink');
  document.documentElement.style.removeProperty('--accent-soft');
  document.documentElement.style.removeProperty('--cover');
  history.replaceState(null, '', location.pathname);
  closeToc();
  window.scrollTo({ top: 0, behavior: 'instant' });
}

async function openFile(file) {
  if (!file) return;
  try {
    const book = await readBookFile(file);
    openBook(book);
  } catch (err) {
    toast(`${t('error')}: ${err.message}`);
  }
}

/* ---------- rendering ---------- */

function render() {
  if (!state.book) return;
  const book = state.book;
  const chapters = book.chapters;
  state.chapterIndex = Math.max(0, Math.min(state.chapterIndex, chapters.length - 1));
  const ch = chapters[state.chapterIndex];

  $('#topbar-title').textContent = `${bookTitle()} · ${chapterTitle(ch)}`;
  $('#lang-toggle').hidden = !hasEnglish(book);
  $('#toc-title').textContent = bookTitle();
  const sub = bookSubtitle();
  $('#toc-subtitle').textContent = sub;
  $('#toc-subtitle').hidden = !sub;
  $('#toc-author').textContent = book.authors?.length ? book.authors.join(' · ') : '';
  $('#toc-kicker').textContent = `${book.languages?.join(' / ') || '中文 / English'} · ${chapters.length} ${state.lang === 'zh' ? '章' : 'chapters'}`;
  $('#toc-count').textContent = `${chapters.length} ${state.lang === 'zh' ? '章' : 'chapters'}`;
  $('#reader-footer-title').textContent = bookTitle();

  renderToc();
  renderChapter(ch);
  renderChapterNav();
  updateProgress();
}

function renderToc() {
  const list = $('#toc-list');
  list.innerHTML = '';
  state.book.chapters.forEach((ch, i) => {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = `toc-item${i === state.chapterIndex ? ' active' : ''}`;
    b.setAttribute('aria-current', i === state.chapterIndex ? 'page' : 'false');
    b.innerHTML = `<span class="num">${String(i + 1).padStart(2, '0')}</span><span class="lbl">${escapeHTML(chapterTitle(ch))}</span>`;
    b.addEventListener('click', () => { setChapter(i); closeToc(); });
    list.appendChild(b);
  });
}

function renderChapter(ch) {
  const article = $('#chapter');
  disposeInteractive(article);
  article.innerHTML = '';
  const blocks = state.lang === 'en' && ch.blocksEn ? ch.blocksEn : ch.blocks;
  const title = chapterTitle(ch) || t('empty');
  const n = state.book.chapters.length;
  const idx = state.chapterIndex + 1;
  const words = ch.words || 300;
  const minutes = Math.max(1, Math.round(words / (state.lang === 'zh' ? 380 : 220)));

  const meta = document.createElement('div');
  meta.className = 'chapter-meta';
  const kicker = state.lang === 'zh' ? `第 ${String(idx).padStart(2, '0')} 章` : `Chapter ${String(idx).padStart(2, '0')}`;
  meta.innerHTML = `<span class="chapter-kicker">${kicker}</span><span class="chapter-dot"></span><span class="chapter-time">${minutes} ${t('min')}</span>`;
  article.appendChild(meta);

  const h1 = document.createElement('h1');
  h1.className = 'chapter-title';
  h1.textContent = title;
  article.appendChild(h1);

  const body = document.createElement('div');
  body.className = 'chapter-body';
  const filtered = blocks.filter((b, i) => !(i === 0 && b.type === 'h' && b.level === 1 && textOf(b.html) === title));
  if (!filtered.length) {
    const p = document.createElement('p');
    p.className = 'raw';
    p.textContent = t('empty');
    body.appendChild(p);
  } else {
    let heading = 0;
    const ctx = { chapter: ch, heading: () => ++heading };
    filtered.forEach((b) => body.appendChild(renderBlock(b, ctx)));
  }
  article.appendChild(body);
}

function renderBlock(b, ctx) {
  switch (b.type) {
    case 'p': {
      if (b.blocks?.length) {
        const wrap = document.createElement('div');
        const p = document.createElement('p');
        p.innerHTML = b.html || '';
        if (b.align) p.style.textAlign = b.align;
        if (b.bg) p.style.background = color(b.bg) || '';
        wrap.appendChild(p);
        b.blocks.forEach((x) => wrap.appendChild(renderBlock(x, ctx)));
        return wrap;
      }
      const p = document.createElement('p');
      p.innerHTML = b.html || '';
      if (b.align) p.style.textAlign = b.align;
      if (b.bg) { p.style.background = color(b.bg) || ''; p.style.padding = '.35em .5em'; p.style.borderRadius = '4px'; }
      return p;
    }
    case 'h': {
      const el = document.createElement(`h${Math.min(9, Math.max(1, b.level))}`);
      el.id = `sec-${ctx.chapter.id}-${ctx.heading()}`;
      el.innerHTML = b.html || '';
      return el;
    }
    case 'list': return renderList(b, ctx);
    case 'quote': {
      const q = document.createElement('blockquote');
      b.blocks.forEach((x) => q.appendChild(renderBlock(x, ctx)));
      return q;
    }
    case 'code': return renderCode(b);
    case 'divider': return document.createElement('hr');
    case 'callout': {
      const c = document.createElement('div');
      c.className = 'callout';
      if (b.bg) c.style.background = color(b.bg) || '';
      if (b.border) c.style.borderColor = color(b.border) || '';
      if (b.color) c.style.color = color(b.color) || '';
      const em = document.createElement('span');
      em.className = 'co-emoji';
      em.textContent = b.emoji || '💡';
      const inner = document.createElement('div');
      b.blocks.forEach((x) => inner.appendChild(renderBlock(x, ctx)));
      c.append(em, inner);
      return c;
    }
    case 'grid': {
      const g = document.createElement('div');
      g.className = 'grid-block';
      const ratios = b.columns.map((c) => (Number.isFinite(c.ratio) && c.ratio > 0 ? c.ratio : 1));
      g.style.gridTemplateColumns = ratios.map((r) => `minmax(0, ${r}fr)`).join(' ');
      b.columns.forEach((c) => {
        const col = document.createElement('div');
        col.className = 'grid-col';
        c.blocks.forEach((x) => col.appendChild(renderBlock(x, ctx)));
        g.appendChild(col);
      });
      return g;
    }
    case 'table': return renderTable(b, ctx);
    case 'todo': {
      const label = document.createElement('label');
      label.className = `todo-block${b.done ? ' done' : ''}`;
      const box = document.createElement('input');
      box.type = 'checkbox';
      box.disabled = true;
      box.checked = Boolean(b.done);
      const span = document.createElement('span');
      span.className = 'todo-text';
      span.innerHTML = b.html || '';
      label.append(box, span);
      return label;
    }
    case 'image': return renderImage(b);
    case 'interactive': return mountInteractive(b, state.book, state.lang);
    case 'bookmark': {
      const a = document.createElement('a');
      a.className = 'bookmark';
      const safe = /^https?:/i.test(b.href) ? b.href : '#';
      a.href = safe;
      a.target = '_blank';
      a.rel = 'noopener noreferrer';
      a.innerHTML = `<span class="bm-icon" aria-hidden="true">↗</span><span>${escapeHTML(b.title || b.href)}</span>`;
      return a;
    }
    case 'note': {
      const d = document.createElement('div');
      d.className = 'note';
      d.innerHTML = `<span class="note-icon">${escapeHTML(b.icon || '•')}</span><span>${escapeHTML(b.title || b.body || '')}</span>`;
      return d;
    }
    default: {
      const d = document.createElement('div');
      d.className = 'raw';
      d.textContent = b.text || b.tag || '';
      return d;
    }
  }
}

function renderList(b, ctx) {
  const list = document.createElement(b.ordered ? 'ol' : 'ul');
  for (const item of b.items || []) {
    const li = document.createElement('li');
    if (item.done !== undefined && item.done !== null) {
      li.className = `todo${item.done ? ' done' : ''}`;
      const box = document.createElement('input');
      box.type = 'checkbox';
      box.disabled = true;
      box.checked = Boolean(item.done);
      const span = document.createElement('span');
      span.className = 'todo-text';
      span.innerHTML = item.html || '';
      li.append(box, span);
    } else {
      li.innerHTML = item.html || '';
    }
    if (item.blocks?.length) {
      const wrap = document.createElement('div');
      item.blocks.forEach((x) => wrap.appendChild(renderBlock(x, ctx)));
      li.appendChild(wrap);
    }
    list.appendChild(li);
  }
  return list;
}

function renderCode(b) {
  const pre = document.createElement('pre');
  const head = document.createElement('div');
  head.className = 'code-head';
  const lang = document.createElement('span');
  lang.textContent = b.lang || 'text';
  const copy = document.createElement('button');
  copy.type = 'button';
  copy.className = 'copy-code';
  copy.textContent = t('copy');
  copy.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(b.code || '');
      copy.textContent = t('copied');
      setTimeout(() => { copy.textContent = t('copy'); }, 1400);
    } catch { /* clipboard unavailable */ }
  });
  head.append(lang, copy);
  const code = document.createElement('code');
  code.textContent = b.code || '';
  pre.append(head, code);
  return pre;
}

function renderTable(b, ctx) {
  const wrap = document.createElement('div');
  wrap.className = 'table-wrap';
  const table = document.createElement('table');
  if (b.cols?.some((x) => x > 0)) {
    const cg = document.createElement('colgroup');
    b.cols.forEach((w) => {
      const col = document.createElement('col');
      if (w > 0) col.style.width = `${w}px`;
      cg.appendChild(col);
    });
    table.appendChild(cg);
  }
  const tbody = document.createElement('tbody');
  for (const row of b.rows || []) {
    const tr = document.createElement('tr');
    for (const cell of row) {
      const td = document.createElement(cell.header ? 'th' : 'td');
      if (cell.colspan > 1) td.colSpan = cell.colspan;
      if (cell.rowspan > 1) td.rowSpan = cell.rowspan;
      if (cell.align) td.style.textAlign = cell.align;
      if (cell.bg) td.style.background = color(cell.bg) || '';
      if (cell.html) {
        const div = document.createElement('div');
        div.innerHTML = cell.html;
        td.appendChild(div);
      }
      cell.blocks?.forEach((x) => td.appendChild(renderBlock(x, ctx)));
      tr.appendChild(td);
    }
    tbody.appendChild(tr);
  }
  table.appendChild(tbody);
  wrap.appendChild(table);
  return wrap;
}

function renderImage(b) {
  const fig = document.createElement('figure');
  const src = assetURL(state.book, b.src);
  if (src) {
    const img = document.createElement('img');
    img.src = src;
    img.alt = b.caption || '';
    img.loading = 'lazy';
    if (b.width > 0) img.style.width = `${Math.min(100, b.width)}px`;
    fig.appendChild(img);
  } else {
    const d = document.createElement('div');
    d.className = 'raw';
    d.textContent = t('imageMissing');
    fig.appendChild(d);
  }
  if (b.caption) {
    const cap = document.createElement('figcaption');
    cap.textContent = b.caption;
    fig.appendChild(cap);
  }
  return fig;
}

function renderChapterNav() {
  const chapters = state.book.chapters;
  const prev = $('#prev-chapter');
  const next = $('#next-chapter');
  const nav = $('.chapter-nav');
  const i = state.chapterIndex;
  prev.disabled = i <= 0;
  next.disabled = i >= chapters.length - 1;
  nav.classList.toggle('only-next', i <= 0);
  nav.classList.toggle('only-prev', i >= chapters.length - 1);
  prev.querySelector('span:last-child').innerHTML = i > 0 ? `<span class="cl-label">${t('prev')}</span><span class="cl-title">${escapeHTML(chapterTitle(chapters[i - 1]))}</span>` : '';
  next.querySelector('span:first-child').innerHTML = i < chapters.length - 1 ? `<span class="cl-label">${t('next')}</span><span class="cl-title">${escapeHTML(chapterTitle(chapters[i + 1]))}</span>` : '';
}

function setChapter(index, scroll = true) {
  if (!state.book) return;
  const max = state.book.chapters.length - 1;
  state.chapterIndex = Math.max(0, Math.min(index, max));
  store.set(`mx-progress:${state.book.id || state.book.title}`, String(state.chapterIndex));
  render();
  if (scroll) window.scrollTo({ top: 0, behavior: 'instant' });
  const active = $('.toc-item.active');
  if (active) active.scrollIntoView({ block: 'nearest' });
}

function updateProgress() {
  const max = document.documentElement.scrollHeight - window.innerHeight;
  const p = max > 0 ? Math.min(1, Math.max(0, window.scrollY / max)) : 0;
  $('#progress-bar').style.width = `${p * 100}%`;
}

function textOf(html) {
  const d = document.createElement('div');
  d.innerHTML = html || '';
  return (d.textContent || '').trim();
}

function hasEnglish(book) {
  return (book.languages || []).some((l) => String(l).toLowerCase().startsWith('en')) || book.chapters.some((c) => c.blocksEn);
}

/* ---------- mobile toc ---------- */

function openToc() {
  $('#toc').classList.add('open');
  $('#toc-scrim').hidden = false;
  $('#toc-toggle').setAttribute('aria-expanded', 'true');
}
function closeToc() {
  $('#toc').classList.remove('open');
  $('#toc-scrim').hidden = true;
  $('#toc-toggle').setAttribute('aria-expanded', 'false');
}
function toggleToc() { $('#toc').classList.contains('open') ? closeToc() : openToc(); }

/* ---------- toast & drop ---------- */

let toastTimer = 0;
function toast(message) {
  const el = $('#toast');
  el.textContent = message;
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { el.hidden = true; }, 4200);
}

let dragDepth = 0;
function bindDrop() {
  window.addEventListener('dragenter', (e) => {
    if (![...(e.dataTransfer?.types || [])].includes('Files')) return;
    e.preventDefault(); dragDepth++; $('#drop-overlay').hidden = false;
  });
  window.addEventListener('dragover', (e) => { if ([...(e.dataTransfer?.types || [])].includes('Files')) e.preventDefault(); });
  window.addEventListener('dragleave', (e) => {
    if (![...(e.dataTransfer?.types || [])].includes('Files')) return;
    dragDepth = Math.max(0, dragDepth - 1);
    if (!dragDepth) $('#drop-overlay').hidden = true;
  });
  window.addEventListener('drop', (e) => {
    e.preventDefault(); dragDepth = 0; $('#drop-overlay').hidden = true;
    const file = e.dataTransfer?.files?.[0];
    if (file) openFile(file);
  });
}

/* ---------- wiring ---------- */

function bind() {
  $('#open-book').addEventListener('click', () => $('#file-input').click());
  $('#file-input').addEventListener('change', (e) => {
    const file = e.target.files?.[0];
    e.target.value = '';
    if (file) openFile(file);
  });
  for (const id of ['#landing-theme', '#reader-theme']) {
    $(id)?.addEventListener('click', () => {
      applyTheme(state.theme === 'dark' ? 'light' : 'dark');
      if (state.book) applyBookTheme(state.book);
    });
  }
  $('#lang-toggle').addEventListener('click', () => applyLang(state.lang === 'zh' ? 'en' : 'zh'));
  $('#toc-toggle').addEventListener('click', toggleToc);
  $('#toc-scrim').addEventListener('click', closeToc);
  $('#close-book').addEventListener('click', closeBook);
  $('#reader-home').addEventListener('click', closeBook);
  $('#prev-chapter').addEventListener('click', () => setChapter(state.chapterIndex - 1));
  $('#next-chapter').addEventListener('click', () => setChapter(state.chapterIndex + 1));
  window.addEventListener('scroll', updateProgress, { passive: true });
  window.addEventListener('resize', updateProgress);
  window.addEventListener('keydown', (e) => {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    if (e.key === 'Escape') { if ($('#toc').classList.contains('open')) closeToc(); }
    if (e.altKey && e.key === 'ArrowLeft') { e.preventDefault(); setChapter(state.chapterIndex - 1); }
    if (e.altKey && e.key === 'ArrowRight') { e.preventDefault(); setChapter(state.chapterIndex + 1); }
  });
  $('#chapter').addEventListener('click', (e) => {
    const a = e.target.closest('a');
    if (a && a.getAttribute('href') === '#') e.preventDefault();
  });
}

function init() {
  applyTheme(store.get('mx-theme', matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'));
  applyLang(store.get('mx-lang', 'zh'));
  bind();
  bindDrop();
  loadExamples();
  if (location.hash === '#read') history.replaceState(null, '', location.pathname);
}

init();
