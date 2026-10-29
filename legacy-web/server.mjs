import http from 'node:http';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { join, extname } from 'node:path';
import { spawn } from 'node:child_process';

const publicDir = fileURLToPath(new URL('./public/', import.meta.url));
const mime = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.svg': 'image/svg+xml' };
const files = new Set(['/','/index.html','/style.css','/app.js','/audio-worklet.js','/icon.svg']);

async function makeToken(apiKey) {
  const response = await fetch('https://generativelanguage.googleapis.com/v1beta/auth_tokens', {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'x-goog-api-key': apiKey },
    body: JSON.stringify({ uses: 1 })
  });
  const body = await response.json().catch(() => ({}));
  if (!response.ok || !body.name) {
    const error = new Error(response.status === 429 ? 'Google rate limit reached. Try again later.' : response.status === 403 || response.status === 400 ? 'Google could not authorize this key. Check the key and model access in AI Studio.' : 'Could not get a token from Google. Check your connection and try again.');
    error.status = response.status >= 400 && response.status < 500 ? response.status : 502;
    throw error;
  }
  return body.name;
}

function json(res, code, value) {
  res.writeHead(code, { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' });
  res.end(JSON.stringify(value));
}

export function createAppServer({ tokenProvider = makeToken } = {}) {
  return http.createServer(async (req, res) => {
    const localHost = `127.0.0.1:${res.socket.localPort}`;
    if (req.headers.host !== localHost) return json(res, 403, { error: 'Invalid local host.' });
    const path = new URL(req.url, `http://${localHost}`).pathname;
    if (req.method === 'POST' && path === '/api/token') {
      if (req.headers.origin !== `http://${localHost}` || !req.headers['content-type']?.startsWith('application/json')) return json(res, 403, { error: 'Invalid request origin.' });
      let body = '';
      try {
        for await (const chunk of req) {
          body += chunk;
          if (body.length > 4096) return json(res, 413, { error: 'Request too large.' });
        }
        const { apiKey } = JSON.parse(body);
        if (typeof apiKey !== 'string' || !apiKey.trim() || apiKey.length > 512) return json(res, 400, { error: 'Enter your Gemini API key.' });
        const token = await tokenProvider(apiKey.trim());
        return json(res, 200, { token });
      } catch (error) {
        return json(res, error.status || 502, { error: error.status ? error.message : 'Could not reach Google. Check your connection and try again.' });
      }
    }
    if (req.method !== 'GET' || !files.has(path)) return json(res, 404, { error: 'Not found.' });
    const file = path === '/' ? '/index.html' : path;
    try {
      const data = await readFile(join(publicDir, file.slice(1)));
      res.writeHead(200, {
        'content-type': mime[extname(file)] || 'application/octet-stream',
        'cache-control': 'no-store',
        'x-content-type-options': 'nosniff',
        'referrer-policy': 'no-referrer',
        'content-security-policy': "default-src 'self'; connect-src 'self' wss://generativelanguage.googleapis.com; img-src 'self' data:; style-src 'self'; script-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"
      });
      res.end(data);
    } catch { json(res, 404, { error: 'Not found.' }); }
  });
}

function openWindow(url) {
  if (process.platform !== 'win32') return;
  const candidates = [
    join(process.env['PROGRAMFILES(X86)'] || '', 'Microsoft/Edge/Application/msedge.exe'),
    join(process.env.PROGRAMFILES || '', 'Microsoft/Edge/Application/msedge.exe'),
    join(process.env.PROGRAMFILES || '', 'Google/Chrome/Application/chrome.exe'),
    join(process.env.LOCALAPPDATA || '', 'Google/Chrome/Application/chrome.exe')
  ];
  import('node:fs').then(({ existsSync }) => {
    const browser = candidates.find(existsSync);
    if (browser) {
      const child = spawn(browser, [`--app=${url}`, '--window-size=460,850'], { detached: true, stdio: 'ignore', windowsHide: true });
      child.on('error', () => console.log('Open the address above in a browser.'));
      child.unref();
    } else console.log('Open the address above in a browser.');
  });
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const server = createAppServer();
  server.listen(0, '127.0.0.1', () => {
    const url = `http://127.0.0.1:${server.address().port}/`;
    console.log(`Speeching is ready at ${url}`);
    console.log('Close this window to stop the local server.');
    if (!process.argv.includes('--no-open')) openWindow(url);
  });
}
