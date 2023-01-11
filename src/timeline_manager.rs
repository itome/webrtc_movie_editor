use std::collections::HashMap;

use crate::timeline::Timeline;
use anyhow::Result;
use tokio::sync::mpsc::Sender;

#[derive(Debug)]
pub enum Command {
    AddPipeline(String, Sender<Vec<u8>>, Sender<Vec<u8>>),
    AddUriClip(String),
    Play(String),
    Pause(String),
}

pub struct TimelineManager {
    timelines: HashMap<String, Timeline>,
}

impl TimelineManager {
    pub fn new() -> Self {
        Self {
            timelines: HashMap::new(),
        }
    }

    pub fn handle_command(&mut self, command: Command) -> Result<()> {
        match command {
            Command::AddPipeline(id, video_tx, audio_tx) => {
                self.add_timeline(id, video_tx, audio_tx)
            }
            Command::AddUriClip(uri) => self.add_uri_clip(uri),
            Command::Play(id) => self.timelines.get(&id).unwrap().play(),
            Command::Pause(id) => self.timelines.get(&id).unwrap().pause(),
        }
    }

    fn add_timeline(
        &mut self,
        id: String,
        video_tx: Sender<Vec<u8>>,
        audio_tx: Sender<Vec<u8>>,
    ) -> Result<()> {
        let timeline = Timeline::new(video_tx, audio_tx)?;
        self.timelines.insert(id, timeline);
        Ok(())
    }

    fn add_uri_clip(&mut self, uri: String) -> Result<()> {
        for timeline in self.timelines.values_mut() {
            timeline.add_uri_clip(uri.clone())?;
        }
        Ok(())
    }
}
