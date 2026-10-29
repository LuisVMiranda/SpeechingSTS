use super::*;

#[test]
fn microphone_cues_are_distinct_and_short() {
    assert_ne!(
        cue_sample(Cue::MicrophoneOn, 0.02),
        cue_sample(Cue::MicrophoneOff, 0.02)
    );
    assert_eq!(cue_sample(Cue::MicrophoneOn, 0.1), 0.0);
    assert_eq!(cue_sample(Cue::MicrophoneOff, 0.1), 0.0);
}
#[test]
fn noise_gate_opens_for_speech_and_closes_after_a_short_hold() {
    let mut gate = NoiseGate::new(Some(0.01));
    gate.begin_block(0.10, 480, 48_000.0);
    let speech: Vec<_> = (0..480).map(|_| gate.sample(0.10, 48_000.0)).collect();
    assert!(speech.last().copied().unwrap() > 0.09);
    gate.begin_block(0.001, 480, 48_000.0);
    assert!(gate.sample(0.001, 48_000.0) > 0.0);
    for _ in 0..40 {
        gate.begin_block(0.001, 480, 48_000.0);
        for _ in 0..480 {
            gate.sample(0.001, 48_000.0);
        }
    }
    assert_eq!(gate.sample(0.001, 48_000.0), 0.0);
}
#[test]
fn push_to_talk_and_mute_both_block_capture_when_released() {
    let controls = AudioControls::new(1.0).configure_input(true, Some(-42.0));
    assert!(!controls.capture_enabled());
    controls.talking.store(true, Ordering::Relaxed);
    assert!(controls.capture_enabled());
    controls.muted.store(true, Ordering::Relaxed);
    assert!(!controls.capture_enabled());
}
#[test]
fn downsampling_produces_a_40_ms_chunk() {
    let mut s = Downsampler::new();
    let mut result = None;
    for _ in 0..1920 {
        if let Some(chunk) = s.add(0.5, 48_000.0) {
            result = Some(chunk);
        }
    }
    assert_eq!(result.unwrap().len(), 1920);
}
#[test]
fn queued_audio_can_be_cleared() {
    let controls = AudioControls::new(1.0);
    controls.queue_pcm(&[0, 0, 255, 127]);
    assert_eq!(controls.output.lock().unwrap().len(), 2);
    controls.clear_output();
    assert!(controls.output.lock().unwrap().is_empty());
}
#[test]
fn slower_speech_is_longer_without_lowering_pitch_and_can_be_interrupted() {
    let controls = AudioControls::new(0.75);
    let mut pcm = Vec::new();
    for i in 0..24_000 {
        let value = ((i as f32 * 220.0 * std::f32::consts::TAU / 24_000.0).sin()
            * 0.5
            * i16::MAX as f32) as i16;
        pcm.extend_from_slice(&value.to_le_bytes());
    }
    for chunk in pcm.chunks(1_920) {
        controls.queue_pcm(chunk);
    }
    controls.finish_output();
    let samples: Vec<f32> = controls.output.lock().unwrap().iter().copied().collect();
    assert!(
        (30_500..=33_500).contains(&samples.len()),
        "{} samples",
        samples.len()
    );
    let crossings = samples
        .windows(2)
        .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
        .count();
    let pitch = crossings as f32 * 24_000.0 / samples.len() as f32;
    assert!((210.0..230.0).contains(&pitch), "{pitch} Hz");
    controls.queue_pcm(&pcm[..1_920]);
    controls.clear_output();
    controls.finish_output();
    assert!(controls.output.lock().unwrap().is_empty());
}
#[test]
fn slightly_slower_setting_increases_the_playback_duration() {
    let controls = AudioControls::new(0.82);
    let mut pcm = Vec::new();
    for i in 0..24_000 {
        let value = ((i as f32 * 220.0 * std::f32::consts::TAU / 24_000.0).sin()
            * 0.5
            * i16::MAX as f32) as i16;
        pcm.extend_from_slice(&value.to_le_bytes());
    }
    for chunk in pcm.chunks(1_920) {
        controls.queue_pcm(chunk);
    }
    controls.finish_output();
    let samples = controls.output.lock().unwrap().len();
    assert!((28_000..=30_500).contains(&samples), "{samples} samples");
}
#[test]
fn input_below_target_rate_is_upsampled() {
    let mut s = Downsampler::new();
    let mut result = None;
    for _ in 0..640 {
        if let Some(chunk) = s.add(0.5, 16_000.0) {
            result = Some(chunk);
        }
    }
    assert_eq!(result.unwrap().len(), 1920);
}
