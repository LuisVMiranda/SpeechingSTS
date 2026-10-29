import test from 'node:test';
import assert from 'node:assert/strict';
import { createAppServer } from '../server.mjs';

test('local app serves its interface and obtains an ephemeral token only for its own origin', async () => {
  let receivedKey;
  const server = createAppServer({ tokenProvider: async key => { receivedKey = key; return 'auth_tokens/test'; } });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  try {
    const page = await fetch(origin);
    assert.equal(page.status, 200);
    assert.match(await page.text(), /Speeching/);
    assert.match(page.headers.get('content-security-policy'), /generativelanguage\.googleapis\.com/);
    const denied = await fetch(`${origin}/api/token`, { method: 'POST', headers: { origin: 'https://example.com', 'content-type': 'application/json' }, body: JSON.stringify({ apiKey: 'secret' }) });
    assert.equal(denied.status, 403);
    assert.equal(receivedKey, undefined);
    const result = await fetch(`${origin}/api/token`, { method: 'POST', headers: { origin, 'content-type': 'application/json' }, body: JSON.stringify({ apiKey: ' key-for-test ' }) });
    assert.deepEqual(await result.json(), { token: 'auth_tokens/test' });
    assert.equal(receivedKey, 'key-for-test');
    const missing = await fetch(`${origin}/api/token`, { method: 'POST', headers: { origin, 'content-type': 'application/json' }, body: JSON.stringify({ apiKey: '' }) });
    assert.equal(missing.status, 400);
    const traversal = await fetch(`${origin}/server.mjs`);
    assert.equal(traversal.status, 404);
  } finally { await new Promise(resolve => server.close(resolve)); }
});
