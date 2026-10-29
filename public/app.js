const $ = (selector) => document.querySelector(selector);
const ui = {
  settings: $('#settings-dialog'), settingsForm: $('#settings-form'), key: $('#api-key'), language: $('#language'), level: $('#level'),
  settingsButton: $('#settings-button'), closeSettings: $('#close-settings'), start: $('#start-button'), startText: $('#start-text'), mute: $('#mute-button'),
  notice: $('#notice'), status: $('#connection-label'), dot: $('#status-dot'), languageLabel: $('#language-label'), waveLabel: $('#wave-label'),
  wave: $('#wave'), timer: $('#timer'), chat: $('#chat'), empty: $('#empty-chat'), clear: $('#clear-button'), voices: [...document.querySelectorAll('.voice-option')]
};
const defaults = { language: 'English', level: 'Intermediate', voice: 'Aoede' };
let preferences = { ...defaults };
try { preferences = { ...defaults, ...JSON.parse(localStorage.getItem('speeching-preferences') || '{}') }; } catch { /* Use defaults if storage is unavailable. */ }
if (!['Aoede', 'Puck', 'Kore'].includes(preferences.voice)) preferences.voice = defaults.voice;
ui.language.value = preferences.language;
ui.level.value = preferences.level;

let state = 'idle';
let muted = false;
let socket = null;
let token = '';
let tokenIssuedAt = 0;
let resumeHandle = '';
let goingAway = false;
let retries = 0;
let manuallyStopped = false;
let audioContext = null;
let stream = null;
let micAnalyser = null;
let playbackAnalyser = null;
let capture = null;
let outputSources = new Set();
let nextAudioTime = 0;
let startedAt = 0;
let timerId = null;
let activeMessages = { user: null, model: null };
const wsBase = 'wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContentConstrained';

