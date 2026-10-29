use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
};
use libsonic::SonicStream;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::mpsc;

const TARGET_RATE: f32 = 24_000.0;
const CHUNK_SAMPLES: usize = 960;
const MAX_OUTPUT_SAMPLES: usize = 24_000 * 20;

#[derive(Clone)]
pub struct AudioControls {
    pub muted: Arc<AtomicBool>,
    pub talking: Arc<AtomicBool>,
    manual_input: bool,
    gate_threshold: Option<f32>,
    pub input_level: Arc<AtomicU32>,
    pub output_level: Arc<AtomicU32>,
    pub output: Arc<Mutex<VecDeque<f32>>>,
    processor: Arc<Mutex<SpeechProcessor>>,
}
impl AudioControls {
    pub fn new(speed: f32) -> Self {
        Self {
            muted: Arc::new(AtomicBool::new(false)),
            talking: Arc::new(AtomicBool::new(false)),
            manual_input: false,
            gate_threshold: None,
            input_level: Arc::new(AtomicU32::new(0f32.to_bits())),
            output_level: Arc::new(AtomicU32::new(0f32.to_bits())),
            output: Arc::new(Mutex::new(VecDeque::new())),
            processor: Arc::new(Mutex::new(SpeechProcessor::new(speed))),
        }
    }
    pub fn configure_input(mut self, manual: bool, gate_db: Option<f32>) -> Self {
        self.manual_input = manual;
        self.gate_threshold = gate_db.map(|db| 10.0f32.powf(db.clamp(-65.0, -20.0) / 20.0));
        self
    }
    fn capture_enabled(&self) -> bool {
        !self.muted.load(Ordering::Relaxed)
            && (!self.manual_input || self.talking.load(Ordering::Relaxed))
    }
    pub fn level(value: &AtomicU32) -> f32 {
        f32::from_bits(value.load(Ordering::Relaxed))
    }
    pub fn clear_output(&self) {
        if let Ok(mut processor) = self.processor.lock() {
            *processor = SpeechProcessor::new(processor.speed);
        }
        if let Ok(mut q) = self.output.lock() {
            q.clear();
        }
        self.output_level.store(0.0f32.to_bits(), Ordering::Relaxed);
    }
    pub fn queue_pcm(&self, bytes: &[u8]) {
        let samples: Vec<f32> = bytes
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0)
            .collect();
        if let Ok(mut processor) = self.processor.lock() {
            let output = processor.push(&samples);
            self.append_output(output);
        }
    }
    pub fn finish_output(&self) {
        if let Ok(mut processor) = self.processor.lock() {
            let output = processor.finish();
            self.append_output(output);
        }
    }
    fn append_output(&self, samples: Vec<f32>) {
        if let Ok(mut queue) = self.output.lock() {
            let remaining = MAX_OUTPUT_SAMPLES.saturating_sub(queue.len());
            queue.extend(samples.into_iter().take(remaining));
        }
    }
}

struct SpeechProcessor {
    stream: SonicStream,
    speed: f32,
    pending: bool,
}
impl SpeechProcessor {
    fn new(speed: f32) -> Self {
        let mut stream = SonicStream::new(TARGET_RATE as u32, 1).expect("valid mono sample rate");
        let speed = speed.clamp(0.7, 1.4);
        stream.set_speed(speed);
        Self {
            stream,
            speed,
            pending: false,
        }
    }
    fn push(&mut self, samples: &[f32]) -> Vec<f32> {
        if !samples.is_empty() {
            self.pending = true;
            self.stream.write_f32(samples);
        }
        self.stream.read_f32(self.stream.available_frames())
    }
    fn finish(&mut self) -> Vec<f32> {
        if !self.pending {
            return Vec::new();
        }
        self.stream.flush();
        let result = self.stream.read_f32(self.stream.available_frames());
        *self = Self::new(self.speed);
        result
    }
}

#[derive(Clone, Copy)]
pub enum Cue {
    Start,
    End,
    MicrophoneOn,
    MicrophoneOff,
}

// Play a short, local chime on its own output stream; never open the microphone.
pub fn play_cue(cue: Cue) {
    std::thread::spawn(move || {
        let Some(device) = cpal::default_host().default_output_device() else {
            return;
        };
        let Ok(supported) = device.default_output_config() else {
            return;
        };
        let config: StreamConfig = supported.clone().into();
        let result = match supported.sample_format() {
            SampleFormat::F32 => cue_stream::<f32>(&device, &config, cue),
            SampleFormat::I16 => cue_stream::<i16>(&device, &config, cue),
            SampleFormat::U16 => cue_stream::<u16>(&device, &config, cue),
            SampleFormat::I32 => cue_stream::<i32>(&device, &config, cue),
            SampleFormat::F64 => cue_stream::<f64>(&device, &config, cue),
            _ => return,
        };
        if let Ok(stream) = result {
            if stream.play().is_ok() {
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    });
}

fn cue_stream<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &StreamConfig,
    cue: Cue,
) -> Result<Stream, cpal::BuildStreamError> {
    let rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;
    let mut index = 0usize;
    device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            for frame in data.chunks_mut(channels) {
                let value = cue_sample(cue, index as f32 / rate);
                for sample in frame {
                    *sample = T::from_sample(value);
                }
                index += 1;
            }
        },
        |_| {},
        None,
    )
}

