use anyhow::Result;
use gstreamer_editing_services::{prelude::*, Timeline, UriClip};

pub struct ProjectManager {
    timeline: Timeline,
}

impl ProjectManager {
    pub fn new() -> Self {
        let timeline = Timeline::new_audio_video();
        Self { timeline }
    }

    pub fn add_uri_clip(&self, uri: &str) -> Result<()> {
        let clip = UriClip::new(uri)?;
        if self.timeline.layers().len() == 0 {
            self.timeline.append_layer();
        }
        let layer = match self.timeline.layer(0) {
            Some(layer) => layer,
            None => self.timeline.append_layer(),
        };
        layer.add_clip(&clip)?;
        Ok(())
    }
}