function savePreferences() {
  try { localStorage.setItem('speeching-preferences', JSON.stringify(preferences)); } catch { /* The app works without persisted settings. */ }
}
function updatePreferences() {
  ui.languageLabel.textContent = `${preferences.language} · ${preferences.level.toLowerCase()}`;
  for (const button of ui.voices) {
    const selected = button.dataset.voice === preferences.voice;
    button.classList.toggle('selected', selected);
    button.setAttribute('aria-pressed', String(selected));
    button.disabled = state !== 'idle';
  }
}
function notice(message, isError = false) {
  ui.notice.textContent = message;
  ui.notice.classList.toggle('error', isError);
}
function setState(value) {
  state = value;
  ui.start.disabled = value === 'starting' || value === 'reconnecting';
  ui.start.classList.toggle('stop', value === 'live');
  ui.startText.textContent = value === 'starting' ? 'Connecting…' : value === 'reconnecting' ? 'Reconnecting…' : value === 'live' ? 'End conversation' : 'Start conversation';
  ui.mute.disabled = value !== 'live';
  ui.dot.classList.toggle('live', value === 'live');
  ui.dot.classList.remove('error');
  ui.status.textContent = value === 'starting' ? 'Connecting' : value === 'reconnecting' ? 'Reconnecting' : value === 'live' ? 'Connected' : 'Ready when you are';
  updatePreferences();
}
function updateMute() {
  ui.mute.classList.toggle('muted', muted);
  ui.mute.setAttribute('aria-label', muted ? 'Unmute microphone' : 'Mute microphone');
  ui.mute.title = muted ? 'Unmute microphone' : 'Mute microphone';
  ui.mute.setAttribute('aria-pressed', String(muted));
}
function appendTranscript(role, fragment) {
  if (!fragment || !fragment.trim()) return;
  if (role === 'user' && activeMessages.model && !activeMessages.user) activeMessages.model = null;
  if (role === 'model' && !activeMessages.model) activeMessages.user = null;
  let entry = activeMessages[role];
  if (!entry) {
    ui.empty.hidden = true;
    const row = document.createElement('div');
    row.className = `message ${role}`;
    const avatar = role === 'model' ? ui.voices.find(button => button.dataset.voice === preferences.voice).querySelector('.avatar').cloneNode(true) : document.createElement('span');
    if (role === 'user') { avatar.className = 'avatar you-avatar'; avatar.textContent = 'You'; }
    const content = document.createElement('div');
    content.className = 'message-content';
    const name = document.createElement('div');
    name.className = 'message-name';
    name.textContent = role === 'user' ? 'You' : preferences.voice;
    const text = document.createElement('p');
    text.className = 'message-text';
    content.append(name, text);
    row.append(avatar, content);
    ui.chat.append(row);
    entry = activeMessages[role] = { text, value: '' };
  }
  if (fragment.startsWith(entry.value) && entry.value) entry.value = fragment;
  else {
    const separator = entry.value && !/\s$/.test(entry.value) && !/^\s|^[.,!?;:…]/u.test(fragment) ? ' ' : '';
    entry.value += separator + fragment;
  }
  entry.text.textContent = entry.value;
  ui.chat.scrollTop = ui.chat.scrollHeight;
}
function clearTranscript() {
  ui.chat.querySelectorAll('.message').forEach(item => item.remove());
  ui.empty.hidden = false;
  activeMessages = { user: null, model: null };
}
function audioToBase64(buffer) {
  const bytes = new Uint8Array(buffer);
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}
function send(payload) {
  if (socket?.readyState === WebSocket.OPEN && state === 'live') socket.send(JSON.stringify(payload));
}
async function prepareAudio() {
  audioContext = new AudioContext();
  await audioContext.resume();
  stream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true } });
  await audioContext.audioWorklet.addModule('/audio-worklet.js');
  const microphone = audioContext.createMediaStreamSource(stream);
  micAnalyser = audioContext.createAnalyser();
  micAnalyser.fftSize = 256;
  microphone.connect(micAnalyser);
  capture = new AudioWorkletNode(audioContext, 'microphone-capture');
  microphone.connect(capture);
  const silent = audioContext.createGain();
  silent.gain.value = 0;
  capture.connect(silent).connect(audioContext.destination);
  playbackAnalyser = audioContext.createAnalyser();
  playbackAnalyser.fftSize = 256;
  playbackAnalyser.connect(audioContext.destination);
  capture.port.onmessage = event => {
    if (!muted && state === 'live' && socket?.readyState === WebSocket.OPEN && socket.bufferedAmount < 150000) {
      send({ realtimeInput: { audio: { data: audioToBase64(event.data), mimeType: 'audio/pcm;rate=16000' } } });
    }
  };
}
function stopPlayback() {
  for (const source of outputSources) { try { source.stop(); } catch { /* Already stopped. */ } }
  outputSources.clear();
  nextAudioTime = audioContext?.currentTime || 0;
}
function playAudio(base64) {
  if (!audioContext || !playbackAnalyser) return;
  const binary = atob(base64);
  const length = Math.floor(binary.length / 2);
  if (!length) return;
  const audio = audioContext.createBuffer(1, length, 24000);
  const samples = audio.getChannelData(0);
  for (let i = 0; i < length; i++) {
    const value = binary.charCodeAt(i * 2) | (binary.charCodeAt(i * 2 + 1) << 8);
    samples[i] = (value >= 32768 ? value - 65536 : value) / 32768;
  }
  const source = audioContext.createBufferSource();
  source.buffer = audio;
  source.connect(playbackAnalyser);
  nextAudioTime = Math.max(audioContext.currentTime + 0.025, nextAudioTime);
  source.start(nextAudioTime);
  nextAudioTime += audio.duration;
  outputSources.add(source);
  source.onended = () => outputSources.delete(source);
}
async function getToken() {
  const response = await fetch('/api/token', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ apiKey: ui.key.value }) });
  const data = await response.json();
  if (!response.ok || !data.token) throw new Error(data.error || 'Could not connect to Google. Try again.');
  tokenIssuedAt = Date.now();
  return data.token;
}
function makeSetup(handle = '') {
  const instructions = `You are a warm and patient spoken language practice partner. Practice in ${preferences.language}. The learner is at ${preferences.level.toLowerCase()} level. Keep responses concise and natural, ask one follow-up question at a time, allow the learner time to speak, and only gently correct significant mistakes when useful. Respond in ${preferences.language}; use another language only if the learner asks for help.`;
  return { setup: {
    model: 'models/gemini-3.8-live',
    generationConfig: { responseModalities: ['AUDIO'], speechConfig: { voiceConfig: { prebuiltVoiceConfig: { voiceName: preferences.voice } } } },
    systemInstruction: { parts: [{ text: instructions }] },
    inputAudioTranscription: {}, outputAudioTranscription: {},
    contextWindowCompression: { slidingWindow: {} }, sessionResumption: handle ? { handle } : {}
  } };
}
function handleResponse(message) {
  if (message.sessionResumptionUpdate?.resumable && message.sessionResumptionUpdate.newHandle) resumeHandle = message.sessionResumptionUpdate.newHandle;
  if (message.goAway) { goingAway = true; notice('Reconnecting soon to keep your conversation going.'); }
  const content = message.serverContent;
  if (!content) return;
  if (content.interrupted) { stopPlayback(); activeMessages.model = null; }
  if (content.inputTranscription?.text) appendTranscript('user', content.inputTranscription.text);
  if (content.outputTranscription?.text) appendTranscript('model', content.outputTranscription.text);
  for (const part of content.modelTurn?.parts || []) {
    if (part.inlineData?.data && part.inlineData.mimeType?.startsWith('audio/')) playAudio(part.inlineData.data);
  }
  if (content.turnComplete) activeMessages = { user: null, model: null };
}
function connect(handle = '') {
  return new Promise((resolve, reject) => {
    let ready = false;
    let settled = false;
    const ws = new WebSocket(`${wsBase}?access_token=${encodeURIComponent(token)}`);
    socket = ws;
    ws.binaryType = 'arraybuffer';
    const timeout = setTimeout(() => { if (!settled) { settled = true; ws.close(); reject(new Error('Google took too long to connect. Please try again.')); } }, 20000);
    ws.onopen = () => ws.send(JSON.stringify(makeSetup(handle)));
    ws.onmessage = async event => {
      try {
        const raw = typeof event.data === 'string' ? event.data : event.data instanceof ArrayBuffer ? new TextDecoder().decode(event.data) : await event.data.text();
        const message = JSON.parse(raw);
        if (message.setupComplete && !ready) {
          ready = true;
          if (!settled) { settled = true; clearTimeout(timeout); resolve(); }
        } else handleResponse(message);
      } catch { notice('A response could not be read. Continue speaking or reconnect.', true); }
    };
    ws.onerror = () => { /* The close event carries the actionable outcome. */ };
    ws.onclose = event => {
      clearTimeout(timeout);
      if (!settled) {
        settled = true;
        reject(new Error(event.code === 1008 ? 'Google rejected this session. Check your key, model access, and quota.' : 'Could not connect to Gemini Live. Check your key, access, and connection.'));
      } else if (ready && !manuallyStopped && socket === ws && state === 'live') {
        if (resumeHandle && (goingAway || event.code !== 1008) && retries < 2) reconnect();
        else finish('The conversation disconnected. Check your connection or quota and try again.', true);
      }
    };
  });
}
async function reconnect() {
  setState('reconnecting');
  stopPlayback();
  retries++;
  try {
    if (Date.now() - tokenIssuedAt > 25 * 60 * 1000) token = await getToken();
    await connect(resumeHandle);
    if (manuallyStopped) return;
    goingAway = false;
    retries = 0;
    setState('live');
    notice(muted ? 'Microphone muted.' : 'Listening when you are ready.');
  } catch { finish('Could not resume the conversation. Start a new one to continue.', true); }
}
async function start() {
  if (!ui.key.value.trim()) { notice('Add your Gemini API key to get started.'); ui.settings.showModal(); ui.key.focus(); return; }
  if (!navigator.mediaDevices?.getUserMedia || !window.AudioWorkletNode) { notice('Use a current Chrome or Edge browser to enable microphone audio.', true); return; }
  manuallyStopped = false;
  resumeHandle = '';
  goingAway = false;
  retries = 0;
  muted = false;
  updateMute();
  setState('starting');
  notice('Allow microphone access if your browser asks.');
  try {
    await prepareAudio();
    token = await getToken();
    await connect();
    if (socket?.readyState !== WebSocket.OPEN) throw new Error('The connection closed before the conversation started. Please try again.');
    setState('live');
    startedAt = Date.now();
    updateTimer();
    timerId = setInterval(updateTimer, 1000);
    notice('Listening when you are ready.');
  } catch (error) {
    const message = error.name === 'NotAllowedError' ? 'Microphone access was blocked. Allow it in your browser and try again.' : error.name === 'NotFoundError' ? 'No microphone was found. Connect one and try again.' : error.message || 'Could not start. Please try again.';
    await finish(message, true);
  }
}
async function finish(message = 'Conversation ended. Your transcript is still here.', isError = false) {
  manuallyStopped = true;
  if (socket?.readyState === WebSocket.OPEN) {
    try { socket.send(JSON.stringify({ realtimeInput: { audioStreamEnd: true } })); socket.close(); } catch { /* Connection already closed. */ }
  } else if (socket?.readyState === WebSocket.CONNECTING) socket.close();
  socket = null;
  stopPlayback();
  capture?.disconnect();
  stream?.getTracks().forEach(track => track.stop());
  stream = null;
  capture = null;
  micAnalyser = null;
  playbackAnalyser = null;
  if (audioContext) { const oldContext = audioContext; audioContext = null; await oldContext.close().catch(() => {}); }
  token = '';
  clearInterval(timerId);
  timerId = null;
  muted = false;
  updateMute();
  activeMessages = { user: null, model: null };
  setState('idle');
  notice(message, isError);
  if (isError) ui.dot.classList.add('error');
}
function updateTimer() {
  const seconds = Math.floor((Date.now() - startedAt) / 1000);
  ui.timer.textContent = `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`;
}
function levelOf(analyser) {
  if (!analyser) return 0;
  const values = new Float32Array(analyser.fftSize);
  analyser.getFloatTimeDomainData(values);
  let sum = 0;
  for (const value of values) sum += value * value;
  return Math.sqrt(sum / values.length);
}
function drawWave() {
  const canvas = ui.wave;
  const ratio = Math.min(devicePixelRatio || 1, 2);
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  if (canvas.width !== Math.round(width * ratio) || canvas.height !== Math.round(height * ratio)) { canvas.width = width * ratio; canvas.height = height * ratio; }
  const ctx = canvas.getContext('2d');
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  ctx.clearRect(0, 0, width, height);
  const input = muted ? 0 : levelOf(micAnalyser);
  const output = levelOf(playbackAnalyser);
  const activity = Math.min(1, Math.max(input * 8, output * 5));
  ui.waveLabel.textContent = state === 'live' ? (output > .015 ? `${preferences.voice} is speaking` : input > .015 && !muted ? 'Listening to you' : muted ? 'Microphone muted' : 'Listening for your voice') : state === 'reconnecting' ? 'Reconnecting your conversation' : 'Your conversation starts here';
  const count = Math.max(25, Math.floor(width / 6));
  const gap = width / count;
  const now = performance.now() / 190;
  ctx.lineCap = 'round';
  ctx.lineWidth = 2;
  for (let i = 0; i < count; i++) {
    const envelope = Math.sin(Math.PI * i / count) ** .7;
    const variation = .35 + .65 * Math.abs(Math.sin(i * .55 + now) * Math.cos(i * .22 - now * .7));
    const bar = 2 + activity * envelope * variation * (height - 6);
    ctx.strokeStyle = activity > .03 ? '#759b72' : '#b5cbb0';
    ctx.beginPath(); ctx.moveTo(i * gap + 1, (height - bar) / 2); ctx.lineTo(i * gap + 1, (height + bar) / 2); ctx.stroke();
  }
  requestAnimationFrame(drawWave);
}

