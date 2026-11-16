// `.mdxb` loading, normalization and asset resolution.
//
// The browser accepts the documented v1 and v2 shapes. Any extra
// fields are ignored, and malformed books fail with a readable message.

import { parseDocxXml } from './docxml.js';
import { parseMarkdown } from './markdown.js';

export const FORMAT = 'markdownx.book';
export const VERSION = 2;

// Match Rust's per-character Unicode lowercase and whitespace rules.
export function headingSlug(text) {
  return Array.from(String(text).replace(/^\p{White_Space}+|\p{White_Space}+$/gu, ''), ch => ch.toLowerCase()).join('')
    .replace(/[^\p{Alphabetic}\p{N}_\-\p{White_Space}]/gu, '').replace(/\p{White_Space}/gu, '-');
}

export async function readBookFile(file) {
  const text = await file.text();
  let raw;
  try {
    raw = JSON.parse(text);
  } catch (e) {
    throw new Error('不是有效的 .mdxb 文件（JSON 解析失败）/ Not a valid .mdxb file: ' + e.message);
  }
  return normalizeBook(raw);
}

export function normalizeBook(raw) {
  if (!raw || raw.format !== FORMAT) throw new Error('缺少 format: "markdownx.book" / Missing book format marker');
  if (![1, 2].includes(Number(raw.version))) throw new Error(`不支持的版本 ${raw.version} / Unsupported book version ${raw.version}`);
  if (!Array.isArray(raw.chapters) || !raw.chapters.length) throw new Error('书里没有章节 / The book has no chapters');
  const sourcePaths = new Map();
  const pathOK = (path) => typeof path === 'string' && path.length > 0 && path.length <= 8192 && !path.startsWith('/')
    && !/[\\:\x00-\x1f\x7f-\x9f]/.test(path) && path.split('/').every(part => part && part !== '.' && part !== '..');
  raw.chapters.forEach((chapter, index) => {
    for (const path of [chapter.source_path, chapter.source_en_path]) {
      if (!pathOK(path)) continue;
      if (sourcePaths.has(path) && sourcePaths.get(path) !== index) sourcePaths.set(path, -1);
      else sourcePaths.set(path, index);
    }
  });
  const resolveChapter = (base, href, currentId) => {
    if (typeof href !== 'string' || !href || /^(?:[a-z][a-z0-9+.-]*:|\/)/i.test(href) || /[?\x00-\x1f\x7f-\x9f]/.test(href)) return '';
    const hash = href.indexOf('#');
    if (hash >= 0 && (hash === href.length - 1 || href.indexOf('#', hash + 1) >= 0)) return '';
    const pathPart = hash < 0 ? href : href.slice(0, hash);
    let anchor = '';
    try { if (hash >= 0) anchor = '?anchor=' + encodeURIComponent(decodeURIComponent(href.slice(hash + 1))); } catch { return ''; }
    if (!pathPart) return '#mdxb-chapter/' + encodeURIComponent(currentId) + anchor;
    if (!pathOK(base) || /[\\]/.test(pathPart)) return '';
    let decoded;
    try { decoded = decodeURIComponent(pathPart); } catch { return ''; }
    if (!decoded || /^[\/\\]/.test(decoded) || /[\\:\x00-\x1f\x7f-\x9f]/.test(decoded)) return '';
    const parts = base.split('/').slice(0, -1);
    for (const part of decoded.split('/')) {
      if (!part || part === '.') continue;
      if (part === '..') { if (!parts.length) return ''; parts.pop(); }
      else parts.push(part);
    }
    const path = parts.join('/'), index = sourcePaths.get(path);
    if (!pathOK(path) || !Number.isInteger(index) || index < 0) return '';
    const target = raw.chapters[index];
    return '#mdxb-chapter/' + encodeURIComponent(String(target.id || `chapter-${index + 1}`)) + anchor;
  };
  const seen = new Set();
  const chapters = raw.chapters.map((ch, index) => {
    const id = String(ch.id || `chapter-${index + 1}`);
    if (seen.has(id)) throw new Error('章节 id 重复 / Duplicate chapter id: ' + id);
    seen.add(id);
    const format = String(ch.format || 'docxxml').toLowerCase();
    const content = String(ch.content || '');
    const contentEn = String(ch.content_en || '');
    const formatEn = String(ch.format_en || format).toLowerCase();
    if (ch.format_en && Number(raw.version) < 2) throw new Error('翻译格式需要书籍版本 2 / Translation format requires book version 2');
    const parse = (source, kind, base) => {
      const resolve = (src) => assetURL(raw, src);
      resolve.link = (href) => resolveChapter(base, href, id) || pdfAssetLink(raw, href);
      if (kind === 'docxxml' || kind === 'docxml') return parseDocxXml(source, resolve);
      if (kind === 'markdown' || kind === 'md') return parseMarkdown(source, '', resolve);
      throw new Error('未知章节格式 / Unknown chapter format: ' + kind);
    };
    const parsed = parse(content, format, ch.source_path);
    const parsedEn = contentEn ? parse(contentEn, formatEn, ch.source_en_path || ch.source_path) : null;
    return {
      id,
      titleRaw: ch.title,
      title: loc(ch.title) || parsed.title || `Chapter ${index + 1}`,
      titleAnchor: parsed.titleAnchor || '',
      titleAnchorEn: parsedEn?.titleAnchor || '',
      titleHtml: parsed.titleHtml || '',
      titleHtmlEn: parsedEn?.titleHtml || '',
      format,
      formatEn,
      blocks: parsed.blocks,
      blocksEn: parsedEn ? parsedEn.blocks : null,
      headings: collectHeadings(parsed.blocks),
      words: countWords(parsed.blocks),
    };
  });
  const languages = Array.isArray(raw.languages) && raw.languages.length ? raw.languages.map(String) : ['zh', 'en'];
  const lang = String(raw.default_language || languages[0] || 'zh');
  return {
    ...raw,
    titleRaw: raw.title,
    subtitleRaw: raw.subtitle,
    title: loc(raw.title) || chapters[0].title,
    subtitle: loc(raw.subtitle),
    authors: Array.isArray(raw.authors) ? raw.authors.map(String) : [],
    languages,
    lang,
    chapters,
    cover: raw.cover || '',
    theme: raw.theme || {},
    reading: { layout: 'scroll', max_width: 760, show_toc: true, ...(raw.reading || {}) },
    assets: raw.assets && typeof raw.assets === 'object' ? raw.assets : {},
  };
}

