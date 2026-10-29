export class AudioSession {
  constructor(events) {
    this.events = events;
    this.sources = new Set();
    this.nextPlayback = 0;
    this.ready = false;
    this.pressed = false;
    this.cancelled = false;
  }
  async start(settings) {
    this.settings = settings;
    this.stream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true } });
    if (this.cancelled) { this.cleanup(); throw new Error('Cancelled'); }
    this.context = new AudioContext();
    await this.context.audioWorklet.addModule(new URL('worklet.js', import.meta.url));
    if (this.cancelled) { this.cleanup(); throw new Error('Cancelled'); }
    const url = new URL('api/live', document.baseURI);
    url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
    this.socket = new WebSocket(url);
    this.socket.binaryType = 'arraybuffer';
    this.socket.onopen = () => this.socket.send(JSON.stringify(settings));
    this.socket.onmessage = event => this.receive(event.data);
    this.socket.onerror = () => this.events.error('Could not establish the live connection.');
    this.socket.onclose = () => { this.cleanup(); this.events.closed(); };
  }
  async receive(data) {
    if (data instanceof ArrayBuffer) { if (!this.cancelled) this.play(data); return; }
    let event;
    try { event = JSON.parse(data); } catch { return; }
    if (this.cancelled && event.type !== 'error') return;
    if (event.type === 'ready') {
      try { await this.startCapture(); this.ready = true; }
      catch { this.events.error('Microphone capture could not start.'); this.stop(); return; }
    }
    if (event.type === 'interrupted') this.interrupt();
    this.events.message(event);
  }
  async startCapture() {
    if (!this.context || !this.stream) return;
    await this.context.resume();
    this.micSource = this.context.createMediaStreamSource(this.stream);
    this.worklet = new AudioWorkletNode(this.context, 'speeching-microphone');
    this.worklet.port.onmessage = event => {
      const { pcm, rms } = event.data;
      if (this.socket?.readyState !== WebSocket.OPEN || this.socket.bufferedAmount > 262144) return;
      if (this.settings.input_mode === 'push' && !this.pressed) return;
      this.socket.send(pcm);
      this.events.level(rms);
    };
    const silence = this.context.createGain();
    silence.gain.value = 0;
    this.micSource.connect(this.worklet).connect(silence).connect(this.context.destination);
  }
  press(active) {
    if (!this.ready || this.settings.input_mode !== 'push' || this.pressed === active) return;
    this.pressed = active;
    this.socket.send(active ? 'press' : 'release');
  }
  play(bytes) {
    if (!this.context || bytes.byteLength % 2) return;
    const pcm = new Int16Array(bytes);
    const buffer = this.context.createBuffer(1, pcm.length, 24000);
    const output = buffer.getChannelData(0);
    for (let index = 0; index < pcm.length; index++) output[index] = pcm[index] / 32768;
    const now = this.context.currentTime;
    if (this.nextPlayback - now > 15) return;
    const source = this.context.createBufferSource();
    source.buffer = buffer;
    source.connect(this.context.destination);
    source.onended = () => { this.sources.delete(source); if (this.sources.size === 0) this.events.listening(); };
    this.sources.add(source);
    this.nextPlayback = Math.max(now, this.nextPlayback);
    source.start(this.nextPlayback);
    this.nextPlayback += buffer.duration;
    this.events.speaking();
  }
  interrupt() {
    for (const source of this.sources) { try { source.stop(); } catch { /* Already finished. */ } }
    this.sources.clear();
    this.nextPlayback = 0;
  }
  stop() {
    if (this.cancelled) return;
    this.cancelled = true;
    if (this.socket?.readyState === WebSocket.OPEN) { this.socket.send('stop'); this.cleanup(); }
    else {
      if (this.socket) this.socket.close();
      else this.events.closed();
      this.cleanup();
    }
  }
  cleanup() {
    this.ready = false;
    this.interrupt();
    this.stream?.getTracks().forEach(track => track.stop());
    this.context?.close().catch(() => {});
    this.stream = null;
    this.context = null;
  }
}
