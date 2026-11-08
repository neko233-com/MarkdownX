// A deliberately small Markdown reader. It covers what books use in practice:
// headings, paragraphs, lists, quotes, fenced code, tables, images, rules and
// the `mdxb-canvas` / `mdxb-html` fences that carry interactive blocks.

import { escapeHTML, mathHTML } from './docxml.js';

export function parseMarkdown(src, title = '') {
  const lines = String(src ?? '').replace(/\r\n?/g, '\n').split('\n');
  const blocks = [];
  let i = 0;
  const isBlank = (s) => !s.trim();

  while (i < lines.length) {
    const line = lines[i];
    if (isBlank(line)) { i++; continue; }

    const fence = line.match(/^(`{3,}|~{3,})\s*([^\s`]*)\s*$/);
    if (fence) {
      const marker = fence[1][0];
      const lang = (fence[2] || '').toLowerCase();
      const body = [];
      i++;
      while (i < lines.length && !new RegExp(`^${marker}{${fence[1].length},}\\s*$`).test(lines[i])) body.push(lines[i++]);
      i++;
      const text = body.join('\n');
      if (lang === 'mdxb-canvas' || lang === 'mdxb-html') {
        try {
          const obj = JSON.parse(text);
          blocks.push({ type: 'interactive', kind: lang.slice(5), title: obj.title || '', height: clamp(Number(obj.height || 360), 120, 1600), source: String(obj.source ?? '') });
        } catch {
          blocks.push({ type: 'code', lang: 'json', code: text, caption: '' });
        }
      } else {
        blocks.push({ type: 'code', lang: fence[2] || '', code: text, caption: '' });
      }
      continue;
    }

    const heading = line.match(/^(#{1,6})\s+(.*)$/);
    if (heading) {
      blocks.push({ type: 'h', level: heading[1].length, html: inlineMd(heading[2]), seq: null });
      i++;
      continue;
    }

    if (/^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/.test(line)) { blocks.push({ type: 'divider' }); i++; continue; }

    if (/^\s*>/.test(line)) {
      const body = [];
      while (i < lines.length && (/^\s*>/.test(lines[i]) || isBlank(lines[i]))) body.push(lines[i++].replace(/^\s*>\s?/, ''));
      blocks.push({ type: 'quote', blocks: parseMarkdown(body.join('\n')).blocks });
      continue;
    }

    if (/^\s*([-*+]|\d+\.)\s+/.test(line)) {
      const ordered = /^\s*\d+\./.test(line);
      const items = [];
      while (i < lines.length && /^\s*([-*+]|\d+\.)\s+/.test(lines[i])) {
        let text = lines[i].replace(/^\s*(?:[-*+]|\d+\.)\s+/, '');
        let done = null;
        const task = text.match(/^\[([ xX])\]\s+(.*)$/);
        if (task) { done = task[1].toLowerCase() === 'x'; text = task[2]; }
        items.push({ html: inlineMd(text), done, blocks: [] });
        i++;
      }
      blocks.push({ type: 'list', ordered, items });
      continue;
    }

    if (/^\s*\|.*\|\s*$/.test(line) && i + 1 < lines.length && /^\s*\|?[\s:|-]+\|[\s:|-]*$/.test(lines[i + 1])) {
      const head = splitRow(line);
      i += 2;
      const rows = [];
      while (i < lines.length && /^\s*\|.*\|\s*$/.test(lines[i])) rows.push(splitRow(lines[i++]));
      blocks.push({
        type: 'table',
        cols: [],
        rows: [head.map((h) => ({ header: true, html: inlineMd(h), blocks: [], colspan: 1, rowspan: 1 })), ...rows.map((r) => r.map((c) => ({ header: false, html: inlineMd(c), blocks: [], colspan: 1, rowspan: 1 })))],
      });
      continue;
    }

    const img = line.match(/^\s*!\[([^\]]*)\]\(([^)]+)\)\s*$/);
    if (img) { blocks.push({ type: 'image', src: img[2], caption: img[1], width: 0, align: null }); i++; continue; }

    const para = [];
    while (i < lines.length && !isBlank(lines[i]) && !/^(#{1,6})\s/.test(lines[i]) && !/^(`{3,}|~{3,})/.test(lines[i]) && !/^\s*([-*+]|\d+\.)\s+/.test(lines[i]) && !/^\s*>/.test(lines[i]) && !/^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/.test(lines[i])) para.push(lines[i++]);
    if (para.length) blocks.push({ type: 'p', html: inlineMd(para.join('\n')).replace(/\n/g, '<br>') });
  }
  return { title, blocks };
}

function splitRow(line) {
  return line.trim().replace(/^\|/, '').replace(/\|$/, '').split('|').map((s) => s.trim());
}

export function inlineMd(s) {
  const stash = [];
  const keep = (html) => `\u0000${stash.push(html) - 1}\u0000`;
  let out = escapeHTML(s);
  out = out.replace(/`([^`]+)`/g, (_, code) => keep(`<code>${code}</code>`));
  out = out.replace(/!\[([^\]]*)\]\(([^)]+)\)/g, (_, alt, src) => keep(`<img src="${safeSrc(src)}" alt="${alt}" loading="lazy">`));
  out = out.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_, label, href) => keep(`<a href="${safeHref(href)}" target="_blank" rel="noopener noreferrer">${label}</a>`));
  out = out.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>').replace(/__([^_]+)__/g, '<strong>$1</strong>');
  out = out.replace(/(^|[\s(])\*([^*\n]+)\*/g, '$1<em>$2</em>').replace(/(^|[\s(])_([^_\n]+)_/g, '$1<em>$2</em>');
  out = out.replace(/~~([^~]+)~~/g, '<del>$1</del>');
  out = out.replace(/\$([^$\n]+)\$/g, (_, expr) => keep(`<span class="math-inline">${mathHTML(expr, true)}</span>`));
  return out.replace(/\u0000(\d+)\u0000/g, (_, n) => stash[Number(n)]);
}

function safeHref(href) {
  const s = String(href).trim();
  return /^(https?:|mailto:|#)/i.test(s) ? escapeHTML(s) : '#';
}
function safeSrc(src) {
  const s = String(src).trim();
  return /^(https?:|data:image\/)/i.test(s) ? escapeHTML(s) : '';
}
function clamp(n, a, b) { return Number.isFinite(n) ? Math.max(a, Math.min(b, n)) : a; }
