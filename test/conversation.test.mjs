import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';

test('a mock live session configures voice and transcripts, sends audio, and stops cleanly', async () => {
  const listeners = new Map();
  class Element {
    constructor(id = '') { this.id = id; this.value = ''; this.dataset = {}; this.hidden = false; this.children = []; this.attributes = {}; this.clientWidth = 300; this.clientHeight = 38; this.classList = { add() {}, remove() {}, toggle() {} }; }
    addEventListener(type, fn) { listeners.set(`${this.id}:${type}`, fn); }
    setAttribute(key, value) { this.attributes[key] = value; }
    append(...children) { this.children.push(...children); }
    querySelector() { return new Element('avatar'); }
    querySelectorAll() { return []; }
    cloneNode() { return new Element('avatar'); }
    showModal() {} close() {} focus() {} remove() {}
    getContext() { return { setTransform() {}, clearRect() {}, beginPath() {}, moveTo() {}, lineTo() {}, stroke() {} }; }
  }
  const elements = new Map();
  const element = id => { if (!elements.has(id)) elements.set(id, new Element(id)); return elements.get(id); };
  element('#language').value = 'English';
  element('#level').value = 'Intermediate';
  element('#api-key').value = 'test-key';
  const voices = ['Aoede', 'Puck', 'Kore'].map(name => { const voice = new Element(name); voice.dataset.voice = name; return voice; });
  const track = { stopped: false, enabled: true, stop() { this.stopped = true; } };
  class Node { connect(node) { return node; } disconnect() {} }
  class AudioContext {
    constructor() { this.currentTime = 1; this.destination = new Node(); this.audioWorklet = { addModule: async () => {} }; }
    async resume() {} async close() {}
    createMediaStreamSource() { return new Node(); }
    createAnalyser() { return Object.assign(new Node(), { fftSize: 256, getFloatTimeDomainData() {} }); }
    createGain() { return Object.assign(new Node(), { gain: { value: 1 } }); }
    createBuffer(_channels, length, rate) { return { duration: length / rate, getChannelData: () => new Float32Array(length) }; }
    createBufferSource() { return Object.assign(new Node(), { start() {}, stop() {} }); }
  }
  let capture;
  class AudioWorkletNode extends Node { constructor() { super(); this.port = {}; capture = this; } }
  let connection;
  class WebSocket {
    static OPEN = 1; static CONNECTING = 0;
    constructor() { this.readyState = 0; this.bufferedAmount = 0; this.sent = []; connection = this; queueMicrotask(() => { this.readyState = 1; this.onopen(); }); }
    send(raw) {
      const message = JSON.parse(raw); this.sent.push(message);
      if (message.setup) queueMicrotask(() => this.onmessage({ data: JSON.stringify({ setupComplete: {} }) }));
    }
    close() { this.readyState = 3; this.onclose?.({ code: 1000 }); }
  }
  const sandbox = {
    document: { querySelector: element, querySelectorAll: () => voices, createElement: () => new Element() },
    window: { AudioWorkletNode, addEventListener() {} },
    navigator: { mediaDevices: { getUserMedia: async () => ({ getTracks: () => [track], getAudioTracks: () => [track] }) } },
    localStorage: { getItem: () => null, setItem() {} },
    fetch: async () => ({ ok: true, json: async () => ({ token: 'auth_tokens/example' }) }),
    AudioContext, AudioWorkletNode, WebSocket, TextDecoder, Float32Array, Uint8Array, ArrayBuffer,
    btoa: value => Buffer.from(value, 'binary').toString('base64'), atob: value => Buffer.from(value, 'base64').toString('binary'),
    requestAnimationFrame() {}, devicePixelRatio: 1, performance: { now: () => 0 }, setTimeout, clearTimeout, setInterval, clearInterval,
    console
  };
  vm.createContext(sandbox);
  vm.runInContext(await readFile(new URL('../public/app.js', import.meta.url), 'utf8'), sandbox);
  await listeners.get('#start-button:click')();
  assert.equal(element('#start-text').textContent, 'End conversation');
  assert.equal(connection.sent[0].setup.model, 'models/gemini-3.8-live');
  assert.equal(connection.sent[0].setup.generationConfig.speechConfig.voiceConfig.prebuiltVoiceConfig.voiceName, 'Aoede');
  assert.deepEqual(Object.keys(connection.sent[0].setup.inputAudioTranscription), []);
  assert.deepEqual(Object.keys(connection.sent[0].setup.outputAudioTranscription), []);
  capture.port.onmessage({ data: new Uint8Array([0, 1, 2, 3]).buffer });
  assert.equal(connection.sent.at(-1).realtimeInput.audio.mimeType, 'audio/pcm;rate=16000');
  await connection.onmessage({ data: JSON.stringify({ serverContent: { inputTranscription: { text: 'Hello there.' } } }) });
  await connection.onmessage({ data: JSON.stringify({ serverContent: { outputTranscription: { text: 'Hello back.' }, turnComplete: true } }) });
  const rows = element('#chat').children;
  assert.equal(rows.length, 2);
  assert.equal(rows[0].children[1].children[1].textContent, 'Hello there.');
  assert.equal(rows[1].children[1].children[1].textContent, 'Hello back.');
  await listeners.get('#start-button:click')();
  assert.equal(track.stopped, true);
  assert.equal(element('#start-text').textContent, 'Start conversation');
});
