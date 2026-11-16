import katex from './katex.mjs';

// DocxXML → a small, safe block tree for the reader.
//
// This mirrors `crates/core/src/docxml.rs` closely enough for reading and is
// intentionally permissive: unknown elements become inert cards instead of
// being dropped. Nothing from the document is inserted as raw HTML.

const BLOCK_TAGS = new Set([
  'p', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'h7', 'h8', 'h9',
  'ul', 'ol', 'checkbox', 'blockquote', 'pre', 'hr', 'callout', 'grid',
  'column', 'table', 'img', 'figure', 'interactive', 'whiteboard', 'sheet', 'bookmark',
]);

const NAMED_COLORS = {
  red: '#c0392b', orange: '#d97706', yellow: '#b7791f', green: '#2f855a',
  blue: '#2b6cb0', purple: '#6b46c1', gray: '#4a5568', grey: '#4a5568',
  'light-red': '#fde8e8', 'light-orange': '#feebc8', 'light-yellow': '#fefcbf',
  'light-green': '#e6fffa', 'light-blue': '#ebf8ff', 'light-purple': '#faf5ff',
  'light-gray': '#f1f5f9', 'light-grey': '#f1f5f9',
  'dark-red': '#742a2a', 'dark-orange': '#7b341e', 'dark-yellow': '#744210',
  'dark-green': '#22543d', 'dark-blue': '#2a4365', 'dark-purple': '#44337a',
  'dark-gray': '#1a202c', 'dark-grey': '#1a202c',
};

export function parseDocxXml(xml, resolveImage = () => '') {
  const wrapped = `<root>${xml || ''}</root>`;
  const doc = new DOMParser().parseFromString(wrapped, 'application/xml');
  const err = doc.querySelector('parsererror');
  if (err) throw new Error('DocxXML 解析失败 / Invalid DocxXML: ' + err.textContent.trim().slice(0, 160));
  const root = doc.documentElement;
  const titleEl = [...root.children].find((el) => el.tagName.toLowerCase() === 'title');
  const blocks = parseBlocks([...root.children].filter((el) => el.tagName.toLowerCase() !== 'title'), resolveImage);
  return {
    title: titleEl ? textOf(titleEl) : '',
    titleAnchor: titleEl ? inlineAnchorText(titleEl) : '',
    titleHtml: titleEl ? inlineHTML(titleEl, resolveImage) : '',
    icon: titleEl?.getAttribute('icon') || '',
    cover: titleEl?.getAttribute('cover') || '',
    coverY: Number(titleEl?.getAttribute('cover-y') || 0.5),
    blocks,
  };
}

function parseBlocks(elements, resolveImage) {
  return elements.map((el) => parseBlock(el, resolveImage)).filter(Boolean);
}

function parseBlock(el, resolveImage) {
  if (!el || el.nodeType !== 1) return null;
  const tag = el.tagName.toLowerCase();
  if (tag === 'p') return { type: 'p', html: inlineHTML(el, resolveImage), align: el.getAttribute('align'), bg: el.getAttribute('background-color'), blocks: parseBlocks(childBlocks(el), resolveImage) };
  if (/^h[1-9]$/.test(tag)) return { type: 'h', level: Number(tag[1]), html: inlineHTML(el, resolveImage), anchorHtml: escapeHTML(inlineAnchorText(el)), seq: el.getAttribute('seq'), folded: el.getAttribute('folded') === 'true' };
  if (tag === 'ul' || tag === 'ol') return { type: 'list', ordered: tag === 'ol', items: [...el.children].filter((x) => x.tagName.toLowerCase() === 'li').map((li) => parseListItem(li, resolveImage)) };
  if (tag === 'checkbox') return { type: 'todo', done: el.getAttribute('done') === 'true', html: inlineHTML(el, resolveImage), blocks: parseBlocks(childBlocks(el), resolveImage) };
  if (tag === 'blockquote') return { type: 'quote', blocks: parseBlocks([...el.children], resolveImage) };
  if (tag === 'pre') {
    const code = [...el.children].find((x) => x.tagName.toLowerCase() === 'code');
    return { type: 'code', lang: el.getAttribute('lang') || '', caption: el.getAttribute('caption') || '', wrap: el.getAttribute('wrap') === 'true', code: code ? code.textContent : el.textContent };
  }
  if (tag === 'hr') return { type: 'divider' };
  if (tag === 'callout') return { type: 'callout', emoji: el.getAttribute('emoji') || '💡', bg: el.getAttribute('background-color'), border: el.getAttribute('border-color'), color: el.getAttribute('text-color'), blocks: parseBlocks([...el.children], resolveImage) };
  if (tag === 'grid') return { type: 'grid', columns: [...el.children].filter((x) => x.tagName.toLowerCase() === 'column').map((c) => ({ ratio: Number(c.getAttribute('width-ratio') || 1), blocks: parseBlocks([...c.children], resolveImage) })) };
  if (tag === 'table') return parseTable(el, resolveImage);
  if (tag === 'img') return { type: 'image', src: el.getAttribute('href') || el.getAttribute('src') || el.getAttribute('path') || '', width: Number(el.getAttribute('width') || 0), caption: el.getAttribute('caption') || '', align: el.getAttribute('align') };
  if (tag === 'figure' || tag === 'source') {
    const file = tag === 'source' ? el : [...el.children].find((child) => child.tagName.toLowerCase() === 'source');
    return file ? { type: 'p', html: attachmentHTML(file, resolveImage), blocks: [] } : { type: 'raw', tag, text: el.textContent.trim().slice(0, 800) };
  }
  if (tag === 'interactive') return { type: 'interactive', kind: (el.getAttribute('type') || 'canvas').toLowerCase(), title: el.getAttribute('title') || '', height: clamp(Number(el.getAttribute('height') || 360), 120, 1600), source: el.textContent.trim() };
  if (tag === 'whiteboard') return { type: 'note', icon: '✏️', title: '白板', body: 'Whiteboard' };
  if (tag === 'sheet') return { type: 'note', icon: '▦', title: '表格', body: 'Sheet' };
  if (tag === 'bookmark') return { type: 'bookmark', href: el.getAttribute('href') || '', title: el.getAttribute('title') || '' };
  return { type: 'raw', tag, text: el.textContent.trim().slice(0, 800) };
}

