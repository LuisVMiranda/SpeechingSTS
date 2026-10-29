use crate::{
    audio::{AudioControls, AudioStreams},
    config::{InputMode, Provider, Settings},
    history::MemoryMessage,
    protocol::{self, Event, Incoming},
};
use connection::run_connection;
use std::{
    sync::{atomic::Ordering, mpsc::Sender},
    thread,
};
use tokio::sync::mpsc;

pub enum Command {
    Stop,
    Mute(bool),
    Talk(bool),
}
pub struct Session {
    commands: mpsc::UnboundedSender<Command>,
    pub audio: AudioControls,
}
impl Session {
    pub fn start(
        settings: Settings,
        key: String,
        history: Vec<MemoryMessage>,
        events: Sender<Event>,
    ) -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let audio = AudioControls::new(settings.speed).configure_input(
            settings.input_mode == InputMode::PushToTalk,
            settings
                .noise_gate_enabled
                .then_some(settings.noise_gate_db),
        );
        let context = ConnectionContext {
            settings,
            key,
            history,
            audio: audio.clone(),
            events,
        };
        thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(_) => {
                    let _ = context
                        .events
                        .send(Event::Error("Could not start the audio engine.".into()));
                    return;
                }
            };
            runtime.block_on(run(context, receiver));
        });
        Self { commands, audio }
    }
    pub fn mute(&self, value: bool) {
        if value {
            self.talk(false);
        }
        self.audio.muted.store(value, Ordering::Relaxed);
        let _ = self.commands.send(Command::Mute(value));
    }
    pub fn talk(&self, value: bool) {
        self.audio.talking.store(value, Ordering::Relaxed);
        let _ = self.commands.send(Command::Talk(value));
    }
    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

struct ConnectionContext {
    settings: Settings,
    key: String,
    history: Vec<MemoryMessage>,
    audio: AudioControls,
    events: Sender<Event>,
}

async fn run(context: ConnectionContext, mut commands: mpsc::UnboundedReceiver<Command>) {
    let (audio_tx, audio_rx) = mpsc::channel(32);
    let _streams = match AudioStreams::start(context.audio.clone(), audio_tx) {
        Ok(streams) => streams,
        Err(error) => {
            let _ = context.events.send(Event::Error(error));
            return;
        }
    };
    run_connection(&context, &mut commands, audio_rx).await;
    let _ = context.events.send(Event::Stopped);
}

mod connection;
mod turns;

#[cfg(test)]
mod tests;