fn cue_sample(cue: Cue, time: f32) -> f32 {
    let (note_time, length, pitch, volume) = match cue {
        Cue::MicrophoneOn => (time, 0.09, 830.0, 0.065),
        Cue::MicrophoneOff => (time, 0.09, 530.0, 0.065),
        Cue::Start | Cue::End => {
            if time >= 0.28 {
                return 0.0;
            }
            let pitch = match (cue, time < 0.14) {
                (Cue::Start, true) | (Cue::End, false) => 620.0,
                _ => 830.0,
            };
            (time % 0.14, 0.14, pitch, 0.10)
        }
    };
    if note_time >= length {
        return 0.0;
    }
    let envelope = (note_time / 0.012).min(1.0) * ((length - note_time) / 0.05).clamp(0.0, 1.0);
    let angle = std::f32::consts::TAU * pitch * note_time;
    (angle.sin() + 0.16 * (angle * 2.0).sin()) * envelope * volume
}

pub struct AudioStreams {
    _input: Stream,
    _output: Stream,
}
impl AudioStreams {
    pub fn start(controls: AudioControls, tx: mpsc::Sender<Vec<u8>>) -> Result<Self, String> {
        let host = cpal::default_host();
        let input = host
            .default_input_device()
            .ok_or("No microphone was found.")?;
        let output = host
            .default_output_device()
            .ok_or("No audio output device was found.")?;
        let in_supported = input
            .default_input_config()
            .map_err(|e| format!("Microphone: {e}"))?;
        let out_supported = output
            .default_output_config()
            .map_err(|e| format!("Audio output: {e}"))?;
        let in_config: StreamConfig = in_supported.clone().into();
        let out_config: StreamConfig = out_supported.clone().into();
        let input_stream = build_input(
            &input,
            &in_config,
            in_supported.sample_format(),
            controls.clone(),
            tx,
        )?;
        let output_stream = build_output(
            &output,
            &out_config,
            out_supported.sample_format(),
            controls,
        )?;
        input_stream
            .play()
            .map_err(|e| format!("Could not start microphone: {e}"))?;
        output_stream
            .play()
            .map_err(|e| format!("Could not start audio output: {e}"))?;
        Ok(Self {
            _input: input_stream,
            _output: output_stream,
        })
    }
}

fn build_input(
    device: &cpal::Device,
    config: &StreamConfig,
    format: SampleFormat,
    controls: AudioControls,
    tx: mpsc::Sender<Vec<u8>>,
) -> Result<Stream, String> {
    match format {
        SampleFormat::F32 => input_stream::<f32>(device, config, controls, tx),
        SampleFormat::I16 => input_stream::<i16>(device, config, controls, tx),
        SampleFormat::U16 => input_stream::<u16>(device, config, controls, tx),
        SampleFormat::I32 => input_stream::<i32>(device, config, controls, tx),
        SampleFormat::F64 => input_stream::<f64>(device, config, controls, tx),
        other => Err(format!("Unsupported microphone format: {other:?}")),
    }
}

fn build_output(
    device: &cpal::Device,
    config: &StreamConfig,
    format: SampleFormat,
    controls: AudioControls,
) -> Result<Stream, String> {
    match format {
        SampleFormat::F32 => output_stream::<f32>(device, config, controls),
        SampleFormat::I16 => output_stream::<i16>(device, config, controls),
        SampleFormat::U16 => output_stream::<u16>(device, config, controls),
        SampleFormat::I32 => output_stream::<i32>(device, config, controls),
        SampleFormat::F64 => output_stream::<f64>(device, config, controls),
        other => Err(format!("Unsupported output format: {other:?}")),
    }
}

struct Downsampler {
    phase: f32,
    sum: f32,
    count: u32,
    chunk: Vec<u8>,
}

