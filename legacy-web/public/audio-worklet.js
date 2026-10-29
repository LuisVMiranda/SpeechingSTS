class MicrophoneCapture extends AudioWorkletProcessor {
  constructor() {
    super();
    this.phase = 0;
    this.sum = 0;
    this.count = 0;
    this.buffer = new Int16Array(512);
    this.offset = 0;
  }
  process(inputs) {
    const channel = inputs[0]?.[0];
    if (!channel) return true;
    for (const value of channel) {
      this.sum += value;
      this.count++;
      this.phase += 16000 / sampleRate;
      if (this.phase >= 1) {
        const sample = Math.max(-1, Math.min(1, this.sum / this.count));
        this.buffer[this.offset++] = sample < 0 ? sample * 32768 : sample * 32767;
        this.phase -= 1;
        this.sum = 0;
        this.count = 0;
        if (this.offset === this.buffer.length) {
          this.port.postMessage(this.buffer.buffer, [this.buffer.buffer]);
          this.buffer = new Int16Array(512);
          this.offset = 0;
        }
      }
    }
    return true;
  }
}
registerProcessor('microphone-capture', MicrophoneCapture);