export function pick(value, lang) {
  if (value == null) return '';
  if (typeof value === 'string') return value;
  const zh = String(value.zh || '').trim();
  const en = String(value.en || '').trim();
  if (String(lang).toLowerCase().startsWith('zh')) return zh || en;
  return en || zh;
}
export function loc(value) {
  if (!value) return '';
  if (typeof value === 'string') return value;
  return String(value.zh || value.en || '').trim();
}

export function assetURL(book, src) {
  if (!src) return '';
  if (/^https?:/i.test(src)) return src;
  if (/^data:image\//i.test(src)) return src;
  const assets = book.assets && typeof book.assets === 'object' ? book.assets : {};
  const lookup = (value) => {
    let key = value.replace(/^(?:\.\/)+/, '');
    try { key = decodeURIComponent(key); } catch { return ''; }
    if (key.includes('\\') || key.includes(':') || key.split('/').some((part) => part === '..')) return '';
    const asset = Object.prototype.hasOwnProperty.call(assets, key) ? assets[key] : null;
    return asset && asset.data ? `data:${asset.mime || guessMime(key)};base64,${asset.data}` : '';
  };
  const exact = lookup(src);
  if (exact) return exact;
  const hash = src.indexOf('#');
  if (hash >= 0) {
    const path = src.slice(0, hash).split('?')[0];
    const value = lookup(path);
    return /^data:image\/svg\+xml;base64,/i.test(value) ? value + src.slice(hash) : '';
  }
  const query = src.indexOf('?');
  return query < 0 ? '' : lookup(src.slice(0, query));
}

/// Internal reader link for an embedded PDF, with an optional one-based page.
export function pdfAssetLink(book, href) {
  if (typeof href !== 'string' || !href || href.includes('?')) return '';
  const hash = href.indexOf('#');
  const rawPath = (hash < 0 ? href : href.slice(0, hash)).replace(/^(?:\.\/)+/, '');
  let key;
  try { key = decodeURIComponent(rawPath); } catch { return ''; }
  if (!key.startsWith('assets/') || /[\\:\x00-\x1f\x7f-\x9f]/.test(key) || key.split('/').some(part => !part || part === '..')) return '';
  const asset = book.assets && Object.prototype.hasOwnProperty.call(book.assets, key) ? book.assets[key] : null;
  if (!asset || String(asset.mime).toLowerCase() !== 'application/pdf' || !asset.data) return '';
  let page = '';
  if (hash >= 0) {
    let fragment;
    try { fragment = decodeURIComponent(href.slice(hash + 1)); } catch { return ''; }
    const match = /^page=([0-9]+)$/.exec(fragment);
    if (!match || !Number.isSafeInteger(Number(match[1])) || Number(match[1]) < 1 || Number(match[1]) > 2147483647) return '';
    page = '?page=' + Number(match[1]);
  }
  return '#mdxb-pdf/' + encodeURIComponent(key) + page;
}

export function mediaAssetURL(book, src) {
  const match = /^(.*?)(?:\?[^#]*)?(#t=(?:npt:)?(?=[0-9:.,]*[0-9])[0-9:.]+(?:,[0-9:.]*)?|#t=(?:npt:)?,[0-9:.]+)$/.exec(src || '');
  if (!match) return assetURL(book, src);
  const value = assetURL(book, match[1]);
  return /^data:(?:audio|video)\//i.test(value) ? value + match[2] : '';
}

export function scriptAssetURL(book, src, module) {
  const query = src.indexOf('?');
  const hash = src.indexOf('#');
  let path = src, suffix = '';
  const identity = raw => {
    const base = 'https://markdownx.invalid/module.js';
    const normalized = new URL(base + raw).href.slice(base.length);
    return normalized ? '#' + [...new TextEncoder().encode(normalized)].map(byte => '%' + byte.toString(16).toUpperCase().padStart(2, '0')).join('') : '';
  };
  if (query >= 0 && (hash < 0 || query < hash)) {
    path = src.slice(0, query);
    if (module) suffix = identity(src.slice(query));
  } else if (hash >= 0) {
    const encoded = src.slice(hash + 1);
    if (!/^%3F(?:%[0-9a-f]{2})*$/i.test(encoded)) return '';
    path = src.slice(0, hash);
    if (module) {
      try { suffix = identity(decodeURIComponent(encoded)); } catch { suffix = '#' + encoded; }
    }
  }
  const value = assetURL(book, path);
  return /^data:(?:text|application)\/(?:java|ecma)script;base64,/i.test(value) ? value + suffix : '';
}

function guessMime(key) {
  const ext = key.split('.').pop().toLowerCase();
  return { png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', webp: 'image/webp', svg: 'image/svg+xml', pdf: 'application/pdf', mp4: 'video/mp4', m4v: 'video/mp4', mov: 'video/quicktime', webm: 'video/webm', ogv: 'video/ogg', mp3: 'audio/mpeg', wav: 'audio/wav', ogg: 'audio/ogg', opus: 'audio/ogg', m4a: 'audio/mp4', aac: 'audio/aac', flac: 'audio/flac', vtt: 'text/vtt', js: 'text/javascript', mjs: 'text/javascript', css: 'text/css', obj: 'model/obj', stl: 'model/stl', gltf: 'model/gltf+json', glb: 'model/gltf-binary' }[ext] || 'application/octet-stream';
}

function collectHeadings(blocks, out = []) {
  for (const b of blocks) {
    if (b.type === 'h') out.push({ level: b.level, html: b.html });
    if (b.blocks) collectHeadings(b.blocks, out);
    if (b.type === 'quote' || b.type === 'callout') collectHeadings(b.blocks, out);
    if (b.type === 'grid') b.columns.forEach((c) => collectHeadings(c.blocks, out));
    if (b.type === 'list') b.items.forEach((it) => collectHeadings(it.blocks, out));
  }
  return out;
}

function countWords(blocks) {
  let n = 0;
  const walk = (list) => {
    for (const b of list) {
      n += strip(b.html).length;
      if (b.code) n += b.code.length;
      if (b.blocks) walk(b.blocks);
      if (b.items) b.items.forEach((it) => { n += strip(it.html).length; walk(it.blocks); });
      if (b.columns) b.columns.forEach((c) => walk(c.blocks));
      if (b.rows) b.rows.forEach((r) => r.forEach((c) => { n += strip(c.html).length; walk(c.blocks); }));
      if (b.notes) b.notes.forEach((note) => walk(note.blocks));
    }
  };
  walk(blocks);
  return Math.max(50, Math.round(n / 2));
}

function strip(html) {
  if (!html) return '';
  return String(html).replace(/<[^>]*>/g, '').replace(/&[a-z#0-9]+;/gi, ' ');
}