// A gentle downward gate with a short attack and a hold so speech is not chopped
// between syllables. The threshold is a microphone level, not speech recognition.
struct NoiseGate {
    threshold: Option<f32>,
    hold_samples: usize,
    gain: f32,
}
impl NoiseGate {
    fn new(threshold: Option<f32>) -> Self {
        Self {
            threshold,
            hold_samples: 0,
            gain: if threshold.is_some() { 0.0 } else { 1.0 },
        }
    }
    fn begin_block(&mut self, rms: f32, frames: usize, rate: f32) {
        let Some(threshold) = self.threshold else {
            return;
        };
        if rms >= threshold {
            self.hold_samples = (rate * 0.20) as usize;
        } else {
            self.hold_samples = self.hold_samples.saturating_sub(frames);
        }
    }
    fn sample(&mut self, value: f32, rate: f32) -> f32 {
        if self.threshold.is_none() {
            return value;
        }
        let open = self.hold_samples > 0;
        let step = if open {
            1.0 / (rate * 0.004)
        } else {
            1.0 / (rate * 0.05)
        };
        self.gain = if open {
            (self.gain + step).min(1.0)
        } else {
            (self.gain - step).max(0.0)
        };
        value * self.gain
    }
}
impl Downsampler {
    fn new() -> Self {
        Self {
            phase: 0.0,
            sum: 0.0,
            count: 0,
            chunk: Vec::with_capacity(CHUNK_SAMPLES * 2),
        }
    }
    fn add(&mut self, value: f32, source_rate: f32) -> Option<Vec<u8>> {
        self.sum += value;
        self.count += 1;
        self.phase += TARGET_RATE / source_rate;
        if self.phase >= 1.0 {
            let value = (self.sum / self.count as f32).clamp(-1.0, 1.0);
            let pcm = (value * i16::MAX as f32) as i16;
            let mut full = None;
            while self.phase >= 1.0 {
                self.chunk.extend_from_slice(&pcm.to_le_bytes());
                self.phase -= 1.0;
                if self.chunk.len() >= CHUNK_SAMPLES * 2 {
                    full = Some(std::mem::replace(
                        &mut self.chunk,
                        Vec::with_capacity(CHUNK_SAMPLES * 2),
                    ));
                }
            }
            self.sum = 0.0;
            self.count = 0;
            if full.is_some() {
                return full;
            }
        }
        None
    }
}

fn input_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    controls: AudioControls,
    tx: mpsc::Sender<Vec<u8>>,
) -> Result<Stream, String>
where
    T: SizedSample + Sample,
    f32: FromSample<T>,
{
    let channels = config.channels as usize;
    let rate = config.sample_rate.0 as f32;
    let mut downsample = Downsampler::new();
    let mut gate = NoiseGate::new(controls.gate_threshold);
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                if !controls.capture_enabled() {
                    downsample = Downsampler::new();
                    gate = NoiseGate::new(controls.gate_threshold);
                    controls
                        .input_level
                        .store(0.0f32.to_bits(), Ordering::Relaxed);
                    return;
                }
                let mut energy = 0.0f32;
                let mut frames = 0;
                for frame in data.chunks(channels) {
                    let sample = frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>()
                        / frame.len() as f32;
                    energy += sample * sample;
                    frames += 1;
                }
                let rms = if frames > 0 {
                    (energy / frames as f32).sqrt()
                } else {
                    0.0
                };
                gate.begin_block(rms, frames, rate);
                for frame in data.chunks(channels) {
                    let sample = frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>()
                        / frame.len() as f32;
                    if let Some(chunk) = downsample.add(gate.sample(sample, rate), rate) {
                        if controls.capture_enabled() {
                            let _ = tx.try_send(chunk);
                        }
                    }
                }
                let level = if !controls.capture_enabled() || frames == 0 {
                    0.0
                } else {
                    (energy / frames as f32).sqrt()
                };
                controls
                    .input_level
                    .store(level.to_bits(), Ordering::Relaxed);
            },
            move |err| eprintln!("Microphone error: {err}"),
            None,
        )
        .map_err(|e| format!("Could not open microphone: {e}"))
}

fn output_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    controls: AudioControls,
) -> Result<Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let rate = config.sample_rate.0 as f32;
    let mut phase = 1.0f32;
    let mut current = 0.0f32;
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                let Ok(mut queue) = controls.output.lock() else {
                    for sample in data {
                        *sample = T::from_sample(0.0);
                    }
                    return;
                };
                let mut energy = 0.0;
                let mut frames = 0;
                for frame in data.chunks_mut(channels) {
                    while phase >= 1.0 {
                        current = queue.pop_front().unwrap_or(0.0);
                        phase -= 1.0;
                    }
                    for sample in frame.iter_mut() {
                        *sample = T::from_sample(current);
                    }
                    energy += current * current;
                    frames += 1;
                    phase += TARGET_RATE / rate;
                }
                let level = if frames > 0 {
                    (energy / frames as f32).sqrt()
                } else {
                    0.0
                };
                controls
                    .output_level
                    .store(level.to_bits(), Ordering::Relaxed);
            },
            move |err| eprintln!("Audio output error: {err}"),
            None,
        )
        .map_err(|e| format!("Could not open audio output: {e}"))
}

#[cfg(test)]
mod tests;