function parseListItem(li, resolveImage) {
  const { html, blocks } = splitInlineAndBlocks(li, resolveImage);
  return { html, blocks, folded: li.getAttribute('folded') === 'true', seq: li.getAttribute('seq') };
}

function parseTable(el, resolveImage) {
  const rows = [];
  for (const tr of el.querySelectorAll('tr')) {
    const cells = [...tr.children].filter((c) => ['td', 'th'].includes(c.tagName.toLowerCase())).map((td) => {
      const blocks = parseBlocks(childBlocks(td), resolveImage);
      return {
        header: td.tagName.toLowerCase() === 'th',
        html: blocks.length ? '' : inlineHTML(td, resolveImage),
        blocks,
        colspan: Number(td.getAttribute('colspan') || 1),
        rowspan: Number(td.getAttribute('rowspan') || 1),
        align: td.getAttribute('align'),
        bg: td.getAttribute('background-color'),
      };
    });
    rows.push(cells);
  }
  const cols = [...el.querySelectorAll('colgroup > col')].map((c) => Number(c.getAttribute('width') || 0));
  return { type: 'table', rows, cols };
}

function childBlocks(el) {
  return [...el.children].filter((c) => BLOCK_TAGS.has(c.tagName.toLowerCase()));
}

function splitInlineAndBlocks(el, resolveImage) {
  const blocks = [];
  let html = '';
  for (const node of el.childNodes) {
    if (node.nodeType === 1 && BLOCK_TAGS.has(node.tagName.toLowerCase())) {
      const b = parseBlock(node, resolveImage);
      if (b) blocks.push(b);
    } else {
      html += inlineNode(node, resolveImage);
    }
  }
  return { html, blocks };
}

function inlineHTML(el, resolveImage) {
  let out = '';
  for (const node of el.childNodes) out += inlineNode(node, resolveImage);
  return out;
}

// Match the desktop's logical heading text: preserve image alt text and TeX
// source, drop other inline objects, and ignore nested block children.
function inlineAnchorText(el) {
  const codeText = (node) => node.nodeType === 3 ? node.nodeValue : node.nodeType !== 1 ? ''
    : node.tagName.toLowerCase() === 'br' ? '\n' : [...node.childNodes].map(codeText).join('');
  const walk = (node) => {
    if (node.nodeType === 3) {
      let text = node.nodeValue || '';
      const tail = text.match(/[ \t\n\r\f\v]*$/)[0];
      const first = tail.indexOf('\n');
      if (first >= 0) text = text.slice(0, text.length - tail.length + first);
      const head = text.match(/^[ \t\n\r\f\v]*/)[0];
      const last = head.lastIndexOf('\n');
      return text.slice(last + 1).replace(/[\n\r]/g, '');
    }
    if (node.nodeType !== 1) return '';
    const tag = node.tagName.toLowerCase();
    if (tag === 'br') return '\n';
    if (tag === 'latex') return codeText(node);
    if (tag === 'inline-image') return node.getAttribute('alt') || '';
    if (!['b', 'strong', 'em', 'i', 'u', 'del', 's', 'code', 'a', 'span'].includes(tag)
        || tag === 'a' && node.getAttribute('type') === 'url-preview' && !node.hasAttribute('href')) return '';
    return [...node.childNodes].map(walk).join('');
  };
  return [...el.childNodes].map(walk).join('');
}

