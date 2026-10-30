import { AudioSession } from './audio.js';
import { createAuth } from './auth.js';
import { t, translate } from './i18n.js';

const $ = id => document.getElementById(id);
const year = String(new Date().getFullYear());
$('current-year').textContent = year;
$('current-year').dateTime = year;
const form = $('settings-form');
const defaults = { provider:'gemini', language:'Spanish', level:'Intermediate', voice:'Aoede', pace:'normal', input_mode:'voice', shortcut:'Space', preferences:'', auto_open_log:false, ui_language:'en', theme:'light' };
const languages = [['English','en'],['Spanish','es'],['Portuguese','pt'],['Portuguese (Brazil)','pt-BR'],['Portuguese (Portugal)','pt-PT'],['French','fr'],['German','de'],['Italian','it'],['Japanese','ja'],['Korean','ko'],['Mandarin Chinese','zh'],['Hindi','hi'],['Arabic','ar'],['Dutch','nl'],['Polish','pl'],['Russian','ru'],['Turkish','tr'],['Indonesian','id'],['Vietnamese','vi'],['Thai','th']];
const voices = { gemini:['Aoede','Puck','Kore','Zephyr','Charon','Fenrir','Leda','Orus','Callirrhoe','Autonoe','Enceladus','Iapetus','Umbriel','Algieba','Despina','Erinome','Algenib','Rasalgethi','Laomedeia','Achernar','Alnilam','Schedar','Gacrux','Pulcherrima','Achird','Zubenelgenubi','Vindemiatrix','Sadachbia','Sadaltager','Sulafat'], openai:['alloy','ash','ballad','coral','echo','sage','shimmer','verse','marin','cedar'] };
const feminine = new Set(['Achernar','Aoede','Autonoe','Callirrhoe','Despina','Erinome','Gacrux','Kore','Laomedeia','Leda','Pulcherrima','Sulafat','Vindemiatrix','Zephyr','alloy','coral','sage','shimmer','marin']);
const providerErrors = new Set(['providerInvalidKey','providerQuota','providerModelUnavailable','providerAccessDenied','providerSetupRejected','providerDisconnected','providerConnectionFailed']);
const voiceTones = {
  en:['Breezy','Upbeat','Firm','Bright','Informative','Excitable','Youthful','Firm','Easy-going','Bright','Breathy','Clear','Easy-going','Smooth','Smooth','Clear','Gravelly','Informative','Upbeat','Soft','Firm','Even','Mature','Forward','Friendly','Casual','Gentle','Lively','Knowledgeable','Warm'],
  es:['Ligera','Animada','Firme','Brillante','Informativa','Entusiasta','Juvenil','Firme','Relajada','Brillante','Aireada','Clara','Relajada','Suave','Suave','Clara','Rasposa','Informativa','Animada','Suave','Firme','Uniforme','Madura','Directa','Amigable','Informal','Gentil','Vivaz','Experta','Cálida'],
  pt:['Leve','Animada','Firme','Brilhante','Informativa','Entusiasmada','Jovem','Firme','Descontraída','Brilhante','Soprosa','Clara','Descontraída','Suave','Suave','Clara','Áspera','Informativa','Animada','Macia','Firme','Equilibrada','Madura','Direta','Amigável','Casual','Gentil','Viva','Experiente','Acolhedora'],
};
let settings = { ...defaults }, user = null, mode = '', chatList = [], selectedLog = null, audio = null, timer = null, startTime = 0, minimal = false;
let lastMessage = null, activeChat = '', soundTimer = null, identityEpoch = 0, chatSequence = 0, logSequence = 0, selectionRevision = 0, searchTimer = null, moreChats = false, chatRevision = 0;
let shortcutHeld = false;
let authUi;
const liveMessagesById = new Map();
const text = key => t(settings.ui_language, key);

