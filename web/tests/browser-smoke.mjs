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
let chrome, service, devtools, proxy, mock, audioChunks = 0, preferenceSeen = false;
const connections = new Set();

function track(socket) { connections.add(socket); socket.on('close', () => connections.delete(socket)); socket.on('error', () => socket.destroy()); }
function listen(server) { return new Promise(resolve => server.listen(0, '127.0.0.1', () => resolve(server.address().port))); }
async function freePort() { const server = net.createServer(); const port = await listen(server); await new Promise(resolve => server.close(resolve)); return port; }
function sendFrame(socket, value) {
  const data = Buffer.from(JSON.stringify(value));
  const header = data.length < 126 ? Buffer.from([0x81, data.length]) : Buffer.from([0x81,126,data.length >> 8,data.length & 255]);
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
      if (value.setup) sendFrame(socket, { setupComplete:{} });
      if (value.clientContent && JSON.stringify(value).includes('Discuss astronomy')) preferenceSeen = true;
      if (!value.realtimeInput?.audio) return;
      audioChunks++;
      if (replied) return;
      replied = true;
      sendFrame(socket, { serverContent:{ inputTranscription:{text:'Synthetic learner speech.'}, outputTranscription:{text:'Hello from the mock provider <img src=x onerror=alert(1)>'}, modelTurn:{parts:[{inlineData:{data:'AAE='}}]}, turnComplete:true } });
    });
  });
  return server;
}
function createProxy(servicePort) {
  const server = http.createServer((request, response) => {
    const suffix = request.url.startsWith('/speeching/') ? request.url.slice('/speeching'.length) : request.url;
    const upstream = http.request({ hostname:'127.0.0.1', port:servicePort, path:suffix, method:request.method, headers:request.headers }, result => { response.writeHead(result.statusCode, result.headers); result.pipe(response); });
    upstream.on('error', () => response.writeHead(502).end()); request.pipe(upstream);
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
  const env = { ...process.env, SPEECHING_AUTH:'off', SPEECHING_LISTEN:`127.0.0.1:${servicePort}`, SPEECHING_ORIGIN:origin, SPEECHING_GEMINI_URL:`ws://127.0.0.1:${mockPort}/mock`, SPEECHING_MASTER_KEY:'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA', SPEECHING_WEB_DATA:dataDir };
  delete env.SPEECHING_MASTER_KEY_FILE;
  delete env.SPEECHING_PROXY_SECRET_FILE; delete env.SPEECHING_PROXY_SECRET;
  env.SPEECHING_TRUST_CLIENT_IP_HEADER = 'false';
  service = spawn(executable, [], { env, cwd:project, windowsHide:true, stdio:'ignore' });
  service.on('error', () => {});
  await waitFor(async () => (await fetch(`http://127.0.0.1:${servicePort}/api/mode`)).ok, 'Rust service did not start');
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
    await capture(cdp, name);
  }
  await evaluate(cdp, `(async () => {
    const wait = () => new Promise(resolve => setTimeout(resolve,200)); const el = id => document.getElementById(id);
    el('history-toggle').click(); await wait();
    if (document.activeElement.id !== 'history-title' || el('history-panel').getBoundingClientRect().top > innerHeight) throw Error('Mobile panel focus failed');
    el('chat-list').querySelector('button').click(); await wait();
    if (!el('log-messages').textContent.includes('<img src=x onerror=alert(1)>') || el('log-messages').querySelector('img')) throw Error('History was not rendered as text');
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
    form.requestSubmit(); await wait();
    if (el('api-key').value || document.documentElement.lang !== 'pt-BR' || document.documentElement.dataset.theme !== 'dark') throw Error('Settings did not save');
    el('mic').click();
    for (let i=0;i<50 && (el('hold').hidden || el('hold').disabled);i++) await wait();
    if (el('hold').hidden || el('hold').disabled) throw Error('Push-to-talk did not become ready: ' + el('notice').textContent);
    document.dispatchEvent(new KeyboardEvent('keydown',{key:'t',code:'KeyT',ctrlKey:true,bubbles:true,cancelable:true}));
    for (let i=0;i<50 && !el('live-messages').textContent.includes('Hello from the mock provider');i++) await wait();
    if (!el('live-messages').textContent.includes('Hello from the mock provider') || el('live-messages').querySelector('img')) throw Error('Browser audio or safe transcript failed');
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
  console.log(JSON.stringify({ browserAudio:true, audioChunks, mountedPath:true, privateTextEscaped:true, shortcutRemapped:true, assistiveToggle:true, logAutoOpened:true, portuguese:true, darkMode:true }));
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