ui.settingsButton.addEventListener('click', () => ui.settings.showModal());
ui.closeSettings.addEventListener('click', () => ui.settings.close());
ui.settingsForm.addEventListener('submit', event => {
  event.preventDefault();
  preferences.language = ui.language.value.trim() || defaults.language;
  preferences.level = ui.level.value || defaults.level;
  ui.language.value = preferences.language;
  savePreferences(); updatePreferences(); ui.settings.close();
  if (state === 'idle') notice(ui.key.value.trim() ? 'Ready to start when you are.' : 'Add your API key in settings to get started.');
});
ui.voices.forEach(button => button.addEventListener('click', () => { preferences.voice = button.dataset.voice; savePreferences(); updatePreferences(); }));
ui.start.addEventListener('click', () => state === 'live' ? finish() : state === 'idle' && start());
ui.mute.addEventListener('click', () => {
  muted = !muted;
  if (stream) stream.getAudioTracks().forEach(track => { track.enabled = !muted; });
  if (muted) send({ realtimeInput: { audioStreamEnd: true } });
  updateMute();
  notice(muted ? 'Microphone muted.' : 'Listening when you are ready.');
});
ui.clear.addEventListener('click', clearTranscript);
window.addEventListener('beforeunload', () => { manuallyStopped = true; stream?.getTracks().forEach(track => track.stop()); socket?.close(); });
updatePreferences(); updateMute(); drawWave();