async function api(path, options = {}) {
  const response = await fetch(new URL(`.${path}`, document.baseURI), { ...options, headers: { ...(options.body ? { 'Content-Type':'application/json' } : {}), ...options.headers } });
  const result = await response.json().catch(() => ({}));
  if (!response.ok) { const error = new Error(result.error || `Request failed (${response.status}).`); error.status = response.status; throw error; }
  return result;
}
function notify(message) {
  const notice = $('notice'); notice.textContent = message; notice.hidden = false;
  clearTimeout(notice.timer); notice.timer = setTimeout(() => { notice.hidden = true; }, 4500);
}
function storageKey() { return `speeching-web:${mode}:${user.name}`; }
function loadSettings() {
  try { settings = { ...defaults, ...JSON.parse(localStorage.getItem(storageKey()) || '{}') }; } catch { settings = { ...defaults }; }
  if (!['en','es','pt'].includes(settings.ui_language)) settings.ui_language = 'en';
}
function saveSettings() { try { localStorage.setItem(storageKey(), JSON.stringify(settings)); } catch { notify('Browser settings could not be saved.'); } }
function displayLanguage(name) {
  const code = languages.find(item => item[0] === name)?.[1];
  try { return new Intl.DisplayNames([settings.ui_language], { type:'language' }).of(code) || name; } catch { return name; }
}
function populateLanguages() {
  $('language').replaceChildren(...languages.map(([name]) => new Option(displayLanguage(name), name)));
  $('language').value = settings.language;
  $('practice-language').textContent = displayLanguage(settings.language);
}
function populateVoices(provider, selected) {
  const labels = { en:['Feminine','Masculine'], es:['Femenina','Masculina'], pt:['Feminina','Masculina'] }[settings.ui_language];
  $('voice').replaceChildren(...voices[provider].map((name, index) => {
    const fem = feminine.has(name);
    const tone = provider === 'gemini' ? voiceTones[settings.ui_language][index] : ({en:'Tone varies',es:'Tono variable',pt:'Tom variável'}[settings.ui_language]);
    return new Option(`${fem ? '♀' : '♂'} ${labels[fem ? 0 : 1]} · ${tone} · ${String(index + 1).padStart(2,'0')}`, name);
  }));
  $('voice').value = voices[provider].includes(selected) ? selected : voices[provider][0];
}
function updateKeyStatus() { $('key-status').textContent = text(user.keys[$('provider').value] ? 'keySaved' : 'keyMissing'); }
function applySettings() {
  document.documentElement.dataset.theme = settings.theme;
  translate(settings.ui_language); populateLanguages();
  for (const [key, value] of Object.entries(settings)) {
    const field = form.elements[key]; if (!field || key === 'voice') continue;
    if (field.type === 'checkbox') field.checked = Boolean(value); else field.value = value;
  }
  populateVoices(settings.provider, settings.voice);
  $('shortcut-field').hidden = settings.input_mode !== 'push';
  updateKeyStatus();
  $('minimal').textContent = text(minimal ? 'showText' : 'hideText');
  $('logout').hidden = mode !== 'builtin';
  $('history-toggle').setAttribute('aria-label', text('history'));
  $('settings-toggle').setAttribute('aria-label', text('settings'));
  $('mic').setAttribute('aria-label', text(audio ? 'end' : 'start'));
  $('mic').title = text(audio ? 'end' : 'start');
  translateMessageLabels();
}
function translateMessageLabels() {
  for (const message of document.querySelectorAll('.message')) {
    const label = message.querySelector('b');
    if (label) label.textContent = text(message.classList.contains('user') ? 'you' : 'partner');
  }
}
function setPanel(name, open) {
  for (const panel of ['history','settings']) {
    const isTarget = panel === name && open;
    $(`${panel}-panel`).hidden = !isTarget;
    $(`${panel}-toggle`).setAttribute('aria-expanded', String(isTarget));
  }
  if (!open) { $(`${name}-toggle`).focus(); return; }
  if (name === 'history') refreshChats();
  const panel = $(`${name}-panel`);
  if (innerWidth <= 800) panel.scrollIntoView({ behavior:'auto', block:'start' });
  $(`${name}-title`).focus({ preventScroll:true });
}
function togglePanel(name) { setPanel(name, $(`${name}-panel`).hidden); }
function timestamp(seconds) { return new Intl.DateTimeFormat(settings.ui_language, { dateStyle:'medium', timeStyle:'short' }).format(new Date(seconds * 1000)); }
function duration(seconds) { const minutes = Math.floor(seconds / 60); return `${String(minutes).padStart(2,'0')}:${String(seconds % 60).padStart(2,'0')}`; }
function renderPicker() {
  const current = $('chat-select').value;
  const previous = $('chat-select').selectedOptions[0]?.textContent;
  const options = chatList.map(chat => new Option(`${chat.title} · ${timestamp(chat.updated_at)}`, chat.id));
  if (current && !chatList.some(chat => chat.id === current)) options.push(new Option(previous, current));
  $('chat-select').replaceChildren(new Option(text('newConversation'), ''), ...options);
  $('chat-select').value = current;
}
function renderChats() {
  const rows = chatList;
  const list = $('chat-list'); list.replaceChildren();
  $('more-chats').hidden = !moreChats;
  if (!rows.length) { const p = document.createElement('p'); p.className = 'empty'; p.textContent = text($('search').value.trim() ? 'noResults' : 'noChats'); list.append(p); return; }
  for (const chat of rows) {
    const row = document.createElement('div'); row.className = 'chat-row';
    const button = document.createElement('button'); button.type = 'button'; button.classList.toggle('selected', selectedLog?.id === chat.id);
    const title = document.createElement('strong'); title.textContent = chat.title;
    const meta = document.createElement('span'); meta.className = 'meta'; meta.textContent = `${displayLanguage(chat.language)} · ${timestamp(chat.updated_at)} · ${duration(chat.duration_seconds)}`;
    button.append(title, meta); button.addEventListener('click', () => selectConversation(chat.id, true)); row.append(button); list.append(row);
  }
}
async function refreshChats(append = false) {
  const epoch = identityEpoch, sequence = ++chatSequence;
  const query = new URLSearchParams({ offset:String(append ? chatList.length : 0), search:$('search').value.trim(), sort:$('sort').value });
  if (append) query.set('revision', String(chatRevision));
  try { const result = await api(`/api/chats?${query}`); if (epoch !== identityEpoch || sequence !== chatSequence) return; chatList = append ? [...chatList, ...result.chats] : result.chats; moreChats = result.more; chatRevision = result.revision; renderPicker(); renderChats(); }
  catch (error) { if (epoch !== identityEpoch || sequence !== chatSequence) return; if (error.status === 409) { await refreshChats(); notify(text('historyChanged')); } else notify(error.message); }
}
function messageNode(message) {
  const node = document.createElement('div'); node.className = `message ${message.speaker === 'user' ? 'user' : ''}`;
  const label = document.createElement('b'); label.textContent = text(message.speaker === 'user' ? 'you' : 'partner');
  const body = document.createElement('span'); body.textContent = message.text;
  node.append(label, body); return node;
}
function clearLog() {
  logSequence++; selectedLog = null; $('log').hidden = true;
  $('log-title').textContent = ''; $('log-duration').textContent = ''; $('log-messages').replaceChildren();
  $('more-messages').hidden = true; $('more-messages').disabled = false; renderChats();
}
function resetLiveMessages() {
  liveMessagesById.clear(); lastMessage = null;
  const empty = document.createElement('p'); empty.className = 'empty'; empty.dataset.i18n = 'transcriptEmpty'; empty.textContent = text('transcriptEmpty');
  $('live-messages').replaceChildren(empty);
}
function selectConversation(id, reveal = false) {
  selectionRevision++;
  if (!audio) { $('chat-select').value = id; activeChat = ''; resetLiveMessages(); }
  if (id) showLog({ id }, reveal); else clearLog();
}
async function showLog(chat, reveal = false) {
  if (!chat) return;
  clearLog();
  const epoch = identityEpoch, sequence = logSequence;
  try {
    const result = await api(`/api/chats/${encodeURIComponent(chat.id)}`);
    if (epoch !== identityEpoch || sequence !== logSequence) return;
    selectedLog = { ...result.chat, messages:result.messages, more:result.more, revision:result.revision }; $('log').hidden = minimal;
    $('log-title').textContent = selectedLog.title; $('log-duration').textContent = duration(selectedLog.duration_seconds);
    $('log-messages').replaceChildren(...result.messages.filter(message => message.text.trim()).map(messageNode));
    $('more-messages').hidden = !result.more; $('delete').disabled = Boolean(audio); renderChats();
    if (reveal && !minimal) {
      $('log').scrollIntoView({ behavior:'auto', block:innerWidth <= 800 ? 'start' : 'nearest' });
      $('log-title').focus({ preventScroll:true });
    }
  } catch (error) { if (epoch === identityEpoch && sequence === logSequence) notify(error.message); }
}
async function completeLog() {
  const epoch = identityEpoch, current = selectedLog;
  let messages = [...current.messages], more = current.more;
  while (more) {
    const result = await api(`/api/chats/${encodeURIComponent(current.id)}?offset=${messages.length}&revision=${current.revision}`);
    if (epoch !== identityEpoch || selectedLog !== current) throw new Error('Conversation changed.');
    messages = [...result.messages, ...messages]; more = result.more;
  }
  const heading = `${current.title}\n${timestamp(current.updated_at)} · ${duration(current.duration_seconds)}`;
  return `${heading}\n\n${messages.filter(message => message.text.trim()).map(message => `${text(message.speaker === 'user' ? 'you' : 'partner')}: ${message.text}`).join('\n\n')}`;
}
async function loadEarlier() {
  const current = selectedLog, epoch = identityEpoch;
  if (!current || $('more-messages').disabled) return;
  $('more-messages').disabled = true;
  try {
    const page = await api(`/api/chats/${encodeURIComponent(current.id)}?offset=${current.messages.length}&revision=${current.revision}`);
    if (epoch !== identityEpoch || selectedLog !== current) return;
    current.messages = [...page.messages, ...current.messages]; current.more = page.more;
    $('log-messages').prepend(...page.messages.filter(message => message.text.trim()).map(messageNode)); $('more-messages').hidden = !page.more;
  } catch (error) { if (epoch !== identityEpoch || selectedLog !== current) return; if (error.status === 409) { await showLog(current); notify(text('historyChanged')); } else notify(error.message); }
  finally { if (selectedLog === current) $('more-messages').disabled = false; }
}
async function exportError(error, fallback) {
  if (error.status === 409 && selectedLog) { await showLog(selectedLog); notify(text('historyChanged')); return; }
  notify(text(fallback));
}
function renderLive(event) {
  const list = $('live-messages');
  if (list.querySelector('.empty')) list.replaceChildren();
  const key = event.id ? `${event.speaker}:${event.id}` : null;
  const current = key ? liveMessagesById.get(key) : lastMessage?.speaker === event.speaker && !lastMessage.id ? lastMessage.node : null;
  if (!current) {
    const node = messageNode(event); list.append(node); lastMessage = { speaker:event.speaker, id:event.id, node };
    if (key) liveMessagesById.set(key, node);
  } else { const body = current.querySelector('span'); body.textContent = event.replace ? event.text : body.textContent + event.text; }
  list.scrollTop = list.scrollHeight;
}
function setStatus(key) { $('status').textContent = text(key); }
function setTalk(active) {
  $('hold').classList.toggle('pressed', active);
  $('hold').setAttribute('aria-pressed', String(active));
  audio?.press(active);
}
function shortcutName(event) {
  const parts = [];
  if (event.ctrlKey) parts.push('Ctrl'); if (event.altKey) parts.push('Alt');
  if (event.shiftKey) parts.push('Shift'); if (event.metaKey) parts.push('Meta');
  parts.push(event.code); return parts.join('+');
}
function pulse() { $('wave').classList.add('active'); clearTimeout(soundTimer); soundTimer = setTimeout(() => { if (!audio?.sources.size) $('wave').classList.remove('active'); }, 450); }
function liveMessage(event) {
  if (event.type === 'ready') { $('hold').disabled = false; setStatus('listening'); startTime = Date.now(); timer = setInterval(() => { $('timer').textContent = duration(Math.floor((Date.now() - startTime) / 1000)); }, 1000); }
  if (event.type === 'transcript') renderLive(event);
  if (event.type === 'user_item') renderLive({ speaker:'user', id:event.id, text:'', replace:false });
  if (event.type === 'chat') activeChat = event.id;
  if (event.type === 'error') notify(providerErrors.has(event.code) ? text(event.code) : event.message);
}
function closed() {
  const epoch = identityEpoch, endedChat = activeChat, selectionAtClose = selectionRevision;
  const refreshVisibleLog = Boolean(endedChat && selectedLog?.id === endedChat);
  logSequence++;
  if (endedChat && selectedLog && !refreshVisibleLog) clearLog();
  setTalk(false);
  audio = null; clearInterval(timer); $('mic').classList.remove('active'); $('mic').setAttribute('aria-label', text('start')); $('hint').textContent = text('startHint'); $('hold').hidden = true;
  $('chat-select').disabled = false; $('delete').disabled = false; setStatus('finished');
  refreshChats().then(() => {
    if (epoch !== identityEpoch || selectionAtClose !== selectionRevision || !endedChat || audio) return;
    if (![...$('chat-select').options].some(option => option.value === endedChat)) $('chat-select').add(new Option(text('currentConversation'), endedChat));
    $('chat-select').value = endedChat;
    if (settings.auto_open_log) { setPanel('history', true); showLog({ id:endedChat }); }
    else if (refreshVisibleLog) showLog({ id:endedChat });
  });
}
async function toggleMic() {
  if (audio) { audio.stop(); return; }
  if (!user.keys[settings.provider]) {
    notify(text('missingKey')); setPanel('settings', true);
    form.elements.provider.value = settings.provider; populateVoices(settings.provider, settings.voice); updateKeyStatus();
    $('api-key').focus({ preventScroll:true }); $('api-key').scrollIntoView({ behavior:'auto', block:'center' });
    return;
  }
  $('mic').classList.add('active'); $('mic').setAttribute('aria-label', text('end')); $('hint').textContent = text('stopHint'); $('chat-select').disabled = true;
  $('hold').disabled = true;
  $('live-messages').replaceChildren(); liveMessagesById.clear(); lastMessage = null; activeChat = ''; $('timer').textContent = '00:00'; setStatus('connecting');
  const epoch = identityEpoch;
  const session = new AudioSession({ message:event => { if (epoch === identityEpoch) liveMessage(event); }, closed:() => { if (audio === session && epoch === identityEpoch) closed(); }, error:message => { if (epoch === identityEpoch) notify(message); }, level:rms => { if (epoch === identityEpoch && rms > .008) pulse(); }, speaking:() => { if (epoch === identityEpoch) { setStatus('speaking'); pulse(); } }, listening:() => { if (epoch === identityEpoch && audio === session) { setStatus('listening'); $('wave').classList.remove('active'); } } });
  audio = session; $('delete').disabled = true;
  try { await session.start({ ...settings, chat_id:$('chat-select').value || null }); $('hold').hidden = settings.input_mode !== 'push'; }
  catch { session.cleanup(); if (audio === session) closed(); if (!session.cancelled) notify(text('micFailed')); }
}
async function submitSettings(event) {
  event.preventDefault();
  const epoch = identityEpoch;
  const proposed = Object.fromEntries(new FormData(form));
  proposed.auto_open_log = form.elements.auto_open_log.checked;
  const key = $('api-key').value.trim();
  try {
    if (key) { await api(`/api/keys/${proposed.provider}`, { method:'POST', body:JSON.stringify({ key }) }); if (epoch !== identityEpoch) return; user.keys[proposed.provider] = true; $('api-key').value = ''; }
    if (epoch !== identityEpoch) return;
    settings = { ...defaults, ...proposed }; saveSettings(); applySettings(); renderPicker(); renderChats(); notify(text('saved'));
  } catch (error) { if (epoch === identityEpoch) notify(error.message); }
}
async function initialize() {
  const epoch = ++identityEpoch;
  const authMode = await api('/api/mode');
  const nextMode = authMode.auth;
  if (epoch !== identityEpoch) return;
  mode = nextMode;
  try { const nextUser = await api('/api/me'); if (epoch !== identityEpoch) return; user = nextUser; }
  catch { if (epoch !== identityEpoch) return; $('workspace').hidden = true; $('workspace-tools').hidden = true; $('login-view').hidden = mode !== 'builtin'; if (mode === 'builtin') authUi.showLogin(authMode.registration); else notify(text('accessDenied')); return; }
  loadSettings(); applySettings(); $('login-view').hidden = true; $('workspace').hidden = false; $('workspace-tools').hidden = false; await refreshChats();
}

