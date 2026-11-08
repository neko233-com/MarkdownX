'use strict';

const $ = (id) => document.getElementById(id);
const state = {
  docs: [],
  doc: null,
  rev: 1,
  clientId: '',
  peers: [],
  ws: null,
  connected: false,
  dirty: false,
  inFlight: false,
  queued: false,
  sent: '',
  saveTimer: 0,
  reconnectTimer: 0,
  closing: false,
  token: localStorage.getItem('mx-server-token') || new URLSearchParams(location.search).get('token') || '',
  name: localStorage.getItem('mx-server-name') || '',
  commits: [],
  selected: '',
  pendingRemote: null,
  composing: false,
};

if (!state.name) {
  state.name = 'Reader-' + Math.random().toString(36).slice(2, 6);
  localStorage.setItem('mx-server-name', state.name);
}
if (state.token) localStorage.setItem('mx-server-token', state.token);
$('user-name').value = state.name;

function toast(text) {
  const el = $('toast');
  el.textContent = text;
  el.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => { el.hidden = true; }, 3200);
}

function escapeHTML(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
}

async function api(path, options = {}) {
  const headers = Object.assign({'Content-Type': 'application/json'}, options.headers || {});
  if (state.token) headers.Authorization = 'Bearer ' + state.token;
  const res = await fetch(path, Object.assign({}, options, {headers}));
  if (res.status === 401) {
    openToken();
    throw new Error('unauthorized');
  }
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || res.status + ' ' + res.statusText);
  return data;
}

function setStatus(text, kind) {
  const el = $('save-state');
  el.textContent = text;
  el.className = 'pill' + (kind ? ' ' + kind : '');
}

function updateCount() {
  const text = $('editor').value;
  const cjk = (text.match(/[\u3400-\u9fff]/g) || []).length;
  const words = text.trim() ? text.trim().split(/\s+/).filter(Boolean).length : 0;
  $('word-count').textContent = cjk ? `${cjk} 字 · ${words} words` : `${words} words`;
}

function renderDocs() {
  const q = $('doc-filter').value.trim().toLowerCase();
  const list = $('doc-list');
  list.innerHTML = '';
  const docs = state.docs.filter((d) => !q || (d.id + ' ' + d.title).toLowerCase().includes(q));
  if (!docs.length) {
    list.innerHTML = '<p class="muted" style="padding:10px 8px;font-size:12px">没有文档。点“新建”开始。</p>';
    return;
  }
  const groups = new Map();
  for (const doc of docs) {
    const cut = doc.id.lastIndexOf('/');
    const folder = cut < 0 ? '' : doc.id.slice(0, cut);
    if (!groups.has(folder)) groups.set(folder, []);
    groups.get(folder).push(doc);
  }
  for (const folder of [...groups.keys()].sort()) {
    const head = document.createElement('div');
    head.className = 'doc-folder';
    head.textContent = folder || '根目录';
    list.appendChild(head);
    for (const doc of groups.get(folder)) {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'doc-item' + (state.doc && state.doc.id === doc.id ? ' active' : '');
      const leaf = doc.id.slice(folder ? folder.length + 1 : 0);
      b.innerHTML = `<strong>${escapeHTML(doc.title || leaf)}</strong><span>${escapeHTML(leaf)}</span>`;
      b.title = doc.id;
      b.onclick = () => openDoc(doc.id);
      list.appendChild(b);
    }
  }
}

async function loadDocs() {
  try {
    const data = await api('/api/docs');
    state.docs = data.docs || [];
    renderDocs();
  } catch (err) {
    if (err.message !== 'unauthorized') toast(err.message);
  }
}

function openToken() {
  const d = $('token-dialog');
  if (!d.open) d.showModal();
  $('token-input').value = state.token;
}

async function openDoc(id) {
  if (!id) return;
  if (state.dirty && !confirm('当前文档还有未保存的修改，仍要切换吗？')) return;
  closeSocket();
  state.doc = {id};
  state.rev = 1;
  state.dirty = false;
  state.inFlight = false;
  state.queued = false;
  state.pendingRemote = null;
  $('banner').hidden = true;
  $('editor').value = '';
  $('editor').disabled = true;
  $('doc-title').textContent = id;
  $('doc-id').textContent = id;
  history.replaceState(null, '', '#doc=' + encodeURIComponent(id));
  await loadDocs();
  connect(id);
}

