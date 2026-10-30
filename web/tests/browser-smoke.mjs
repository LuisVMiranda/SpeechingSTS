// Optional developer test: Node 24+ and a local Chromium browser are required.
// Production serves embedded assets directly from the Rust executable.
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';

const project = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const temp = mkdtempSync(path.join(os.tmpdir(), 'speeching-browser-'));
const executable = process.env.SPEECHING_TEST_EXE || path.join(project, 'target', 'release', process.platform === 'win32' ? 'speeching-web.exe' : 'speeching-web');
const chromePath = process.env.CHROME || (process.platform === 'win32' ? 'C:/Program Files/Google/Chrome/Application/chrome.exe' : process.platform === 'darwin' ? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' : 'google-chrome');
const screenshots = process.env.SPEECHING_SCREENSHOT_DIR;
let chrome, service, devtools, proxy, mock, audioChunks = 0, preferenceSeen = false, rejectNextSetup = false;
const historyDelays = new Map();
let chatListDelay = 0;
const connections = new Set();

function track(socket) { connections.add(socket); socket.on('close', () => connections.delete(socket)); socket.on('error', () => socket.destroy()); }
function listen(server) { return new Promise(resolve => server.listen(0, '127.0.0.1', () => resolve(server.address().port))); }
async function freePort() { const server = net.createServer(); const port = await listen(server); await new Promise(resolve => server.close(resolve)); return port; }
function sendFrame(socket, value, binary = false) {
  const data = Buffer.from(JSON.stringify(value));
  const opcode = binary ? 0x82 : 0x81;
  const header = data.length < 126 ? Buffer.from([opcode, data.length]) : Buffer.from([opcode,126,data.length >> 8,data.length & 255]);
  socket.write(Buffer.concat([header, data]));
}
function frameSize(buffer) {
  if (buffer.length < 2) return null;
  const length = buffer[1] & 127;
  if (length === 126) { if (buffer.length < 4) return null; return { length:buffer.readUInt16BE(2), offset:4 }; }
  if (length === 127) { if (buffer.length < 10) return null; return { length:Number(buffer.readBigUInt64BE(2)), offset:10 }; }
  return { length, offset:2 };
}
function decodeFrame(buffer) {
  const size = frameSize(buffer); if (!size) return null;
  const masked = (buffer[1] & 128) !== 0, maskSize = masked ? 4 : 0;
  const offset = size.offset + maskSize;
  if (buffer.length < offset + size.length) return null;
  const mask = masked ? buffer.subarray(size.offset, offset) : null;
  const payload = Buffer.from(buffer.subarray(offset, offset + size.length));
  if (mask) for (let index = 0; index < payload.length; index++) payload[index] ^= mask[index % 4];
  return { opcode:buffer[0] & 15, payload, rest:buffer.subarray(offset + size.length) };
}
function consumeFrames(socket, onMessage) {
  let buffer = Buffer.alloc(0);
  socket.on('data', data => {
    buffer = Buffer.concat([buffer, data]);
    while (buffer.length) {
      const frame = decodeFrame(buffer); if (!frame) return;
      buffer = frame.rest;
      if (frame.opcode === 8) { socket.end(); return; }
      if (frame.opcode === 1) onMessage(JSON.parse(frame.payload.toString()));
    }
  });
}
function createMock() {
  const server = http.createServer();
  server.on('upgrade', (request, socket) => {
    track(socket);
    const accept = createHash('sha1').update(request.headers['sec-websocket-key'] + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
    socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`);
    let replied = false;
    consumeFrames(socket, value => {
      if (value.setup) {
        if (rejectNextSetup) { rejectNextSetup = false; sendFrame(socket, { error:{message:'API key not valid. synthetic-secret-marker'} }, true); }
        else sendFrame(socket, { setupComplete:{} }, true);
      }
      if (value.clientContent && JSON.stringify(value).includes('Discuss astronomy')) preferenceSeen = true;
      if (!value.realtimeInput?.audio) return;
      audioChunks++;
      if (replied) return;
      replied = true;
      sendFrame(socket, { serverContent:{ inputTranscription:{text:'Synthetic learner speech.'}, outputTranscription:{text:'Hello from the mock provider <img src=x onerror=alert(1)>'}, modelTurn:{parts:[{inlineData:{data:'AAE='}}]}, turnComplete:true } }, true);
    });
  });
  return server;
}
function createProxy(servicePort) {
  const server = http.createServer((request, response) => {
    const suffix = request.url.startsWith('/speeching/') ? request.url.slice('/speeching'.length) : request.url;
    const upstream = http.request({ hostname:'127.0.0.1', port:servicePort, path:suffix, method:request.method, headers:request.headers }, result => {
      const route = suffix.split('?')[0];
      const id = route.startsWith('/api/chats/') ? decodeURIComponent(route.slice('/api/chats/'.length)) : '';
      const delayMs = request.method === 'GET' ? (id ? historyDelays.get(id) || 0 : route === '/api/chats' ? chatListDelay : 0) : 0;
      const send = () => { if (response.destroyed) { result.resume(); return; } response.writeHead(result.statusCode, result.headers); result.pipe(response); };
      if (delayMs) setTimeout(send, delayMs); else send();
    });
    upstream.on('error', () => response.writeHead(502).end());
    // Exercise a slower key save rather than relying on a fast local round trip.
    if (request.method === 'POST' && suffix.startsWith('/api/keys/')) setTimeout(() => request.pipe(upstream), 350);
    else request.pipe(upstream);
  });
  server.on('upgrade', (request, socket, head) => {
    const upstream = net.connect(servicePort, '127.0.0.1', () => {
      const headers = request.rawHeaders.reduce((all, value, index) => all + (index % 2 ? `${value}\r\n` : `${value}: `), '');
      upstream.write(`${request.method} ${request.url.slice('/speeching'.length)} HTTP/1.1\r\n${headers}\r\n`);
      if (head.length) upstream.write(head);
      socket.pipe(upstream).pipe(socket);
    });
    track(socket); track(upstream);
  });
  return server;
}
async function waitFor(check, message) {
  for (let attempt = 0; attempt < 100; attempt++) { try { const result = await check(); if (result) return result; } catch {} await delay(100); }
  throw new Error(message);
}
function seedHistory(dataDir) {
  const db = new DatabaseSync(path.join(dataDir, 'speeching-web.sqlite3'));
  const owner = createHash('sha256').update('off\0guest').digest('base64url');
  db.prepare("INSERT INTO chats(id,owner,title,language,created_at,updated_at,duration) VALUES(?,?,?,?,?,?,?)").run('synthetic-chat', owner, 'A walk through Madrid', 'Spanish', 1700000000, 1700000000, 145);
  db.prepare('INSERT INTO messages(chat_id,speaker,text) VALUES(?,?,?)').run('synthetic-chat', 'user', '<img src=x onerror=alert(1)> What is Madrid like?');
  db.prepare("INSERT INTO chats(id,owner,title,language,created_at,updated_at,duration) VALUES(?,?,?,?,?,?,?)").run('synthetic-other', owner, 'Another conversation', 'Spanish', 1690000000, 1690000000, 80);
  const insert = db.prepare('INSERT INTO messages(chat_id,speaker,text) VALUES(?,?,?)');
  for (let index = 0; index < 102; index++) insert.run('synthetic-other', 'user', `Earlier practice message ${index}`);
  db.prepare("INSERT INTO chats(id,owner,title,language,created_at,updated_at,duration) VALUES(?,?,?,?,?,?,?)").run('synthetic-temporary', owner, 'Temporary practice', 'Spanish', 1680000000, 1680000000, 20);
  insert.run('synthetic-temporary', 'user', 'Temporary message');
  db.close();
}
async function connectDevtools(port) {
  const target = await waitFor(async () => (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find(item => item.type === 'page'), 'Chrome target unavailable');
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let id = 0;
  const pending = new Map();
  socket.onmessage = event => { const message = JSON.parse(event.data); if (!message.id) return; const task = pending.get(message.id); pending.delete(message.id); message.error ? task.reject(new Error(message.error.message)) : task.resolve(message.result); };
  return { socket, call:(method, params = {}) => new Promise((resolve,reject) => { const number = ++id; pending.set(number,{resolve,reject}); socket.send(JSON.stringify({id:number,method,params})); }) };
}
async function evaluate(cdp, expression) {
  const result = await cdp.call('Runtime.evaluate', { expression, returnByValue:true, awaitPromise:true });
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
  return result.result.value;
}
async function capture(cdp, name) {
  if (!screenshots) return;
  mkdirSync(screenshots, { recursive:true });
  const image = await cdp.call('Page.captureScreenshot', { format:'png', captureBeyondViewport:false });
  writeFileSync(path.join(screenshots, `${name}.png`), Buffer.from(image.data, 'base64'));
}

try {
  if (!existsSync(executable)) throw new Error('Build the release executable first or set SPEECHING_TEST_EXE.');
  mock = createMock(); const mockPort = await listen(mock);
  const servicePort = await freePort(); proxy = createProxy(servicePort); const proxyPort = await listen(proxy);
  const origin = `http://127.0.0.1:${proxyPort}`;
  const dataDir = path.join(temp, 'data');
  const env = { ...process.env };
  for (const name of Object.keys(env)) if (name.toUpperCase().startsWith('SPEECHING_')) delete env[name];
  Object.assign(env, { SPEECHING_AUTH:'off', SPEECHING_LISTEN:`127.0.0.1:${servicePort}`, SPEECHING_ORIGIN:origin, SPEECHING_GEMINI_URL:`ws://127.0.0.1:${mockPort}/mock`, SPEECHING_MASTER_KEY:'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA', SPEECHING_WEB_DATA:dataDir });
  env.SPEECHING_TRUST_CLIENT_IP_HEADER = 'false';
  service = spawn(executable, [], { env, cwd:project, windowsHide:true, stdio:'ignore' });
  service.on('error', () => {});
  await waitFor(async () => (await fetch(`http://127.0.0.1:${servicePort}/api/mode`)).ok, 'Rust service did not start');
  const logoResponse = await fetch(`${origin}/speeching/icon.png`);
  if (!logoResponse.ok || !logoResponse.headers.get('content-type')?.startsWith('image/png')) throw new Error('The embedded logo was not served as a PNG');
  const logoBytes = Buffer.from(await logoResponse.arrayBuffer());
  if (!logoBytes.equals(readFileSync(path.join(project, 'static', 'icon.png')))) throw new Error('The served logo differs from the web asset');
  const officialLogo = path.join(project, '..', 'assets', 'icon.png');
  if (existsSync(officialLogo) && !logoBytes.equals(readFileSync(officialLogo))) throw new Error('The web logo differs from the original Speeching asset');
  seedHistory(dataDir);
  const profile = path.join(temp, 'chrome');
  chrome = spawn(chromePath, ['--headless=new','--disable-gpu','--no-first-run','--disable-extensions','--disable-background-networking','--use-fake-device-for-media-stream','--use-fake-ui-for-media-stream','--autoplay-policy=no-user-gesture-required','--remote-debugging-port=0',`--user-data-dir=${profile}`,'about:blank'], { windowsHide:true, stdio:'ignore' });
  chrome.on('error', () => {});
  const portFile = path.join(profile, 'DevToolsActivePort');
  const port = await waitFor(() => existsSync(portFile) && Number(readFileSync(portFile, 'utf8').split('\n')[0]), 'Chrome did not start');
  const cdp = await connectDevtools(port); devtools = cdp.socket;
  await cdp.call('Page.enable'); await cdp.call('Runtime.enable');
  for (const [name,width,height,mobile] of [['desktop',1200,900,false],['mobile',390,844,true]]) {
    await cdp.call('Emulation.setDeviceMetricsOverride', { width,height,deviceScaleFactor:1,mobile });
    await cdp.call('Page.navigate', { url:`${origin}/speeching/` });
    await waitFor(() => evaluate(cdp, "document.readyState === 'complete' && document.getElementById('workspace')?.hidden === false"), 'Practice view did not load');
    const dimensions = await evaluate(cdp, '({width:innerWidth,page:document.documentElement.scrollWidth})');
    if (dimensions.width !== width || dimensions.page > width) throw new Error(`${name} has horizontal overflow`);
    const micStyle = await evaluate(cdp, `(() => { const mic = document.getElementById('mic'); return { background:getComputedStyle(mic).backgroundColor, opacity:getComputedStyle(mic.querySelector('.mic-facets')).opacity }; })()`);
    if (micStyle.background !== 'rgb(7, 87, 239)' || micStyle.opacity !== '0.16') throw new Error('Microphone does not use the logo blue and 16% facets');
    const rings = await evaluate(cdp, `(() => { const mic = document.getElementById('mic'); return [getComputedStyle(mic,'::before').animationName,getComputedStyle(mic,'::after').animationName]; })()`);
    if (rings.some(name => name !== 'mic-nudge')) throw new Error('Idle microphone rings do not animate');
    await capture(cdp, name);
  }
  await cdp.call('Emulation.setEmulatedMedia', { features:[{name:'prefers-reduced-motion',value:'reduce'}] });
  const reducedRing = await evaluate(cdp, "getComputedStyle(document.getElementById('mic'),'::before').animationDuration");
  if (parseFloat(reducedRing) > 0.001) throw new Error('Microphone rings ignore reduced motion');
  await cdp.call('Emulation.setEmulatedMedia', { features:[] });
  for (const [name,width,height,mobile] of [['desktop',1200,720,false],['intermediate',820,640,false],['small',320,568,true],['mobile',390,844,true]]) {
    await cdp.call('Emulation.setDeviceMetricsOverride', { width,height,deviceScaleFactor:1,mobile });
    await cdp.call('Page.navigate', { url:`${origin}/speeching/` });
    await waitFor(() => evaluate(cdp, "document.readyState === 'complete' && document.getElementById('workspace')?.hidden === false"), 'Practice view did not load');
    await evaluate(cdp, `(async () => {
      const el = id => document.getElementById(id);
      const logo = document.querySelector('.brand-mark');
      if (!logo.complete || logo.naturalWidth !== 256 || logo.alt !== '') throw Error('Brand asset did not load accessibly');
      if (el('current-year').textContent !== String(new Date().getFullYear())) throw Error('Footer year is not current');
      const togglePosition = el('settings-toggle').getBoundingClientRect().right;
      el('settings-toggle').click();
      if (Math.abs(el('settings-toggle').getBoundingClientRect().right - togglePosition) > 2) throw Error('Header controls shifted when settings opened');
      const panel = el('settings-panel'), page = document.scrollingElement;
      if (getComputedStyle(panel).overflowY !== 'visible' || panel.scrollHeight > panel.clientHeight + 1) throw Error('Settings has an inner vertical scroll');
      if (page.scrollHeight <= innerHeight || page.scrollWidth > innerWidth) throw Error('Page scrolling or responsive width is incorrect');
      if (innerWidth > 800) {
        const header = document.querySelector('.topbar').getBoundingClientRect();
        const practice = document.querySelector('.practice').getBoundingClientRect();
        const drawer = panel.getBoundingClientRect();
        if (Math.abs(header.left - practice.left) > 2 || Math.abs(header.right - drawer.right) > 2) throw Error('Header is not aligned with the workspace');
      }
      if (document.querySelector('.site-footer').getBoundingClientRect().top < panel.getBoundingClientRect().bottom) throw Error('Footer overlaps settings');
    })()`);
    if (name === 'small') await evaluate(cdp, `(async () => {
      const form = document.getElementById('settings-form');
      form.elements.ui_language.value = 'es'; form.requestSubmit();
      if (document.documentElement.lang !== 'es' || document.scrollingElement.scrollWidth > innerWidth) throw Error('Spanish settings do not fit a narrow viewport');
      form.elements.ui_language.value = 'en'; form.requestSubmit();
    })()`);
    if (name === 'desktop') { await evaluate(cdp, "document.documentElement.dataset.theme = 'dark'"); await delay(300); await capture(cdp, 'layout-desktop-settings'); }
    if (name === 'mobile') await capture(cdp, 'layout-mobile-settings');
    await evaluate(cdp, `(() => {
      const save = document.querySelector('#settings-form button[type="submit"]');
      save.scrollIntoView({block:'end'});
      if (save.getBoundingClientRect().bottom > innerHeight + 1) throw Error('Page scrolling did not reach the save button');
      document.querySelector('.site-footer').scrollIntoView({block:'end'});
      if (document.querySelector('.site-footer').getBoundingClientRect().bottom > innerHeight + 1) throw Error('Page scrolling did not reach the footer');
    })()`);
  }
  await evaluate(cdp, "document.getElementById('settings-toggle').click()");
  await waitFor(() => evaluate(cdp, "document.getElementById('chat-select').options.length >= 4"), 'Saved conversations did not load');
  await evaluate(cdp, `(async () => {
    const wait = () => new Promise(resolve => setTimeout(resolve,100)); const el = id => document.getElementById(id);
    const picker = el('chat-select'); picker.value = 'synthetic-chat'; picker.dispatchEvent(new Event('change', {bubbles:true}));
    for (let i=0;i<100 && el('log').hidden;i++) await wait();
    if (el('log').hidden || !el('history-panel').hidden || !el('log-messages').textContent.includes('What is Madrid like?')) throw Error('Picker did not reveal the saved log without opening history');
    if (el('log-messages').querySelector('img') || el('log').getBoundingClientRect().top < document.querySelector('.practice').getBoundingClientRect().bottom) throw Error('Saved log is unsafe or not beneath practice');
    el('minimal').click(); if (!el('log').hidden) throw Error('Hidden text mode revealed the saved log');
    el('minimal').click(); if (el('log').hidden) throw Error('Saved log did not return after showing text');
  })()`);
  await cdp.call('Emulation.setDeviceMetricsOverride', { width:1200,height:900,deviceScaleFactor:1,mobile:false });
  await evaluate(cdp, "document.getElementById('log').scrollIntoView({block:'end'})");
  await capture(cdp, 'desktop-selected-log');
  await cdp.call('Emulation.setDeviceMetricsOverride', { width:390,height:844,deviceScaleFactor:1,mobile:true });
  await evaluate(cdp, "document.getElementById('log').scrollIntoView({block:'start'})");
  await capture(cdp, 'mobile-selected-log');
  historyDelays.set('synthetic-chat', 700);
  await evaluate(cdp, `(async () => {
    const wait = () => new Promise(resolve => setTimeout(resolve,100)); const el = id => document.getElementById(id); const picker = el('chat-select');
    picker.value = 'synthetic-chat'; picker.dispatchEvent(new Event('change', {bubbles:true}));
    await wait(); picker.value = 'synthetic-other'; picker.dispatchEvent(new Event('change', {bubbles:true}));
    for (let i=0;i<100 && el('log-title').textContent !== 'Another conversation';i++) await wait();
    await new Promise(resolve => setTimeout(resolve,900));
    if (picker.value !== 'synthetic-other' || el('log-title').textContent !== 'Another conversation') throw Error('A delayed log replaced the newer selection');
    if (el('more-messages').hidden) throw Error('Older messages were not offered');
    el('more-messages').click(); el('more-messages').click();
    for (let i=0;i<100 && el('log-messages').children.length < 102;i++) await wait();
    if (el('log-messages').children.length !== 102 || !el('more-messages').hidden) throw Error('Saved-message paging duplicated or lost messages');
    Object.defineProperty(navigator, 'clipboard', {configurable:true, value:{writeText:async value => { window.copiedLog = value; }}});
    el('copy').click(); for (let i=0;i<100 && !window.copiedLog;i++) await wait();
    if (!window.copiedLog.includes('Earlier practice message 0') || !window.copiedLog.includes('Earlier practice message 101')) throw Error('Copy omitted saved messages');
    const oldCreate = URL.createObjectURL, oldRevoke = URL.revokeObjectURL, oldClick = HTMLAnchorElement.prototype.click;
    try {
      URL.createObjectURL = blob => { window.downloadedLog = blob; return 'blob:speeching-test'; }; URL.revokeObjectURL = () => {};
      HTMLAnchorElement.prototype.click = function() { window.downloadName = this.download; };
      el('download').click(); for (let i=0;i<100 && !window.downloadedLog;i++) await wait();
      const content = await window.downloadedLog.text();
      if (window.downloadName !== 'speeching-conversation.txt' || !content.includes('Earlier practice message 0') || !content.includes('Earlier practice message 101')) throw Error('Download omitted saved messages');
    } finally { URL.createObjectURL = oldCreate; URL.revokeObjectURL = oldRevoke; HTMLAnchorElement.prototype.click = oldClick; }
    picker.value = 'synthetic-chat'; picker.dispatchEvent(new Event('change', {bubbles:true}));
    await wait(); picker.value = ''; picker.dispatchEvent(new Event('change', {bubbles:true}));
    await new Promise(resolve => setTimeout(resolve,900));
    if (picker.value || !el('log').hidden || el('log-messages').children.length) throw Error('A stale log appeared after starting a new conversation');
  })()`);
  historyDelays.clear();
  await evaluate(cdp, `(async () => {
    const wait = () => new Promise(resolve => setTimeout(resolve,100)); const el = id => document.getElementById(id); const picker = el('chat-select');
    picker.value = 'synthetic-temporary'; picker.dispatchEvent(new Event('change', {bubbles:true}));
    for (let i=0;i<100 && el('log-title').textContent !== 'Temporary practice';i++) await wait();
    const oldPrompt = window.prompt, oldConfirm = window.confirm;
    try {
      window.prompt = () => 'Renamed practice'; el('rename').click();
      for (let i=0;i<100 && el('log-title').textContent !== 'Renamed practice';i++) await wait();
      if (el('log-title').textContent !== 'Renamed practice') throw Error('Rename failed after moving the log');
      window.confirm = () => false; el('delete').click(); await wait();
      if (el('log').hidden) throw Error('Canceling deletion removed the log');
      window.confirm = () => true; el('delete').click();
      for (let i=0;i<100 && !el('log').hidden;i++) await wait();
      if (!el('log').hidden || picker.value) throw Error('Deleting the selected log did not reset the picker');
    } finally { window.prompt = oldPrompt; window.confirm = oldConfirm; }
  })()`);
  await evaluate(cdp, `(async () => {
    const wait = () => new Promise(resolve => setTimeout(resolve,200)); const el = id => document.getElementById(id);
    el('history-toggle').click(); await wait();
    if (document.activeElement.id !== 'history-title' || el('history-panel').getBoundingClientRect().top > innerHeight) throw Error('Mobile panel focus failed');
    el('chat-list').querySelector('button').click(); await wait();
    if (!el('log-messages').textContent.includes('<img src=x onerror=alert(1)>') || el('log-messages').querySelector('img')) throw Error('History was not rendered as text');
    if (getComputedStyle(el('history-panel')).overflowY !== 'visible' || getComputedStyle(el('log-messages')).overflowY !== 'visible') throw Error('Saved history has an inner vertical scroll');
    el('settings-toggle').click(); el('mic').click(); await wait();
    if (el('settings-panel').hidden || document.activeElement.id !== 'api-key' || !el('notice').textContent.includes('API key')) throw Error('Missing-key recovery did not focus the key field');
  })()`);
  await capture(cdp, 'mobile-missing-key');
  await evaluate(cdp, `(async () => {
    const wait = () => new Promise(resolve => setTimeout(resolve,200)); const el = id => document.getElementById(id);
    const form = el('settings-form');
    form.elements.input_mode.value = 'push'; form.elements.input_mode.dispatchEvent(new Event('change'));
    el('shortcut').focus(); el('shortcut').dispatchEvent(new KeyboardEvent('keydown',{key:'t',code:'KeyT',ctrlKey:true,bubbles:true,cancelable:true}));
    if (el('shortcut').value !== 'Ctrl+KeyT') throw Error('Shortcut remapping failed'); el('shortcut').blur();
    form.elements.preferences.value = 'Discuss astronomy'; form.elements.auto_open_log.checked = true;
    form.elements.ui_language.value = 'pt'; form.elements.theme.value = 'dark'; el('api-key').value = 'synthetic-test-key';
    form.requestSubmit();
    for (let i=0;i<50 && (el('api-key').value || document.documentElement.lang !== 'pt-BR' || document.documentElement.dataset.theme !== 'dark');i++) await wait();
    if (el('api-key').value || document.documentElement.lang !== 'pt-BR' || document.documentElement.dataset.theme !== 'dark') throw Error('Settings did not save');
    el('mic').click();
    for (let i=0;i<50 && (el('hold').hidden || el('hold').disabled);i++) await wait();
    if (el('hold').hidden || el('hold').disabled) throw Error('Push-to-talk did not become ready: ' + el('notice').textContent);
    await new Promise(resolve => setTimeout(resolve,300));
    if (getComputedStyle(el('mic')).backgroundColor !== 'rgb(166, 56, 71)' || getComputedStyle(el('mic').querySelector('.mic-facets')).opacity !== '0.16') throw Error('Active microphone lost its red state or gem texture');
    if (getComputedStyle(el('mic'),'::before').animationName !== 'none') throw Error('Active microphone rings should not nudge');
    document.dispatchEvent(new KeyboardEvent('keydown',{key:'t',code:'KeyT',ctrlKey:true,bubbles:true,cancelable:true}));
    for (let i=0;i<50 && !el('live-messages').textContent.includes('Hello from the mock provider');i++) await wait();
    if (!el('live-messages').textContent.includes('Hello from the mock provider') || el('live-messages').querySelector('img')) throw Error('Browser audio or safe transcript failed');
    const activeSelection = el('chat-select').value; el('history-toggle').click(); await wait();
    const other = [...el('chat-list').querySelectorAll('button')].find(button => button.textContent.includes('Another conversation'));
    if (!other) throw Error('Saved history row was missing'); other.click();
    for (let i=0;i<50 && el('log-title').textContent !== 'Another conversation';i++) await wait();
    if (el('chat-select').value !== activeSelection || !el('delete').disabled) throw Error('Browsing history changed or endangered the active session');
    el('settings-toggle').click();
    document.dispatchEvent(new KeyboardEvent('keyup',{key:'t',code:'KeyT',ctrlKey:true,bubbles:true,cancelable:true}));
    if (el('hold').getAttribute('aria-pressed') !== 'false') throw Error('Shortcut release failed');
    el('hold').click(); if (el('hold').getAttribute('aria-pressed') !== 'true') throw Error('Assistive activation failed');
  })()`);
  if (!audioChunks || !preferenceSeen) throw new Error('Audio or lower-priority practice preferences did not reach the mock provider');
  await cdp.call('Emulation.setDeviceMetricsOverride', { width:1200,height:900,deviceScaleFactor:1,mobile:false });
  await evaluate(cdp, "window.scrollTo(0,0)"); await capture(cdp, 'desktop-settings');
  await cdp.call('Emulation.setDeviceMetricsOverride', { width:390,height:844,deviceScaleFactor:1,mobile:true });
  await evaluate(cdp, "document.getElementById('hold').click(); document.getElementById('history-toggle').click()");
  await delay(300); await capture(cdp, 'mobile-history');
  await evaluate(cdp, `(async () => {
    const wait = () => new Promise(resolve => setTimeout(resolve,200)); const el = id => document.getElementById(id);
    el('mic').click();
    for (let i=0;i<50 && (el('history-panel').hidden || !el('log-messages').textContent.includes('Hello from the mock provider'));i++) await wait();
    if (el('history-panel').hidden || !el('log-messages').textContent.includes('Hello from the mock provider')) throw Error('Log did not open after session');
  })()`);
  await evaluate(cdp, `(async () => {
    const wait = () => new Promise(resolve => setTimeout(resolve,100)); const el = id => document.getElementById(id);
    el('mic').click();
    for (let i=0;i<100 && el('hold').disabled;i++) await wait();
    if (el('hold').disabled) throw Error('Second session did not start');
  })()`);
  chatListDelay = 700;
  await evaluate(cdp, `(async () => {
    const wait = () => new Promise(resolve => setTimeout(resolve,100)); const el = id => document.getElementById(id); const picker = el('chat-select');
    el('mic').click();
    for (let i=0;i<100 && picker.disabled;i++) await wait();
    picker.value = 'synthetic-other'; picker.dispatchEvent(new Event('change', {bubbles:true}));
    for (let i=0;i<100 && el('log-title').textContent !== 'Another conversation';i++) await wait();
    await new Promise(resolve => setTimeout(resolve,900));
    if (picker.value !== 'synthetic-other' || el('log-title').textContent !== 'Another conversation') throw Error('A late post-session refresh replaced the user selection');
  })()`);
  chatListDelay = 0;
  rejectNextSetup = true;
  await evaluate(cdp, `(async () => {
    const wait = () => new Promise(resolve => setTimeout(resolve,100)); const el = id => document.getElementById(id);
    el('mic').click();
    for (let i=0;i<100 && !el('notice').textContent.includes('rejeitou a chave');i++) await wait();
    if (!el('notice').textContent.includes('rejeitou a chave') || el('notice').textContent.includes('synthetic-secret-marker')) throw Error('Localized provider error was missing or unsafe');
  })()`);
  service.kill();
  await waitFor(() => service.exitCode !== null || service.signalCode !== null, 'First service did not stop');
  const authPort = await freePort(), authOrigin = `http://127.0.0.1:${authPort}`;
  const authEnv = { ...env, SPEECHING_AUTH:'builtin', SPEECHING_LISTEN:`127.0.0.1:${authPort}`, SPEECHING_ORIGIN:authOrigin, SPEECHING_WEB_DATA:path.join(temp, 'auth-data') };
  service = spawn(executable, [], { env:authEnv, cwd:project, windowsHide:true, stdio:'ignore' });
  service.on('error', () => {});
  await waitFor(async () => (await fetch(`${authOrigin}/api/mode`)).ok, 'Account service did not start');
  await cdp.call('Page.navigate', { url:`${authOrigin}/` });
  await waitFor(() => evaluate(cdp, "document.readyState === 'complete' && document.getElementById('login-view')?.hidden === false"), 'Login did not load');
  await cdp.call('Emulation.setDeviceMetricsOverride', { width:1200,height:900,deviceScaleFactor:1,mobile:false });
  await capture(cdp, 'desktop-login');
  await cdp.call('Emulation.setDeviceMetricsOverride', { width:390,height:844,deviceScaleFactor:1,mobile:true });
  await capture(cdp, 'mobile-login');
  await evaluate(cdp, "document.getElementById('register-link').click()"); await capture(cdp, 'mobile-register');
  await evaluate(cdp, "document.getElementById('login-link').click(); document.getElementById('forgot-password').click()"); await capture(cdp, 'mobile-recovery');
  await evaluate(cdp, `(async () => {
    const wait = () => new Promise(resolve => setTimeout(resolve,100)); const el = id => document.getElementById(id);
    el('forgot-password').click();
    if (el('recovery-help').hidden || !el('recovery-help').textContent.includes('start-web.cmd user add USERNAME') || document.activeElement.id !== 'auth-heading') throw Error('Administrator recovery guidance failed');
    el('login-link').click(); el('register-link').click();
    const registration = el('register-form');
    registration.elements.username.value = 'bad name'; registration.elements.password.value = 'browser-password-123'; registration.elements.confirm.value = 'browser-password-123';
    registration.requestSubmit(); await wait();
    if (!el('auth-message').textContent.includes('Use letters')) throw Error('Username validation failed');
    registration.elements.username.value = 'BrowserUser'; registration.elements.password.value = 'browser-password-123'; registration.elements.confirm.value = 'different-password';
    registration.requestSubmit(); await wait();
    if (!el('auth-message').textContent.includes('do not match')) throw Error('Password confirmation failed');
    registration.elements.confirm.value = 'browser-password-123'; registration.requestSubmit();
    for (let i=0;i<100 && !el('auth-message').textContent.includes('Account created');i++) await wait();
    if (el('login-form').hidden || el('login-form').elements.username.value !== 'BrowserUser') throw Error('Registration did not return to sign in');
    el('login-form').elements.password.value = 'browser-password-123'; el('login-form').requestSubmit();
    for (let i=0;i<100 && el('workspace').hidden;i++) await wait();
    if (el('workspace').hidden) throw Error('New account could not sign in');
    el('settings-form').elements.ui_language.value = 'pt'; el('settings-form').requestSubmit();
    for (let i=0;i<50 && document.documentElement.lang !== 'pt-BR';i++) await wait();
    el('logout').click();
    for (let i=0;i<100 && el('login-view').hidden;i++) await wait();
    el('forgot-password').click();
    if (el('recovery-help').hidden || !el('recovery-help').textContent.includes('administrador')) throw Error('Localized recovery guidance failed');
  })()`);
  console.log(JSON.stringify({ selectedLog:true, staleSelectionGuard:true, paging:true, logActions:true, micFacets:true, pageScrolling:true, logoAndFooter:true, browserAudio:true, audioChunks, localizedProviderError:true, mountedPath:true, privateTextEscaped:true, shortcutRemapped:true, assistiveToggle:true, logAutoOpened:true, portuguese:true, darkMode:true, registration:true, administratorRecovery:true }));
} finally {
  devtools?.close(); chrome?.kill(); service?.kill();
  for (const socket of connections) socket.destroy();
  proxy?.close(); mock?.close();
  await delay(500);
  const parent = path.resolve(os.tmpdir()) + path.sep;
  if (!path.resolve(temp).startsWith(parent)) throw new Error('Refusing to remove a test directory outside the temp directory');
  for (let attempt=0; attempt<5; attempt++) { try { rmSync(temp, { recursive:true, force:true }); break; } catch { await delay(300); } }
  if (existsSync(temp)) { console.error(`Could not remove test directory ${temp}`); process.exitCode = 1; }
}
