// DocxXML → a small, safe block tree for the reader.
//
// This mirrors `crates/core/src/docxml.rs` closely enough for reading and is
// intentionally permissive: unknown elements become inert cards instead of
// being dropped. Nothing from the document is inserted as raw HTML.

const BLOCK_TAGS = new Set([
  'p', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'h7', 'h8', 'h9',
  'ul', 'ol', 'checkbox', 'blockquote', 'pre', 'hr', 'callout', 'grid',
  'column', 'table', 'img', 'interactive', 'whiteboard', 'sheet', 'bookmark',
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

export function parseDocxXml(xml) {
  const wrapped = `<root>${xml || ''}</root>`;
  const doc = new DOMParser().parseFromString(wrapped, 'application/xml');
  const err = doc.querySelector('parsererror');
  if (err) throw new Error('DocxXML 解析失败 / Invalid DocxXML: ' + err.textContent.trim().slice(0, 160));
  const root = doc.documentElement;
  const titleEl = [...root.children].find((el) => el.tagName.toLowerCase() === 'title');
  const blocks = parseBlocks([...root.children].filter((el) => el.tagName.toLowerCase() !== 'title'));
  return {
    title: titleEl ? textOf(titleEl) : '',
    icon: titleEl?.getAttribute('icon') || '',
    cover: titleEl?.getAttribute('cover') || '',
    coverY: Number(titleEl?.getAttribute('cover-y') || 0.5),
    blocks,
  };
}

function parseBlocks(elements) {
  return elements.map(parseBlock).filter(Boolean);
}

function parseBlock(el) {
  if (!el || el.nodeType !== 1) return null;
  const tag = el.tagName.toLowerCase();
  if (tag === 'p') return { type: 'p', html: inlineHTML(el), align: el.getAttribute('align'), bg: el.getAttribute('background-color'), blocks: parseBlocks(childBlocks(el)) };
  if (/^h[1-9]$/.test(tag)) return { type: 'h', level: Number(tag[1]), html: inlineHTML(el), seq: el.getAttribute('seq'), folded: el.getAttribute('folded') === 'true' };
  if (tag === 'ul' || tag === 'ol') return { type: 'list', ordered: tag === 'ol', items: [...el.children].filter((x) => x.tagName.toLowerCase() === 'li').map(parseListItem) };
  if (tag === 'checkbox') return { type: 'todo', done: el.getAttribute('done') === 'true', html: inlineHTML(el), blocks: parseBlocks(childBlocks(el)) };
  if (tag === 'blockquote') return { type: 'quote', blocks: parseBlocks([...el.children]) };
  if (tag === 'pre') {
    const code = [...el.children].find((x) => x.tagName.toLowerCase() === 'code');
    return { type: 'code', lang: el.getAttribute('lang') || '', caption: el.getAttribute('caption') || '', wrap: el.getAttribute('wrap') === 'true', code: code ? code.textContent : el.textContent };
  }
  if (tag === 'hr') return { type: 'divider' };
  if (tag === 'callout') return { type: 'callout', emoji: el.getAttribute('emoji') || '💡', bg: el.getAttribute('background-color'), border: el.getAttribute('border-color'), color: el.getAttribute('text-color'), blocks: parseBlocks([...el.children]) };
  if (tag === 'grid') return { type: 'grid', columns: [...el.children].filter((x) => x.tagName.toLowerCase() === 'column').map((c) => ({ ratio: Number(c.getAttribute('width-ratio') || 1), blocks: parseBlocks([...c.children]) })) };
  if (tag === 'table') return parseTable(el);
  if (tag === 'img') return { type: 'image', src: el.getAttribute('href') || el.getAttribute('src') || '', width: Number(el.getAttribute('width') || 0), caption: el.getAttribute('caption') || '', align: el.getAttribute('align') };
  if (tag === 'interactive') return { type: 'interactive', kind: (el.getAttribute('type') || 'canvas').toLowerCase(), title: el.getAttribute('title') || '', height: clamp(Number(el.getAttribute('height') || 360), 120, 1600), source: el.textContent.trim() };
  if (tag === 'whiteboard') return { type: 'note', icon: '✏️', title: '白板', body: 'Whiteboard' };
  if (tag === 'sheet') return { type: 'note', icon: '▦', title: '表格', body: 'Sheet' };
  if (tag === 'bookmark') return { type: 'bookmark', href: el.getAttribute('href') || '', title: el.getAttribute('title') || '' };
  return { type: 'raw', tag, text: el.textContent.trim().slice(0, 800) };
}

function parseListItem(li) {
  const { html, blocks } = splitInlineAndBlocks(li);
  return { html, blocks, folded: li.getAttribute('folded') === 'true', seq: li.getAttribute('seq') };
}

function parseTable(el) {
  const rows = [];
  for (const tr of el.querySelectorAll('tr')) {
    const cells = [...tr.children].filter((c) => ['td', 'th'].includes(c.tagName.toLowerCase())).map((td) => {
      const blocks = parseBlocks(childBlocks(td));
      return {
        header: td.tagName.toLowerCase() === 'th',
        html: blocks.length ? '' : inlineHTML(td),
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

function splitInlineAndBlocks(el) {
  const blocks = [];
  let html = '';
  for (const node of el.childNodes) {
    if (node.nodeType === 1 && BLOCK_TAGS.has(node.tagName.toLowerCase())) {
      const b = parseBlock(node);
      if (b) blocks.push(b);
    } else {
      html += inlineNode(node);
    }
  }
  return { html, blocks };
}

function inlineHTML(el) {
  let out = '';
  for (const node of el.childNodes) out += inlineNode(node);
  return out;
}

// The desktop editor renders full TeX. The reader keeps the same source but
// folds the common commands into Unicode + a little HTML, so a book still reads
// well without shipping a math engine.
export function mathHTML(src, alreadyEscaped = false) {
  let s = alreadyEscaped ? String(src ?? '') : escapeHTML(src);
  s = s.replace(/\\ddot\{\\theta\}/g, 'θ̈')
    .replace(/\\ddot\{\\phi\}/g, 'φ̈')
    .replace(/\\frac\{([^{}]*)\}\{([^{}]*)\}/g, '<span class="frac"><span class="frac-num">$1</span><span class="frac-den">$2</span></span>');
  const symbols = [
    ['\\left', ''], ['\\right', ''], ['\\,', ' '], ['\\;', ' '], ['\\!', ''], ['\\ ', ' '],
    ['\\theta', 'θ'], ['\\phi', 'φ'], ['\\alpha', 'α'], ['\\beta', 'β'], ['\\gamma', 'γ'],
    ['\\Delta', 'Δ'], ['\\delta', 'δ'], ['\\pi', 'π'], ['\\infty', '∞'],
    ['\\sin', 'sin'], ['\\cos', 'cos'], ['\\tan', 'tan'], ['\\log', 'log'], ['\\ln', 'ln'],
    ['\\cdot', '·'], ['\\times', '×'], ['\\pm', '±'], ['\\le', '≤'], ['\\ge', '≥'], ['\\neq', '≠'],
  ];
  for (const [cmd, ch] of symbols) s = s.split(cmd).join(ch);
  s = s.replace(/\^\{([^{}]+)\}/g, '<sup>$1</sup>')
    .replace(/\^([A-Za-z0-9])/g, '<sup>$1</sup>')
    .replace(/_\{([^{}]+)\}/g, '<sub>$1</sub>')
    .replace(/_([A-Za-z0-9])/g, '<sub>$1</sub>')
    .replace(/[{}]/g, '');
  return s.trim();
}

function inlineNode(node) {
  if (node.nodeType === 3) return escapeHTML(node.nodeValue);
  if (node.nodeType !== 1) return '';
  const tag = node.tagName.toLowerCase();
  switch (tag) {
    case 'b': case 'strong': return `<strong>${inlineHTML(node)}</strong>`;
    case 'i': case 'em': return `<em>${inlineHTML(node)}</em>`;
    case 'u': return `<u>${inlineHTML(node)}</u>`;
    case 's': case 'del': return `<del>${inlineHTML(node)}</del>`;
    case 'code': return `<code>${escapeHTML(node.textContent)}</code>`;
    case 'br': return '<br>';
    case 'latex': return `<span class="math-inline">${mathHTML(node.textContent)}</span>`;
    case 'cite': {
      const who = node.getAttribute('type') === 'user' ? '成员' : '引用';
      return `<span class="cite" title="${escapeAttr(who)}">@${escapeHTML(who)}</span>`;
    }
    case 'a': {
      const href = node.getAttribute('href') || '';
      const safe = /^(https?:|mailto:)/i.test(href) ? href : '#';
      return `<a href="${escapeAttr(safe)}" target="_blank" rel="noopener noreferrer">${inlineHTML(node)}</a>`;
    }
    case 'span': {
      const fg = color(node.getAttribute('text-color'));
      const bg = color(node.getAttribute('background-color'));
      const style = [fg && `color:${fg}`, bg && `background:${bg}`].filter(Boolean).join(';');
      return `<span${style ? ` style="${style}"` : ''}>${inlineHTML(node)}</span>`;
    }
    default: return inlineHTML(node);
  }
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