function connect(id) {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  const qs = new URLSearchParams({name: state.name});
  if (state.token) qs.set('token', state.token);
  const ws = new WebSocket(`${proto}//${location.host}/ws/doc?id=${encodeURIComponent(id)}&${qs}`);
  state.ws = ws;
  state.closing = false;
  setStatus('连接中…');
  ws.onopen = () => {
    state.connected = true;
    setStatus('已连接', 'live');
  };
  ws.onmessage = (ev) => {
    let msg;
    try { msg = JSON.parse(ev.data); } catch { return; }
    handle(msg);
  };
  ws.onclose = () => {
    state.connected = false;
    setStatus('已断开');
    $('editor').disabled = true;
    if (!state.closing && state.doc) {
      clearTimeout(state.reconnectTimer);
      state.reconnectTimer = setTimeout(() => connect(state.doc.id), 1600);
    }
  };
  ws.onerror = () => setStatus('连接失败');
}

function closeSocket() {
  state.closing = true;
  clearTimeout(state.reconnectTimer);
  if (state.ws) {
    try { state.ws.close(); } catch {}
  }
  state.ws = null;
  state.connected = false;
}

function handle(msg) {
  switch (msg.type) {
    case 'snapshot':
      state.clientId = msg.client_id;
      state.doc = {id: msg.id, title: msg.title};
      $('editor').disabled = false;
      $('doc-title').textContent = msg.title || msg.id;
      $('doc-id').textContent = msg.id;
      state.peers = msg.peers || [];
      renderPeers();
      state.inFlight = false;
      if (state.dirty && $('editor').value && $('editor').value !== (msg.content || '')) {
        // Reconnect after a drop: keep local text and let the user choose.
        state.pendingRemote = msg;
        showRemoteBanner(msg);
        setStatus('待合并', 'dirty');
      } else {
        state.rev = msg.rev;
        state.sent = msg.content || '';
        state.dirty = false;
        $('editor').value = msg.content || '';
        $('rev-label').textContent = 'rev ' + msg.rev;
        setStatus('已同步', 'live');
      }
      updateCount();
      loadHistory();
      break;
    case 'update':
      if (msg.origin === state.clientId) {
        const editor = $('editor');
        const pendingLocal = editor.value !== state.sent;
        state.inFlight = false;
        if (!pendingLocal) {
          state.rev = msg.rev;
          state.sent = msg.content || '';
          state.dirty = false;
          if (msg.content != null && msg.content !== editor.value) {
            const focused = document.activeElement === editor;
            const start = editor.selectionStart;
            const end = editor.selectionEnd;
            state.suppress = true;
            editor.value = msg.content;
            state.suppress = false;
            if (focused) {
              const max = editor.value.length;
              editor.setSelectionRange(Math.min(start, max), Math.min(end, max));
            }
            updateCount();
          }
          $('rev-label').textContent = 'rev ' + msg.rev;
          setStatus('已保存', 'live');
        }
        if (state.queued) {
          state.queued = false;
          scheduleSave(80);
        }
        break;
      }
      // Do not advance our base revision while local edits are pending:
      // keeping the older base is what lets the server three-way merge the
      // local and remote changes instead of silently overwriting the remote.
      // A focused but clean editor can safely take the remote text; keep the
      // caret near where the reader left it.
      if (state.dirty || state.composing) {
        state.pendingRemote = msg;
        showRemoteBanner(msg);
      } else {
        const editor = $('editor');
        const focused = document.activeElement === editor;
        const start = editor.selectionStart;
        const end = editor.selectionEnd;
        const top = editor.scrollTop;
        state.rev = msg.rev;
        state.sent = msg.content || '';
        state.dirty = false;
        state.suppress = true;
        editor.value = msg.content || '';
        state.suppress = false;
        if (focused) {
          const max = editor.value.length;
          editor.setSelectionRange(Math.min(start, max), Math.min(end, max));
          editor.scrollTop = top;
        }
        $('doc-title').textContent = msg.title || msg.id;
        $('rev-label').textContent = 'rev ' + msg.rev;
        updateCount();
        setStatus('已同步', 'live');
      }
      break;
    case 'saved':
      state.inFlight = false;
      $('commit-label').textContent = msg.commit ? msg.commit.slice(0, 7) : '';
      if (msg.warning) toast('已保存，但 Git 提交失败：' + msg.warning);
      // Only advance the base when no newer local keystrokes are waiting.
      // Otherwise keep the old base so the next save can be three-way merged.
      if ($('editor').value === state.sent) {
        state.rev = msg.rev;
        state.dirty = false;
        $('rev-label').textContent = 'rev ' + msg.rev;
        setStatus('已保存', 'live');
      }
      if (state.queued) {
        state.queued = false;
        scheduleSave(80);
      }
      loadHistory();
      break;
    case 'conflict':
      state.inFlight = false;
      showConflict(msg);
      break;
    case 'presence':
      state.peers = msg.peers || [];
      renderPeers();
      break;
    case 'cursor':
      break;
    case 'error':
      state.inFlight = false;
      toast(msg.message || '服务器错误');
      setStatus('保存失败', 'dirty');
      break;
  }
}

