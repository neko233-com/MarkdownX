// A deliberately small Markdown reader. It covers what books use in practice:
// headings, paragraphs, lists, quotes, fenced code, tables, images, rules and
// the `mdxb-canvas` / `mdxb-html` fences that carry interactive blocks.

import { escapeHTML, mathHTML } from './docxml.js';

let footnoteScope = 0;

export function parseMarkdown(src, title = '', resolveImage = () => '', sharedFootnotes = null, depth = 0) {
  if (depth > 64) return { title, blocks: [{ type: 'p', html: escapeHTML(String(src ?? '')).replace(/\n/g, '<br>') }] };
  const lines = expandReferenceImages(String(src ?? '').replace(/\r\n?/g, '\n'), resolveImage.link).split('\n');
  const root = sharedFootnotes === null;
  const footnotes = sharedFootnotes || collectFootnotes(lines);
  const definitions = root ? footnotes.byLine : new Map();
  const blocks = [];
  let i = 0;
  const isBlank = (s) => !s.trim();

  while (i < lines.length) {
    const line = lines[i];
    if (isBlank(line)) { i++; continue; }
    const definition = definitions.get(i);
    if (definition) {
      blocks.push({ type: 'footnote-placeholder', definition });
      i = definition.end;
      continue;
    }

    const fence = line.match(/^ {0,3}(`{3,}|~{3,})\s*([^\s`]*)\s*$/);
    if (fence) {
      const marker = fence[1][0];
      const lang = (fence[2] || '').toLowerCase();
      const body = [];
      i++;
      while (i < lines.length && !new RegExp(`^ {0,3}${marker}{${fence[1].length},}\\s*$`).test(lines[i])) body.push(lines[i++]);
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

    if (/^(?: {4}|\t)/.test(line)) {
      const body = [];
      while (i < lines.length) {
        if (/^(?: {4}|\t)/.test(lines[i])) { body.push(lines[i++].replace(/^(?: {4}|\t)/, '')); continue; }
        if (isBlank(lines[i])) {
          let next = i + 1;
          while (next < lines.length && isBlank(lines[next])) next++;
          if (next < lines.length && /^(?: {4}|\t)/.test(lines[next])) { body.push(''); i++; continue; }
        }
        break;
      }
      blocks.push({ type: 'code', lang: '', code: body.join('\n'), caption: '' });
      continue;
    }

    const displayMath = displayMathAt(lines, i);
    if (displayMath) {
      blocks.push({ type: 'p', html: `<span class="math-display">${mathHTML(displayMath.source, true)}</span>` });
      i = displayMath.end;
      continue;
    }

    const heading = line.match(/^(#{1,6})\s+(.*)$/);
    if (heading) {
      const anchorHtml = inlineMd(heading[2], resolveImage, null, true);
      blocks.push({ type: 'h', level: heading[1].length, html: inlineMd(heading[2], resolveImage, footnotes), anchorHtml, seq: null });
      i++;
      continue;
    }

    if (/^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/.test(line)) { blocks.push({ type: 'divider' }); i++; continue; }

    if (/^\s*>/.test(line)) {
      const body = [];
      while (i < lines.length && (/^\s*>/.test(lines[i]) || isBlank(lines[i]))) body.push(lines[i++].replace(/^\s*>\s?/, ''));
      blocks.push({ type: 'quote', blocks: parseMarkdown(body.join('\n'), '', resolveImage, footnotes, depth + 1).blocks });
      continue;
    }

    if (listMarker(line)) {
      const list = parseList(lines, i, definitions, resolveImage, footnotes, depth);
      blocks.push(list.block);
      i = list.end;
      continue;
    }

    if (tableStart(lines, i)) {
      const head = splitRow(line);
      const markers = splitRow(lines[i + 1]);
      i += 2;
      const rows = [];
      while (i < lines.length && !isBlank(lines[i]) && lines[i].includes('|')) rows.push(splitRow(lines[i++]));
      const cell = (text, col, header) => {
        const value = text || '';
        const pieces = value.includes('<markdownx-interactive data-base64="') ? value.split('<br>') : [value];
        const widgets = pieces.map(tableWidget);
        const hasWidget = widgets.some(Boolean);
        return {
          header, html: hasWidget ? '' : inlineMd(value, resolveImage, footnotes),
          blocks: hasWidget ? pieces.map((piece, index) => widgets[index] || { type: 'p', html: inlineMd(piece, resolveImage, footnotes) }) : [],
          colspan: 1, rowspan: 1,
          align: markers[col].startsWith(':') && markers[col].endsWith(':') ? 'center' : markers[col].endsWith(':') ? 'right' : null,
        };
      };
      blocks.push({
        type: 'table',
        cols: [],
        rows: [head.map((h, col) => cell(h, col, true)), ...rows.map((r) => head.map((_, col) => cell(r[col], col, false)))],
      });
      continue;
    }

    const htmlMedia = htmlMediaAt(lines, i);
    if (htmlMedia) {
      blocks.push({ type: 'interactive', kind: 'html', title: '', height: 360, source: htmlMedia.source });
      i = htmlMedia.end;
      continue;
    }

    const htmlLines = /^\s*<img(?=[\s/>]|$)/i.test(line) ? lines.slice(i, i + 64) : [];
    if (htmlLines.length) htmlLines[0] = htmlLines[0].trimStart();
    const htmlSource = htmlLines.join('\n').slice(0, 65536);
    const htmlImage = htmlImageAt(htmlSource, 0);
    const restOfLine = htmlImage ? htmlSource.slice(htmlImage.end).split('\n', 1)[0] : '';
    if (htmlImage && !restOfLine.trim()) {
      blocks.push({ type: 'image', src: htmlImage.src, caption: htmlImage.alt, width: htmlImage.width || 0, align: null });
      i += htmlSource.slice(0, htmlImage.end).split('\n').length;
      continue;
    }
    const imageLine = line.trim();
    const img = markdownImageAt(imageLine, 0);
    if (img && img.end === imageLine.length) { blocks.push({ type: 'image', src: img.src, caption: img.alt, width: 0, align: null }); i++; continue; }

    const para = [];
    while (i < lines.length && !isBlank(lines[i]) && !definitions.has(i) && !/^(#{1,6})\s/.test(lines[i]) && !/^ {0,3}(`{3,}|~{3,})/.test(lines[i]) && !listMarker(lines[i]) && !/^\s*>/.test(lines[i]) && !/^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/.test(lines[i]) && !tableStart(lines, i) && !htmlMediaAt(lines, i) && !displayMathAt(lines, i)) para.push(lines[i++]);
    if (para.length) blocks.push({ type: 'p', html: inlineMd(para.join('\n'), resolveImage, footnotes).replace(/\n/g, '<br>') });
  }
  if (root) {
    const notes = [];
    // Rendering a note can discover another referenced note; process that queue too.
    for (let n = 0; n < footnotes.used.length; n++) {
      const note = footnotes.used[n];
      footnotes.comment = false; footnotes.raw = null;
      notes.push({ id: note.id, number: n + 1, backlinks: note.backlinks,
        blocks: parseMarkdown(note.body, '', resolveImage, footnotes, depth + 1).blocks });
    }
    const kept = [];
    for (const block of blocks) {
      if (block.type !== 'footnote-placeholder') kept.push(block);
      else if (!block.definition.number) {
        const literal = { byLabel: new Map(), byLine: new Map(), used: [], scope: footnotes.scope, nextRef: 0 };
        kept.push(...parseMarkdown(block.definition.raw, '', resolveImage, literal, depth + 1).blocks);
      }
    }
    if (notes.length) kept.push({ type: 'footnotes', notes });
    return { title, blocks: kept };
  }
  return { title, blocks };
}

// Display math can span lines; an unmatched delimiter remains ordinary text.
function displayMathAt(lines, start) {
  const single = lines[start]?.match(/^ {0,3}\$\$[ \t]*(\S(?:.*?\S)?)[ \t]*\$\$[ \t]*$/);
  if (single) return { source: single[1], end: start + 1 };
  if (!/^ {0,3}\$\$[ \t]*$/.test(lines[start] || '')) return null;
  let end = start + 1;
  while (end < lines.length && !/^ {0,3}\$\$[ \t]*$/.test(lines[end])) end++;
  if (end === lines.length || !lines.slice(start + 1, end).join('\n').trim()) return null;
  return { source: lines.slice(start + 1, end).join('\n'), end: end + 1 };
}

// Reference definitions are scanned before quoted blocks are unwrapped.
function referenceDisplayMathAt(lines, start) {
  const strip = line => {
    let rest = line, depth = 0, match;
    while ((match = rest.match(/^ {0,3}>[ \t]?/))) { rest = rest.slice(match[0].length); depth++; }
    return { rest, depth };
  };
  const opening = strip(lines[start] || '');
  if (!opening.depth) return displayMathAt(lines, start)?.end || null;
  if (/^ {0,3}\$\$[ \t]*\S.*\$\$[ \t]*$/.test(opening.rest)) return start + 1;
  if (!/^ {0,3}\$\$[ \t]*$/.test(opening.rest)) return null;
  let hasBody = false;
  for (let i = start + 1; i < lines.length; i++) {
    const line = strip(lines[i]);
    if (line.depth !== opening.depth) return null;
    if (/^ {0,3}\$\$[ \t]*$/.test(line.rest)) return hasBody ? i + 1 : null;
    if (line.rest.trim()) hasBody = true;
  }
  return null;
}

// Locate TeX without treating escaped or currency-like dollars as delimiters.
function mathSpanAt(source, at) {
  if (source[at] !== '$' || source[at - 1] === '$') return null;
  let slash = at;
  while (slash > 0 && source[slash - 1] === '\\') slash--;
  if ((at - slash) % 2) return null;
  for (const delimiter of ['$$', '$']) {
    if (!source.startsWith(delimiter, at) || source[at + delimiter.length] === '$') continue;
    const begin = at + delimiter.length;
    if (!source[begin] || /\s/.test(source[begin])) continue;
    let end = begin;
    while (end < source.length && source[end] !== '\n') {
      if (source[end] === '\\' && end + 1 < source.length) { end += 2; continue; }
      if (source.startsWith(delimiter, end)) break;
      end++;
    }
    if (end >= source.length || source[end] === '\n' || end === begin || /\s/.test(source[end - 1])) continue;
    const after = source[end + delimiter.length];
    if (after === '$' || delimiter === '$' && /[0-9]/.test(after || '')) continue;
    return { source: source.slice(begin, end), end: end + delimiter.length, display: delimiter === '$$' };
  }
  return null;
}

// Protect TeX before Markdown emphasis and HTML escaping can change its source.
function replaceInlineMath(source, keep, anchor) {
  let out = '', start = 0, at = 0;
  while (at < source.length) {
    if (source[at] === '\\' && source[at + 1] === '$') {
      out += source.slice(start, at) + keep('$');
      at += 2; start = at; continue;
    }
    if (source[at] === '\\' && at + 1 < source.length) { at += 2; continue; }
    const math = mathSpanAt(source, at);
    if (!math) { at++; continue; }
    out += source.slice(start, at) + keep(anchor ? escapeHTML(math.source) : `<span class="math-inline">${mathHTML(math.source, math.display)}</span>`);
    at = math.end;
    start = at;
  }
  return out + source.slice(start);
}

// Indentation is measured in columns so tab-indented continuations behave
// like the same content written with spaces.
function indentation(line) {
  let column = 0, at = 0;
  while (at < line.length && (line[at] === ' ' || line[at] === '\t')) {
    column += line[at++] === '\t' ? 4 - column % 4 : 1;
  }
  return { column, at };
}

function stripIndent(line, count) {
  let column = 0, at = 0;
  while (at < line.length && column < count && (line[at] === ' ' || line[at] === '\t')) {
    column += line[at++] === '\t' ? 4 - column % 4 : 1;
  }
  return ' '.repeat(Math.max(0, column - count)) + line.slice(at);
}

function listMarker(line) {
  const match = String(line ?? '').match(/^( {0,3})([-+*]|\d{1,9}[.)。])([ \t]+)(.*)$/);
  if (!match) return null;
  const ordered = /^\d/.test(match[2]);
  const columns = (text) => [...text].reduce((column, ch) => column + (ch === '\t' ? 4 - column % 4 : 1), 0);
  const markerEnd = columns(match[1] + match[2]);
  const padding = columns(match[1] + match[2] + match[3]) - markerEnd <= 4 && match[4] ? match[3] : match[3][0];
  const prefix = match[1] + match[2] + padding;
  const contentIndent = columns(prefix);
  return { indent: match[1].length, contentIndent,
    marker: ordered ? match[2].slice(-1) : match[2], ordered,
    start: ordered ? Number(match[2].slice(0, -1)) : null,
    text: match[3].slice(padding.length) + match[4] };
}

function parseList(lines, start, definitions, resolveImage, footnotes, depth) {
  const first = listMarker(lines[start]);
  const sameList = (marker) => marker && marker.indent === first.indent
    && marker.ordered === first.ordered && marker.marker === first.marker;
  const items = [];
  let i = start;
  while (i < lines.length) {
    const marker = listMarker(lines[i]);
    if (!sameList(marker)) break;
    let text = marker.text, done = null;
    const task = text.match(/^\[([ xX])\]\s+(.*)$/);
    if (task) { done = task[1].toLowerCase() === 'x'; text = task[2]; }
    i++;
    const body = [];
    while (i < lines.length && !definitions.has(i)) {
      if (!lines[i].trim()) {
        let next = i + 1;
        while (next < lines.length && !lines[next].trim()) next++;
        if (next >= lines.length || definitions.has(next)) break;
        if (sameList(listMarker(lines[next]))) { i = next; break; }
        if (indentation(lines[next]).column < marker.contentIndent) break;
        while (i < next) { body.push(''); i++; }
        continue;
      }
      if (sameList(listMarker(lines[i]))) break;
      if (indentation(lines[i]).column >= marker.contentIndent) {
        body.push(stripIndent(lines[i], marker.contentIndent)); i++; continue;
      }
      // A plain line can continue the preceding list paragraph without a
      // blank line. Structural starts remain available to the outer parser.
      if (!body.length && !listMarker(lines[i]) && !/^ {0,3}(?:#{1,6}\s|>|`{3,}|~{3,})/.test(lines[i])
          && !tableStart(lines, i) && !htmlMediaAt(lines, i)) {
        text += '\n' + lines[i].trim(); i++; continue;
      }
      break;
    }
    const media = htmlMediaAt([text, ...body], 0);
    const nestedStart = indentation(text).column >= 4 || media;
    const html = nestedStart ? '' : inlineMd(text, resolveImage, footnotes).replace(/\n/g, '<br>');
    const childLines = nestedStart ? [text, ...body] : body;
    const children = childLines.length ? parseMarkdown(childLines.join('\n'), '', resolveImage, footnotes, depth + 1).blocks : [];
    items.push({ html, done, blocks: children });
  }
  return { block: { type: 'list', ordered: first.ordered, start: first.start, items }, end: i };
}

// Definitions are collected only at the top level. Ambiguous duplicates and
// unreferenced definitions remain visible as ordinary Markdown text.
function collectFootnotes(lines) {
  const masks = referenceMasks(lines), candidates = [], counts = new Map();
  for (let i = 0; i < lines.length; i++) {
    const match = lines[i].match(/^ {0,3}\[\^([^\]\\]+)\]:[ \t]*(.*)$/);
    if (!match || masks[i][lines[i].indexOf('[')] !== '[' || !match[1].trim()) continue;
    let end = i + 1;
    while (end < lines.length) {
      if (/^(?: {4}|\t)/.test(lines[end])) { end++; continue; }
      if (!lines[end].trim()) {
        let next = end + 1;
        while (next < lines.length && !lines[next].trim()) next++;
        if (next < lines.length && /^(?: {4}|\t)/.test(lines[next])) { end = next; continue; }
      }
      break;
    }
    const label = match[1], raw = lines.slice(i, end).join('\n');
    const body = [match[2], ...lines.slice(i + 1, end).map(line => line.replace(/^(?: {4}|\t)/, ''))].join('\n');
    candidates.push({ label, start: i, end, raw, body, number: 0, backlinks: [] });
    counts.set(label, (counts.get(label) || 0) + 1);
    i = end - 1;
  }
  const unique = candidates.filter(note => counts.get(note.label) === 1);
  return { byLabel: new Map(unique.map(note => [note.label, note])),
    byLine: new Map(unique.map(note => [note.start, note])), used: [], scope: ++footnoteScope, nextRef: 0 };
}

function referenceKey(label) {
  return label.replace(/\\([!"#$%&'()*+,\-./:;<=>?@\[\\\]^_`{|}~])/g, '$1').trim().replace(/\s+/g, ' ').toLowerCase();
}

function referenceTableSafe(destination) {
  let out = '', slashes = 0;
  for (const ch of destination) {
    if (ch === '|' && slashes % 2 === 0) out += '\\';
    out += ch;
    slashes = ch === '\\' ? slashes + 1 : 0;
  }
  return out;
}

function referenceImageAt(s, start) {
  if (!s.startsWith('![', start)) return null;
  const reference = referenceAt(s, start + 1);
  return reference && { ...reference, end: reference.end };
}

function referenceAt(s, start) {
  if (s[start] !== '[') return null;
  let at = start + 1, depth = 1;
  while (at < s.length) {
    if (s[at] === '\\' && at + 1 < s.length) { at += 2; continue; }
    if (s[at] === '[') depth++;
    else if (s[at] === ']' && --depth === 0) break;
    at++;
  }
  if (s[at] !== ']') return null;
  const alt = s.slice(start + 1, at++);
  if (s[at] === '(') return null;
  if (s[at] !== '[') return { alt, label: alt, end: at };
  const begin = ++at;
  while (at < s.length && s[at] !== ']') at += s[at] === '\\' && at + 1 < s.length ? 2 : 1;
  if (s[at] !== ']') return null;
  return { alt, label: s.slice(begin, at) || alt, end: at + 1 };
}

// Mask literal regions before collecting definitions or substituting uses.
function referenceMasks(lines) {
  const result = [], state = { fence: null, comment: false, raw: null };
  let mediaEnd = 0, mathEnd = 0;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i], mask = line.split('');
    const erase = (a, b) => { for (let n = a; n < b; n++) mask[n] = ' '; };
    const fence = line.match(/^\s{0,3}(`{3,}|~{3,})(.*)$/);
    if (i < mediaEnd || i < mathEnd) { result.push(' '.repeat(line.length)); continue; }
    if (state.fence) {
      if (fence && fence[1][0] === state.fence[0] && fence[1].length >= state.fence.length && !fence[2].trim()) state.fence = null;
      result.push(' '.repeat(line.length)); continue;
    }
    if (!state.comment && !state.raw && fence) { state.fence = fence[1]; result.push(' '.repeat(line.length)); continue; }
    if (!state.comment && !state.raw) {
      const end = referenceDisplayMathAt(lines, i);
      if (end) { mathEnd = end; result.push(' '.repeat(line.length)); continue; }
      const media = htmlMediaAt(lines, i);
      if (media) { mediaEnd = media.end; result.push(' '.repeat(line.length)); continue; }
    }
    let at = 0;
    while (at < line.length) {
      if (state.comment) {
        const close = line.indexOf('-->', at), end = close < 0 ? line.length : close + 3;
        erase(at, end); at = end; if (close >= 0) state.comment = false; continue;
      }
      if (state.raw) {
        const match = new RegExp(`</${state.raw}\\s*>`, 'i').exec(line.slice(at));
        const end = match ? at + match.index + match[0].length : line.length;
        erase(at, end); at = end; if (match) state.raw = null; continue;
      }
      if (line.startsWith('<!--', at)) { state.comment = true; continue; }
      if (line[at] === '\\' && at + 1 < line.length) { erase(at, at + 2); at += 2; continue; }
      if (line[at] === '`') {
        const run = line.slice(at).match(/^`+/)[0];
        const close = line.indexOf(run, at + run.length);
        const end = close < 0 ? at + run.length : close + run.length;
        erase(at, end); at = end; continue;
      }
      const math = mathSpanAt(line, at);
      if (math) { erase(at, math.end); at = math.end; continue; }
      if (line[at] === '<') {
        const raw = line.slice(at).match(/^<(script|style|textarea|title|pre|code)(?=[\s>])/i);
        if (raw) { state.raw = raw[1]; continue; }
        const end = htmlTagEnd(line, at);
        if (end > 0 && /^<\/?[a-z]/i.test(line.slice(at))) { erase(at, end); at = end; continue; }
      }
      at++;
    }
    result.push(mask.join(''));
  }
  return result;
}

function expandReferenceImages(source, resolveLink = () => '') {
  if (!source.includes(']:')) return source;
  const lines = source.split('\n'), masks = referenceMasks(lines), definitions = new Map(), defLines = new Set();
  for (let i = 0; i < lines.length; i++) {
    const def = lines[i].match(/^ {0,3}\[((?:\\.|[^\[\]\\]){1,999})\]:\s*(.*?)\s*$/);
    if (!def || def[1].startsWith('^') || masks[i][lines[i].indexOf('[')] !== '[') continue;
    const probe = `![](${def[2]})`, image = markdownImageAt(probe, 0);
    if (!image || image.end !== probe.length) continue;
    const key = referenceKey(def[1]);
    if (!key) continue;
    if (!definitions.has(key)) definitions.set(key, def[2]);
    defLines.add(i);
  }
  if (!definitions.size) return source;
  const unresolvedLinks = new Set();
  for (let i = 0; i < lines.length; i++) {
    if (defLines.has(i)) continue;
    let out = '', at = 0, start = 0;
    while (at < lines[i].length) {
      if (masks[i][at] === '!') {
        const direct = markdownImageAt(lines[i], at);
        if (direct) { at = direct.end; continue; }
        const image = referenceImageAt(lines[i], at), destination = image && definitions.get(referenceKey(image.label));
        if (destination) {
          out += lines[i].slice(start, at) + `![${image.alt}](${referenceTableSafe(destination)})`;
          at = image.end; start = at; continue;
        }
      } else if (masks[i][at] === '[') {
        const link = referenceAt(lines[i], at), key = link && referenceKey(link.label), destination = key && definitions.get(key);
        if (destination) {
          const parsed = markdownImageAt(`![](${destination})`, 0);
          if (parsed && (/^(https?:|mailto:|ftp:)/i.test(parsed.src) || resolveLink?.(parsed.src))) {
            const url = parsed.src.replace(/\(/g, '%28').replace(/\)/g, '%29').replace(/ /g, '%20');
            out += lines[i].slice(start, at) + `[${link.alt}](${referenceTableSafe(url)})`;
            at = link.end; start = at; continue;
          }
          unresolvedLinks.add(key);
        }
      }
      at++;
    }
    lines[i] = out + lines[i].slice(start);
  }
  for (const i of defLines) {
    const def = lines[i].match(/^ {0,3}\[((?:\\.|[^\[\]\\]){1,999})\]:/);
    if (def && !unresolvedLinks.has(referenceKey(def[1]))) lines[i] = '';
  }
  return lines.join('\n');
}

// A balanced div containing real media; markup-looking script/comment text is inert.
function htmlMediaContainer(lines, start) {
  const parts = []; let size = 0;
  for (const line of lines.slice(start, start + 1024)) {
    size += line.length + 1;
    if (size > 256 * 1024) break;
    parts.push(line);
  }
  const source = parts.join('\n').replace(/[A-Z]/g, c => c.toLowerCase());
  let at = 0, depth = 0, media = false, raw = '';
  while (at < source.length) {
    if (!raw && at > 0 && source[at - 1] === '\n' && /^\s*(`{3,}|~{3,})/.test(source.slice(at).split('\n', 1)[0])) return -1;
    if (raw) {
      const close = `</${raw}`;
      do { at = source.indexOf(close, at); if (at < 0) return -1; if (/[\s>]/.test(source[at + close.length] || '')) break; at += close.length; } while (at < source.length);
      if (at >= source.length) return -1;
      raw = '';
    }
    if (source[at] !== '<') { at++; continue; }
    if (source.startsWith('<!--', at)) { const end = source.indexOf('-->', at + 4); if (end < 0) return -1; at = end + 3; continue; }
    let p = at + 1; const closing = source[p] === '/'; if (closing) p++;
    const name = source.slice(p).match(/^[a-z0-9:-]+/);
    if (!name) { at++; continue; }
    const tag = name[0]; p += tag.length;
    let quote = '';
    for (; p < source.length; p++) {
      const c = source[p];
      if (quote && c === quote) quote = '';
      else if (!quote && (c === '"' || c === "'")) quote = c;
      else if (!quote && c === '>') break;
    }
    if (p >= source.length) return -1;
    const selfClosing = source.slice(at, p).trimEnd().endsWith('/');
    if (tag === 'div') {
      if (closing) {
        if (--depth < 0) return -1;
        if (!depth) {
          const end = p + 1, tail = source.slice(end).split('\n', 1)[0].trim();
          if (!media || (tail && !tail.startsWith('<script') && !tail.startsWith('<style'))) return -1;
          return start + source.slice(0, end).split('\n').length;
        }
      } else if (!selfClosing) depth++;
    } else if (!closing && depth > 0) {
      if (['video', 'audio', 'canvas', 'model-viewer'].includes(tag)) media = true;
      if (['script', 'style', 'textarea', 'title'].includes(tag) && !selfClosing) raw = tag;
    }
    at = p + 1;
  }
  return -1;
}

function htmlMediaAt(lines, start) {
  const opening = lines[start]?.match(/^\s*<(video|audio|canvas|model-viewer|div)(?=[\s/>])/i);
  if (!opening) return null;
  const tag = opening[1].toLowerCase();
  const throughClose = (from, name) => {
    const close = `</${name}>`;
    for (let at = from; at < lines.length; at++) {
      if (name !== 'script' && name !== 'style' && at > from && /^\s*(`{3,}|~{3,})/.test(lines[at])) return -1;
      if (lines[at].toLowerCase().includes(close)) return at + 1;
    }
    return -1;
  };
  const throughTag = (from) => {
    let quote = '';
    for (let at = from; at < lines.length; at++) for (const ch of lines[at]) {
      if (quote && ch === quote) quote = '';
      else if (!quote && (ch === '"' || ch === "'")) quote = ch;
      else if (!quote && ch === '>') return at + 1;
    }
    return -1;
  };
  let end = tag === 'div' ? htmlMediaContainer(lines, start) : throughClose(start, tag);
  if (end < 0) return null;
  const tail = lines[end - 1].toLowerCase().split(`</${tag}>`).slice(1).join(`</${tag}>`);
  if (tail.includes('<script') && !tail.includes('</script>')) {
    end = throughClose(end, 'script');
    if (end < 0) return null;
  } else if (tail.includes('<style') && !tail.includes('</style>')) {
    end = throughClose(end, 'style');
    if (end < 0) return null;
  }
  while (end < lines.length) {
    let next = end;
    while (next < lines.length && !lines[next].trim()) next++;
    if (/^\s*<script(?=[\s/>])/i.test(lines[next] || '')) end = throughClose(next, 'script');
    else if (/^\s*<style(?=[\s/>])/i.test(lines[next] || '')) end = throughClose(next, 'style');
    else if (/^\s*<link(?=[\s/>])/i.test(lines[next] || '')) end = throughTag(next);
    else break;
    if (end < 0) return null;
  }
  return { source: lines.slice(start, end).join('\n'), end };
}

function tableWidget(value) {
  const encoded = value.match(/^<markdownx-interactive data-base64="([A-Za-z0-9+/=]+)"><\/markdownx-interactive>$/);
  if (encoded) {
    try {
      const bytes = Uint8Array.from(atob(encoded[1]), ch => ch.charCodeAt(0));
      const obj = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
      if (!['html', 'canvas'].includes(obj.kind) || typeof obj.source !== 'string' || !obj.source) return null;
      return { type: 'interactive', kind: obj.kind, title: typeof obj.title === 'string' ? obj.title : '', height: clamp(Number(obj.height || 360), 120, 1600), source: obj.source };
    } catch { return null; }
  }
  const media = htmlMediaAt([value], 0);
  return media ? { type: 'interactive', kind: 'html', title: '', height: 360, source: media.source } : null;
}

function splitRow(line) {
  let text = line.trim().replace(/^\|/, '');
  if (text.endsWith('|') && !text.endsWith('\\|')) text = text.slice(0, -1);
  const cells = [];
  let cell = '';
  for (let i = 0; i < text.length; i++) {
    if (text[i] === '\\' && text[i + 1] === '|') { cell += '|'; i++; }
    else if (text[i] === '|') { cells.push(cell.trim()); cell = ''; }
    else cell += text[i];
  }
  cells.push(cell.trim());
  return cells;
}

function tableStart(lines, i) {
  if (!lines[i]?.includes('|') || !lines[i + 1]?.includes('-')) return false;
  const head = splitRow(lines[i]), markers = splitRow(lines[i + 1]);
  return head.length === markers.length && markers.every((marker) => /^:?-+:?$/.test(marker));
}

export function inlineMd(s, resolveImage = () => '', footnotes = null, anchor = false) {
  const stash = [];
  const keep = (html) => `\u0000${stash.push(html) - 1}\u0000`;
  let out = String(s ?? '').replace(/\u0000/g, '\ufffd');
  out = out.replace(/`([^`]+)`/g, (_, code) => keep(`<code>${escapeHTML(code)}</code>`));
  out = replaceMarkdownImages(out, (image) => {
    if (anchor) return keep(escapeHTML(image.alt));
    const src = safeSrc(resolveImage(image.src) || image.src);
    return keep(src ? `<img class="inline-image" src="${escapeHTML(src)}" alt="${escapeHTML(image.alt)}" loading="lazy">` : `<span>${escapeHTML(image.alt)}</span>`);
  });
  out = replaceHtmlImages(out, (image) => {
    if (anchor) return keep(escapeHTML(image.alt));
    const src = safeSrc(resolveImage(image.src) || image.src);
    const width = image.width ? ` width="${image.width}"` : '';
    const height = image.height ? ` height="${image.height}"` : '';
    return keep(src ? `<img class="inline-image" src="${escapeHTML(src)}" alt="${escapeHTML(image.alt)}"${width}${height} loading="lazy">` : `<span>${escapeHTML(image.alt)}</span>`);
  });
  out = replaceMarkdownLinks(out, (link) => {
    const resolved = resolveImage.link?.(link.src) || safeHref(link.src);
    const internal = resolved.startsWith('#mdxb-chapter/') || resolved.startsWith('#mdxb-pdf/');
    let label = replaceInlineMath(link.rawAlt, keep, anchor);
    label = label.replace(/\\([!"#$%&'()*+,\-./:;<=>?@\[\\\]^_`{|}~])/g, '$1');
    label = escapeHTML(label).replace(/\u0000(\d+)\u0000/g, (_, n) => stash[Number(n)] ?? '\ufffd');
    return keep(`<a href="${escapeHTML(resolved)}"${internal ? '' : ' target="_blank" rel="noopener noreferrer"'}>${label}</a>`);
  });
  if (footnotes) out = replaceFootnoteReferences(out, footnotes, keep);
  out = replaceInlineMath(out, keep, anchor);
  out = escapeHTML(out);
  out = out.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>').replace(/__([^_]+)__/g, '<strong>$1</strong>');
  out = out.replace(/(^|[\s(])\*([^*\n]+)\*/g, '$1<em>$2</em>').replace(/(^|[\s(])_([^_\n]+)_/g, '$1<em>$2</em>');
  out = out.replace(/~~([^~]+)~~/g, '<del>$1</del>');
  return out.replace(/\u0000(\d+)\u0000/g, (_, n) => stash[Number(n)]);
}

function replaceFootnoteReferences(s, footnotes, keep) {
  let out = '', at = 0;
  while (at < s.length) {
    if (footnotes.comment) {
      const close = s.indexOf('-->', at), end = close < 0 ? s.length : close + 3;
      out += s.slice(at, end); at = end; if (close >= 0) footnotes.comment = false; continue;
    }
    if (footnotes.raw) {
      const close = new RegExp(`</${footnotes.raw}\\s*>`, 'i').exec(s.slice(at));
      const end = close ? at + close.index + close[0].length : s.length;
      out += s.slice(at, end); at = end; if (close) footnotes.raw = null; continue;
    }
    if (s[at] === '\\' && at + 1 < s.length) { out += s.slice(at, at + 2); at += 2; continue; }
    const math = mathSpanAt(s, at);
    if (math) { out += s.slice(at, math.end); at = math.end; continue; }
    if (s[at] === '<') {
      if (s.startsWith('<!--', at)) { footnotes.comment = true; continue; }
      const raw = s.slice(at).match(/^<(script|style|textarea|title|pre|code)(?=[\s>])/i);
      if (raw) { footnotes.raw = raw[1]; continue; }
      const end = htmlTagEnd(s, at);
      if (end > 0 && /^<\/?[a-z]/i.test(s.slice(at))) { out += s.slice(at, end); at = end; continue; }
    }
    if (s.startsWith('[^', at) && (at === 0 || s[at - 1] !== '!')) {
      const end = s.indexOf(']', at + 2);
      const note = end > at + 2 && footnotes.byLabel.get(s.slice(at + 2, end));
      if (note) {
        if (!note.number) {
          note.number = footnotes.used.length + 1;
          note.id = `mdxfn-${footnotes.scope}-note-${note.number}`;
          footnotes.used.push(note);
        }
        const id = `mdxfn-${footnotes.scope}-ref-${++footnotes.nextRef}`;
        note.backlinks.push(id);
        out += keep(`<sup class="footnote-ref"><a id="${id}" href="#${note.id}">${note.number}</a></sup>`);
        at = end + 1; continue;
      }
    }
    out += s[at++];
  }
  return out;
}

function replaceMarkdownLinks(s, render) {
  let out = '', at = 0;
  while (at < s.length) {
    if (s[at] === '\\' && at + 1 < s.length) { out += s.slice(at, at + 2); at += 2; continue; }
    const math = mathSpanAt(s, at);
    if (math) { out += s.slice(at, math.end); at = math.end; continue; }
    if (s[at] === '<') {
      if (s.startsWith('<!--', at)) {
        const close = s.indexOf('-->', at + 4), end = close < 0 ? s.length : close + 3;
        out += s.slice(at, end); at = end; continue;
      }
      const raw = s.slice(at).match(/^<(script|style|textarea|title)(?=[\s>])/i);
      if (raw) {
        const close = new RegExp(`</${raw[1]}\\s*>`, 'i').exec(s.slice(at + raw[0].length));
        const end = close ? at + raw[0].length + close.index + close[0].length : s.length;
        out += s.slice(at, end); at = end; continue;
      }
      const end = htmlTagEnd(s, at);
      if (end > 0 && /^<\/?[a-z]/i.test(s.slice(at))) { out += s.slice(at, end); at = end; continue; }
    }
    if (s[at] === '[') {
      const link = markdownImageAt('!' + s.slice(at), 0);
      if (link) { out += render(link); at += link.end - 1; continue; }
    }
    out += s[at++];
  }
  return out;
}

function markdownImageAt(s, start) {
  if (!s.startsWith('![', start)) return null;
  let at = start + 2, depth = 1;
  while (at < s.length) {
    if (s[at] === '\\' && at + 1 < s.length) { at += 2; continue; }
    if (s[at] === '[') depth++;
    else if (s[at] === ']' && --depth === 0) break;
    at++;
  }
  if (s.slice(at, at + 2) !== '](') return null;
  const alt = s.slice(start + 2, at);
  at += 2;
  while (/\s/.test(s[at] || '')) at++;
  const angle = s[at] === '<';
  if (angle) at++;
  const begin = at;
  depth = 0;
  while (at < s.length) {
    const ch = s[at];
    if (ch === '\\' && at + 1 < s.length) { at += 2; continue; }
    if (angle) { if (ch === '>') break; }
    else if (ch === '(') depth++;
    else if (ch === ')') { if (!depth) break; depth--; }
    else if (/\s/.test(ch) && !depth) break;
    at++;
  }
  const src = s.slice(begin, at);
  if (!src || depth || angle && s[at] !== '>') return null;
  if (angle) at++;
  while (/\s/.test(s[at] || '')) at++;
  if (s[at] === '"' || s[at] === "'") {
    const quote = s[at++];
    while (at < s.length && s[at] !== quote) at += s[at] === '\\' && at + 1 < s.length ? 2 : 1;
    if (s[at] !== quote) return null;
    at++;
    while (/\s/.test(s[at] || '')) at++;
  }
  if (s[at] !== ')') return null;
  const unescape = (value) => value.replace(/\\([!"#$%&'()*+,\-./:;<=>?@\[\\\]^_`{|}~])/g, '$1');
  return { end: at + 1, src: unescape(src), alt: unescape(alt), rawAlt: alt };
}

function replaceMarkdownImages(s, render) {
  let out = '', at = 0;
  while (at < s.length) {
    if (s[at] === '\\' && at + 1 < s.length) { out += s.slice(at, at + 2); at += 2; continue; }
    const math = mathSpanAt(s, at);
    if (math) { out += s.slice(at, math.end); at = math.end; continue; }
    if (s[at] === '<') {
      if (s.startsWith('<!--', at)) {
        const close = s.indexOf('-->', at + 4), end = close < 0 ? s.length : close + 3;
        out += s.slice(at, end); at = end; continue;
      }
      const raw = s.slice(at).match(/^<(script|style|textarea|title)(?=[\s>])/i);
      if (raw) {
        const close = new RegExp(`</${raw[1]}\\s*>`, 'i').exec(s.slice(at + raw[0].length));
        const end = close ? at + raw[0].length + close.index + close[0].length : s.length;
        out += s.slice(at, end); at = end; continue;
      }
      const end = htmlTagEnd(s, at);
      if (end > 0 && /^<\/?[a-z]/i.test(s.slice(at))) { out += s.slice(at, end); at = end; continue; }
    }
    const image = markdownImageAt(s, at);
    if (image) { out += render(image); at = image.end; }
    else out += s[at++];
  }
  return out;
}

function htmlTagEnd(s, start) {
  let quote = '';
  for (let at = start + 1; at < s.length; at++) {
    const ch = s[at];
    if (quote && ch === quote) quote = '';
    else if (!quote && (ch === '"' || ch === "'")) quote = ch;
    else if (!quote && ch === '>') return at + 1;
  }
  return -1;
}

function htmlEntity(value) {
  if (typeof document !== 'undefined' && typeof document.createElement === 'function') {
    const area = document.createElement('textarea');
    area.innerHTML = value.replace(/</g, '&lt;');
    if (typeof area.value === 'string') return area.value;
  }
  return value.replace(/&(#x[0-9a-f]+|#\d+|amp|lt|gt|quot|apos|nbsp);/gi, (whole, token) => {
    const names = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: '\u00a0' };
    if (token[0] !== '#') return names[token.toLowerCase()] || whole;
    const n = token[1].toLowerCase() === 'x' ? parseInt(token.slice(2), 16) : parseInt(token.slice(1), 10);
    return n > 0 && n <= 0x10ffff && !(n >= 0xd800 && n <= 0xdfff) ? String.fromCodePoint(n) : '\ufffd';
  });
}

function htmlImageAt(s, start) {
  if (!/^<img(?=[\s/>])/i.test(s.slice(start))) return null;
  const end = htmlTagEnd(s, start);
  if (end < 0) return null;
  const attrs = Object.create(null);
  let at = start + 4;
  while (at < end) {
    const space = s.slice(at, end).match(/^\s+/);
    if (space) at += space[0].length;
    if (at >= end || s[at] === '>' || s[at] === '/' && s[at + 1] === '>') break;
    const name = s.slice(at, end).match(/^[^\s=/>]+/);
    if (!name) { at++; continue; }
    at += name[0].length;
    const key = name[0].toLowerCase();
    while (/\s/.test(s[at] || '')) at++;
    let value = '';
    if (s[at] === '=') {
      at++;
      while (/\s/.test(s[at] || '')) at++;
      const q = s[at];
      if (q === '"' || q === "'") {
        const begin = ++at;
        while (at < end && s[at] !== q) at++;
        value = s.slice(begin, at);
        if (s[at] === q) at++;
      } else {
        const begin = at;
        while (at < end && !/[\s>]/.test(s[at])) at++;
        value = s.slice(begin, at);
      }
    }
    if (!(key in attrs)) attrs[key] = htmlEntity(value);
  }
  const src = (attrs.src || '').trim();
  if (!src) return null;
  const dimension = (name) => {
    const css = (attrs.style || '').split(';').map(part => part.split(':')).find(part => part[0]?.trim().toLowerCase() === name);
    const value = (css?.slice(1).join(':') || attrs[name] || '').trim();
    return /^(?:\d+(?:\.\d+)?|\.\d+)(?:px)?$/i.test(value) ? Math.min(4096, Number(value.replace(/px$/i, ''))) : 0;
  };
  return { end, src, alt: attrs.alt || '', width: dimension('width'), height: dimension('height') };
}

function replaceHtmlImages(s, render) {
  let out = '', at = 0;
  while (at < s.length) {
    const math = mathSpanAt(s, at);
    if (math) { out += s.slice(at, math.end); at = math.end; continue; }
    if (s[at] !== '<') { out += s[at++]; continue; }
    let slash = at;
    while (slash > 0 && s[slash - 1] === '\\') slash--;
    if ((at - slash) % 2) { out += s[at++]; continue; }
    if (s.startsWith('<!--', at)) {
      const close = s.indexOf('-->', at + 4), end = close < 0 ? s.length : close + 3;
      out += s.slice(at, end); at = end; continue;
    }
    const raw = s.slice(at).match(/^<(script|style|textarea|title)(?=[\s>])/i);
    if (raw) {
      const close = new RegExp(`</${raw[1]}\\s*>`, 'i').exec(s.slice(at + raw[0].length));
      const end = close ? at + raw[0].length + close.index + close[0].length : s.length;
      out += s.slice(at, end); at = end; continue;
    }
    const image = htmlImageAt(s, at);
    if (image) { out += render(image); at = image.end; continue; }
    const end = htmlTagEnd(s, at);
    if (end > 0 && /^<\/?[a-z]/i.test(s.slice(at))) { out += s.slice(at, end); at = end; }
    else out += s[at++];
  }
  return out;
}

function safeHref(href) {
  const s = String(href).trim();
  return /^(https?:|mailto:|ftp:|#)/i.test(s) ? s : '#';
}
function safeSrc(src) {
  const s = String(src).trim();
  return /^(https?:|data:image\/)/i.test(s) ? s : '';
}
function clamp(n, a, b) { return Number.isFinite(n) ? Math.max(a, Math.min(b, n)) : a; }
