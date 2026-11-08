// `.mdxb` loading, normalization and asset resolution.
//
// The browser intentionally accepts only the documented v1 shape. Any extra
// fields are ignored, and malformed books fail with a readable message.

import { parseDocxXml } from './docxml.js';
import { parseMarkdown } from './markdown.js';

export const FORMAT = 'markdownx.book';
export const VERSION = 1;

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
  if (Number(raw.version) !== VERSION) throw new Error(`不支持的版本 ${raw.version} / Unsupported book version ${raw.version}`);
  if (!Array.isArray(raw.chapters) || !raw.chapters.length) throw new Error('书里没有章节 / The book has no chapters');
  const seen = new Set();
  const chapters = raw.chapters.map((ch, index) => {
    const id = String(ch.id || `chapter-${index + 1}`);
    if (seen.has(id)) throw new Error('章节 id 重复 / Duplicate chapter id: ' + id);
    seen.add(id);
    const format = String(ch.format || 'docxxml').toLowerCase();
    const content = String(ch.content || '');
    const contentEn = String(ch.content_en || '');
    let parsed;
    let parsedEn;
    if (format === 'docxxml' || format === 'docxml') {
      parsed = parseDocxXml(content);
      parsedEn = contentEn ? parseDocxXml(contentEn) : null;
    } else if (format === 'markdown' || format === 'md') {
      parsed = parseMarkdown(content);
      parsedEn = contentEn ? parseMarkdown(contentEn) : null;
    } else throw new Error('未知章节格式 / Unknown chapter format: ' + format);
    return {
      id,
      titleRaw: ch.title,
      title: loc(ch.title) || parsed.title || `Chapter ${index + 1}`,
      format,
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
  const key = src.replace(/^\.\//, '');
  const asset = book.assets[key] || book.assets[src];
  if (asset && asset.data) {
    const mime = asset.mime || guessMime(key);
    return `data:${mime};base64,${asset.data}`;
  }
  if (/^data:image\//i.test(src)) return src;
  return '';
}

function guessMime(key) {
  const ext = key.split('.').pop().toLowerCase();
  return { png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', webp: 'image/webp', svg: 'image/svg+xml', pdf: 'application/pdf' }[ext] || 'application/octet-stream';
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
    }
  };
  walk(blocks);
  return Math.max(50, Math.round(n / 2));
}

function strip(html) {
  if (!html) return '';
  return String(html).replace(/<[^>]*>/g, '').replace(/&[a-z#0-9]+;/gi, ' ');
}