function renderPeers() {
  const box = $('peers');
  box.innerHTML = '';
  for (const p of state.peers) {
    const el = document.createElement('span');
    el.className = 'peer';
    el.innerHTML = `<i style="background:${escapeHTML(p.color || '#b05226')}"></i>${escapeHTML(p.name || 'Reader')}`;
    box.appendChild(el);
  }
}

function showRemoteBanner(msg) {
  const b = $('banner');
  b.hidden = false;
  b.innerHTML = `<span>${escapeHTML(msg.by || '其他人')} 更新了文档。</span>
    <button class="ghost small" id="take-remote">载入远端</button>
    <button class="ghost small" id="keep-local">保留本地</button>`;
  $('take-remote').onclick = () => {
    state.suppress = true;
    $('editor').value = msg.content || '';
    state.suppress = false;
    state.rev = msg.rev;
    state.sent = msg.content || '';
    state.dirty = false;
    state.pendingRemote = null;
    b.hidden = true;
    setStatus('已同步', 'live');
    updateCount();
  };
  $('keep-local').onclick = () => {
    b.hidden = true;
    state.pendingRemote = null;
  };
}

function showConflict(msg) {
  const b = $('banner');
  b.hidden = false;
  const canMerge = !!msg.merged;
  b.innerHTML = `<span>版本冲突：你的修改基于旧版本。</span>
    <button class="ghost small" id="take-latest">载入最新</button>
    ${canMerge ? '<button class="ghost small" id="take-merged">采用合并稿</button>' : ''}
    <button class="ghost small" id="keep-mine">覆盖远端</button>`;
  setStatus('版本冲突', 'dirty');
  $('take-latest').onclick = () => {
    state.suppress = true;
    $('editor').value = msg.content || '';
    state.suppress = false;
    state.rev = msg.rev;
    state.sent = msg.content || '';
    state.dirty = false;
    b.hidden = true;
    setStatus('已同步', 'live');
    updateCount();
  };
  if (canMerge) $('take-merged').onclick = () => {
    state.suppress = true;
    $('editor').value = msg.merged;
    state.suppress = false;
    state.rev = msg.rev;
    state.sent = msg.content || '';
    state.dirty = true;
    b.hidden = true;
    scheduleSave(80);
  };
  $('keep-mine').onclick = () => {
    // The reader explicitly chose their text over the server copy. Rebase
    // onto the current revision first so this is a deliberate overwrite, not
    // another stale-base conflict.
    state.rev = msg.rev;
    state.sent = msg.content || '';
    state.dirty = true;
    b.hidden = true;
    scheduleSave(80);
  };
}

function scheduleSave(delay = 650) {
  state.dirty = true;
  setStatus('未保存', 'dirty');
  clearTimeout(state.saveTimer);
  state.saveTimer = setTimeout(sendSave, delay);
}

function sendSave() {
  if (!state.connected || !state.ws) return;
  if (state.inFlight) { state.queued = true; return; }
  const content = $('editor').value;
  if (content === state.sent) return;
  state.inFlight = true;
  state.sent = content;
  state.ws.send(JSON.stringify({type: 'save', base: state.rev, content}));
  setStatus('保存中…');
}

function sendCursor() {
  if (!state.connected || !state.ws) return;
  state.ws.send(JSON.stringify({type: 'cursor', cursor: $('editor').selectionStart || 0}));
}

$('editor').addEventListener('compositionstart', () => { state.composing = true; });
$('editor').addEventListener('compositionend', () => {
  state.composing = false;
  if (!state.suppress) {
    updateCount();
    scheduleSave();
  }
});
$('editor').addEventListener('input', () => {
  if (state.suppress) return;
  updateCount();
  scheduleSave();
});
let cursorTimer = 0;
$('editor').addEventListener('select', () => {
  clearTimeout(cursorTimer);
  cursorTimer = setTimeout(sendCursor, 250);
});
$('editor').addEventListener('keydown', (e) => {
  if (e.key === 'Tab') {
    e.preventDefault();
    const el = $('editor');
    const start = el.selectionStart, end = el.selectionEnd;
    el.value = el.value.slice(0, start) + '  ' + el.value.slice(end);
    el.selectionStart = el.selectionEnd = start + 2;
    el.dispatchEvent(new Event('input'));
  }
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
    e.preventDefault();
    clearTimeout(state.saveTimer);
    sendSave();
  }
});

