use std::collections::HashMap;

use anyhow::Result;
use gstreamer::{Bin, Element, ElementFactory, GhostPad, State};
use gstreamer_app::{AppSink, AppSinkCallbacks};
use gstreamer_editing_services::{prelude::*, Pipeline, Timeline, UriClip};
use tokio::sync::mpsc::Sender;

#[derive(Debug)]
pub enum EditorCommand {
    AddPipeline(String, Sender<Vec<u8>>),
    AddUriClip(String),
    Play(String),
    Pause(String),
}

pub struct ProjectManager {
    timeline: Timeline,
    pipelines: HashMap<String, Pipeline>,
}

impl ProjectManager {
    pub fn new() -> Self {
        gstreamer::init().unwrap();
        gstreamer_editing_services::init().unwrap();
        let timeline = Timeline::new_audio_video();
        Self {
            timeline,
            pipelines: HashMap::new(),
        }
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

    pub fn handle_command(&mut self, command: EditorCommand) -> Result<()> {
        match command {
            EditorCommand::AddPipeline(id, tx) => self.add_pipeline(id, tx),
            EditorCommand::AddUriClip(uri) => self.add_uri_clip(uri.as_str()),
            EditorCommand::Play(id) => self.play(id),
            EditorCommand::Pause(id) => self.pause(id),
        }
    }

    fn add_pipeline(&mut self, id: String, tx: Sender<Vec<u8>>) -> Result<()> {
        let pipeline = Pipeline::new();
        let (video_sink, appsink) = Self::create_app_sink()?;
        pipeline.preview_set_video_sink(Some(&video_sink));
        pipeline.set_timeline(&self.timeline)?;
        appsink.set_callbacks(
            AppSinkCallbacks::builder()
                .new_sample(move |appsink| {
                    if let Ok(sample) = appsink.pull_sample() {
                        if let Some(buffer) = sample.buffer() {
                            if let Ok(map) = buffer.map_readable() {
                                let _ = tokio::runtime::Builder::new_multi_thread()
                                    .enable_all()
                                    .build()
                                    .unwrap()
                                    .block_on(async {
                                        tx.send(map.as_slice().to_vec()).await.unwrap();
                                    });
                            }
                        }
                    }
                    Ok(gstreamer::FlowSuccess::Ok)
                })
                .build(),
        );
        self.pipelines.insert(id, pipeline);
        Ok(())
    }

    fn play(&self, id: String) -> Result<()> {
        self.pipelines.get(&id).unwrap().set_state(State::Playing)?;
        Ok(())
    }

    fn pause(&self, id: String) -> Result<()> {
        self.pipelines.get(&id).unwrap().set_state(State::Paused)?;
        Ok(())
    }

    fn create_app_sink() -> Result<(Element, AppSink)> {
        let sink = AppSink::builder().build();
        let enc = ElementFactory::make("vp8enc").build()?;
        enc.set_property("deadline", 1i64);
        enc.set_property("target-bitrate", 10 * 1024 * 10000);
        let rtpvp8pay = ElementFactory::make("rtpvp8pay").build()?;

        let bin = Bin::builder().build();
        bin.add_many(&[&enc, &rtpvp8pay, sink.upcast_ref()])?;
        Element::link_many(&[&enc, &rtpvp8pay, sink.upcast_ref()])?;

        let pad = enc
            .static_pad("sink")
            .expect("Failed to get a static pad from equalizer.");
        let ghost_pad = GhostPad::with_target(Some("sink"), &pad)?;
        ghost_pad.set_active(true)?;
        bin.add_pad(&ghost_pad)?;

        Ok((bin.upcast(), sink))
    }
}