function bindEvents() {
  authUi = createAuth({ api, text, onSignIn:initialize });
  $('settings-form').addEventListener('submit', submitSettings);
  $('history-toggle').addEventListener('click', () => togglePanel('history'));
  $('settings-toggle').addEventListener('click', () => togglePanel('settings'));
  document.querySelectorAll('[data-close]').forEach(button => button.addEventListener('click', () => togglePanel(button.dataset.close)));
  $('provider').addEventListener('change', () => { populateVoices($('provider').value, ''); updateKeyStatus(); $('api-key').value = ''; });
  form.elements.input_mode.addEventListener('change', () => { $('shortcut-field').hidden = form.elements.input_mode.value !== 'push'; });
  $('shortcut').addEventListener('keydown', event => {
    event.preventDefault(); event.stopPropagation();
    if (!event.code || ['ControlLeft','ControlRight','ShiftLeft','ShiftRight','AltLeft','AltRight','MetaLeft','MetaRight'].includes(event.code)) return;
    $('shortcut').value = shortcutName(event);
  });
  $('mic').addEventListener('click', toggleMic);
  $('chat-select').addEventListener('change', () => selectConversation($('chat-select').value));
  $('hold').addEventListener('pointerdown', event => { event.preventDefault(); $('hold').setPointerCapture(event.pointerId); setTalk(true); });
  for (const kind of ['pointerup','pointercancel','lostpointercapture']) $('hold').addEventListener(kind, () => setTalk(false));
  $('hold').addEventListener('keydown', event => {
    if (![' ', 'Enter'].includes(event.key) || event.repeat) return;
    event.preventDefault(); setTalk(true);
  });
  $('hold').addEventListener('keyup', event => {
    if (![' ', 'Enter'].includes(event.key)) return;
    event.preventDefault(); setTalk(false);
  });
  $('hold').addEventListener('blur', () => setTalk(false));
  $('hold').addEventListener('click', event => { if (event.detail === 0) setTalk($('hold').getAttribute('aria-pressed') !== 'true'); });
  document.addEventListener('keydown', event => {
    if (!audio?.ready || settings.input_mode !== 'push' || event.repeat || /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement.tagName)) return;
    if (shortcutName(event) !== settings.shortcut) return;
    event.preventDefault(); shortcutHeld = true; setTalk(true);
  });
  document.addEventListener('keyup', event => {
    if (!shortcutHeld) return;
    if (event.code !== settings.shortcut.split('+').at(-1) && !/^(Control|Shift|Alt|Meta)/.test(event.code)) return;
    event.preventDefault(); shortcutHeld = false; setTalk(false);
  });
  window.addEventListener('blur', () => { if (shortcutHeld) { shortcutHeld = false; setTalk(false); } });
  $('minimal').addEventListener('click', () => { minimal = !minimal; $('live-messages').hidden = minimal; $('log').hidden = minimal || !selectedLog; $('minimal').textContent = text(minimal ? 'showText' : 'hideText'); });
  $('search').addEventListener('input', () => { clearTimeout(searchTimer); searchTimer = setTimeout(() => refreshChats(), 200); });
  $('sort').addEventListener('change', () => refreshChats());
  $('more-chats').addEventListener('click', () => refreshChats(true)); $('more-messages').addEventListener('click', loadEarlier);
  $('copy').addEventListener('click', async () => { try { await navigator.clipboard.writeText(await completeLog()); notify(text('copied')); } catch (error) { await exportError(error, 'copyFailed'); } });
  $('download').addEventListener('click', async () => { try { const url = URL.createObjectURL(new Blob([await completeLog()], { type:'text/plain;charset=utf-8' })); const link = document.createElement('a'); link.href = url; link.download = 'speeching-conversation.txt'; link.click(); URL.revokeObjectURL(url); } catch (error) { await exportError(error, 'downloadFailed'); } });
  $('rename').addEventListener('click', async () => {
    const current = selectedLog, epoch = identityEpoch, title = prompt(text('renamePrompt'), current.title); if (!title) return;
    try { await api(`/api/chats/${encodeURIComponent(current.id)}`, { method:'PATCH', body:JSON.stringify({ title }) }); if (epoch !== identityEpoch || selectedLog !== current) return; await refreshChats(); if (epoch === identityEpoch && selectedLog === current) await showLog(chatList.find(chat => chat.id === current.id)); } catch (error) { if (epoch === identityEpoch) notify(error.message); }
  });
  $('delete').addEventListener('click', async () => {
    const current = selectedLog, epoch = identityEpoch; if (!confirm(text('deleteConfirm'))) return;
    try { await api(`/api/chats/${encodeURIComponent(current.id)}`, { method:'DELETE' }); if (epoch !== identityEpoch || selectedLog !== current) return; if ($('chat-select').value === current.id) { selectionRevision++; $('chat-select').value = ''; activeChat = ''; resetLiveMessages(); } clearLog(); await refreshChats(); } catch (error) { if (epoch === identityEpoch) notify(error.message); }
  });
  $('logout').addEventListener('click', async () => {
    identityEpoch++; selectionRevision++; logSequence++;
    if (audio) { audio.stop(); audio = null; }
    clearInterval(timer); clearTimeout(soundTimer); $('wave').classList.remove('active'); $('mic').classList.remove('active'); $('hold').hidden = true; $('chat-select').disabled = false;
    try {
      await api('/api/logout', { method:'POST' });
      user = null; selectedLog = null; chatList = []; activeChat = ''; lastMessage = null; liveMessagesById.clear();
      $('log').hidden = true; $('log-messages').replaceChildren(); $('live-messages').replaceChildren(); $('api-key').value = '';
      await initialize();
    } catch (error) { notify(error.message); }
  });
}
bindEvents();
initialize().catch(error => notify(error.message));