$('user-name').addEventListener('change', () => {
  const name = $('user-name').value.trim() || 'Reader';
  state.name = name;
  localStorage.setItem('mx-server-name', name);
  if (state.doc) {
    closeSocket();
    connect(state.doc.id);
  }
});

$('doc-filter').addEventListener('input', renderDocs);
$('doc-refresh').onclick = loadDocs;
$('history-toggle').onclick = () => {
  const panel = $('history');
  panel.hidden = !panel.hidden;
  document.querySelector('.layout').classList.toggle('history-open', !panel.hidden);
  if (!panel.hidden) loadHistory();
};
$('history-close').onclick = () => {
  $('history').hidden = true;
  document.querySelector('.layout').classList.remove('history-open');
};
$('token-open').onclick = openToken;
$('token-save').onclick = (e) => {
  e.preventDefault();
  state.token = $('token-input').value.trim();
  if (state.token) localStorage.setItem('mx-server-token', state.token);
  else localStorage.removeItem('mx-server-token');
  $('token-dialog').close();
  if (state.doc) { closeSocket(); connect(state.doc.id); }
  loadDocs();
};

$('new-doc').onclick = async () => {
  const title = prompt('新文档标题');
  if (title === null) return;
  const folder = prompt('文件夹（可留空，如 design/game）', state.doc ? state.doc.id.split('/').slice(0, -1).join('/') : '');
  if (folder === null) return;
  const template = prompt('模板：game-design / level-design / narrative / product-prd / film-treatment / content-article / meeting-notes（可留空）', '');
  if (template === null) return;
  try {
    const data = await api('/api/docs', {method: 'POST', body: JSON.stringify({title, id: '', folder, template})});
    await loadDocs();
    await openDoc(data.id);
  } catch (err) {
    if (err.message !== 'unauthorized') toast(err.message);
  }
};

async function loadHistory() {
  if (!state.doc || !state.doc.id) return;
  try {
    const data = await api(`/api/history?id=${encodeURIComponent(state.doc.id)}&limit=60`);
    state.commits = data.commits || [];
    renderHistory();
  } catch (err) {
    if (err.message !== 'unauthorized') toast(err.message);
  }
}

function renderHistory() {
  const list = $('commit-list');
  list.innerHTML = '';
  if (!state.commits.length) {
    list.innerHTML = '<p class="muted" style="padding:10px;font-size:12px">还没有 Git 提交。</p>';
    return;
  }
  for (const c of state.commits) {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'commit' + (state.selected === c.hash ? ' active' : '');
    const when = c.time ? new Date(c.time).toLocaleString() : '';
    b.innerHTML = `<strong>${escapeHTML(c.subject || '(no subject)')}</strong><span>${escapeHTML(c.hash.slice(0, 8))} · ${escapeHTML(c.author || '')} · ${escapeHTML(when)}</span>`;
    b.onclick = () => selectCommit(c);
    list.appendChild(b);
  }
}

async function selectCommit(c) {
  state.selected = c.hash;
  renderHistory();
  $('diff-title').textContent = c.hash.slice(0, 8);
  $('restore').disabled = false;
  try {
    const data = await api(`/api/diff?id=${encodeURIComponent(state.doc.id)}&to=${encodeURIComponent(c.hash)}`);
    $('diff-body').innerHTML = colorDiff(data.diff || '(empty diff)');
  } catch (err) {
    $('diff-body').textContent = err.message;
  }
}

function colorDiff(text) {
  return text.split('\n').map((line) => {
    if (line.startsWith('+') && !line.startsWith('+++')) return `<span class="diff-add">${escapeHTML(line)}</span>`;
    if (line.startsWith('-') && !line.startsWith('---')) return `<span class="diff-del">${escapeHTML(line)}</span>`;
    return escapeHTML(line);
  }).join('\n');
}

$('restore').onclick = async () => {
  if (!state.selected || !state.doc) return;
  if (!confirm('把当前文档恢复到这次提交？这会生成一个新的 Git 提交。')) return;
  try {
    await api(`/api/restore?id=${encodeURIComponent(state.doc.id)}`, {
      method: 'POST', body: JSON.stringify({rev: state.selected})
    });
    toast('已恢复；等待房间同步');
    state.dirty = false;
  } catch (err) {
    if (err.message !== 'unauthorized') toast(err.message);
  }
};

window.addEventListener('beforeunload', (e) => {
  if (state.dirty) { e.preventDefault(); e.returnValue = ''; }
});

(async function boot() {
  if (!state.token) setStatus('只读/需令牌');
  await loadDocs();
  const hash = new URLSearchParams(location.hash.slice(1));
  const wanted = hash.get('doc');
  if (wanted) await openDoc(wanted);
  else if (state.docs.length) await openDoc(state.docs[0].id);
})();