// KaTeX produces native MathML without external stylesheets or font downloads.
// Keep invalid or unusually large expressions visible as escaped source.
export function mathHTML(src, displayMode = false) {
  const source = String(src ?? '');
  if (source.length > 16384) return `<code class="math-fallback">${escapeHTML(source)}</code>`;
  try {
    return katex.renderToString(source, { output: 'mathml', displayMode, throwOnError: true, trust: false, strict: 'ignore', maxExpand: 1000, maxSize: 100 });
  } catch {
    return `<code class="math-fallback">${escapeHTML(source)}</code>`;
  }
}

function inlineNode(node, resolveImage) {
  if (node.nodeType === 3) return escapeHTML(node.nodeValue);
  if (node.nodeType !== 1) return '';
  const tag = node.tagName.toLowerCase();
  switch (tag) {
    case 'b': case 'strong': return `<strong>${inlineHTML(node, resolveImage)}</strong>`;
    case 'i': case 'em': return `<em>${inlineHTML(node, resolveImage)}</em>`;
    case 'u': return `<u>${inlineHTML(node, resolveImage)}</u>`;
    case 's': case 'del': return `<del>${inlineHTML(node, resolveImage)}</del>`;
    case 'code': return `<code>${escapeHTML(node.textContent)}</code>`;
    case 'br': return '<br>';
    case 'latex': {
      const display = node.getAttribute('display') === 'block';
      return `<span class="${display ? 'math-display' : 'math-inline'}">${mathHTML(node.textContent, display)}</span>`;
    }
    case 'source': return attachmentHTML(node, resolveImage);
    case 'inline-image': {
      const source = node.getAttribute('src') || node.getAttribute('path') || node.getAttribute('href') || '';
      const alt = node.getAttribute('alt') || '';
      const src = resolveImage(source) || source;
      return /^(https?:|data:image\/)/i.test(src) ? `<img class="inline-image" src="${escapeAttr(src)}" alt="${escapeAttr(alt)}" loading="lazy">` : escapeHTML(alt);
    }
    case 'cite': {
      const who = node.getAttribute('type') === 'user' ? '成员' : '引用';
      return `<span class="cite" title="${escapeAttr(who)}">@${escapeHTML(who)}</span>`;
    }
    case 'a': {
      const href = node.getAttribute('href') || '';
      const internal = resolveImage.link?.(href) || '';
      const safe = internal || (/^(https?:|mailto:|ftp:)/i.test(href) ? href : '#');
      return `<a href="${escapeAttr(safe)}"${internal ? '' : ' target="_blank" rel="noopener noreferrer"'}>${inlineHTML(node, resolveImage)}</a>`;
    }
    case 'span': {
      const fg = color(node.getAttribute('text-color'));
      const bg = color(node.getAttribute('background-color'));
      const style = [fg && `color:${fg}`, bg && `background:${bg}`].filter(Boolean).join(';');
      return `<span${style ? ` style="${style}"` : ''}>${inlineHTML(node, resolveImage)}</span>`;
    }
    default: return inlineHTML(node, resolveImage);
  }
}

function attachmentHTML(node, resolveAsset) {
  const path = node.getAttribute('path') || '';
  const name = node.getAttribute('name') || path.split(/[\\/]/).pop() || 'Attachment';
  const resolved = path ? resolveAsset(path) : '';
  // Force embedded attachments to download as bytes, including HTML or SVG
  // files, rather than navigating to executable document content.
  const data = /^data:[^,]*;base64,([a-z0-9+/]*={0,2})$/i.exec(resolved);
  if (data) return `<a class="attachment" href="data:application/octet-stream;base64,${data[1]}" download="${escapeAttr(name)}">${escapeHTML(name)}</a>`;
  return `<span class="attachment">${escapeHTML(name)}</span>`;
}

export function color(name) {
  if (!name) return '';
  if (NAMED_COLORS[name]) return NAMED_COLORS[name];
  if (/^#[0-9a-f]{3,8}$/i.test(name)) return name;
  return '';
}

function textOf(el) {
  return (el.textContent || '').replace(/\s+/g, ' ').trim();
}
export function escapeHTML(s) {
  return String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}
export function escapeAttr(s) { return escapeHTML(s); }
function clamp(n, a, b) { return Number.isFinite(n) ? Math.max(a, Math.min(b, n)) : a; }
