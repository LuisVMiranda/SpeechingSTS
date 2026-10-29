class MicrophoneProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.phase = 0;
    this.sum = 0;
    this.count = 0;
    this.enabled = true;
    this.hangover = 0;
    this.output = [];
    this.port.onmessage = event => { this.enabled = Boolean(event.data.enabled); };
  }
  process(inputs) {
    const input = inputs[0]?.[0];
    if (!input || !this.enabled) return true;
    let power = 0;
    for (const sample of input) power += sample * sample;
    const rms = Math.sqrt(power / input.length);
    this.hangover = rms > 0.008 ? 18 : Math.max(0, this.hangover - 1);
    for (const sample of input) {
      this.sum += sample;
      this.count++;
      this.phase += 24000;
      if (this.phase < sampleRate) continue;
      const filtered = this.hangover ? this.sum / this.count : 0;
      this.output.push(Math.max(-32768, Math.min(32767, Math.round(filtered * 32767))));
      this.phase -= sampleRate;
      this.sum = 0;
      this.count = 0;
    }
    if (this.output.length >= 480) {
      const buffer = new Int16Array(this.output).buffer;
      this.output = [];
      this.port.postMessage({ pcm: buffer, rms }, [buffer]);
    }
    return true;
  }
}
registerProcessor('speeching-microphone', MicrophoneProcessor);
