use anyhow::Result;
use gstreamer::{Bin, Element, ElementFactory, GhostPad, State};
use gstreamer_app::{AppSink, AppSinkCallbacks};
use gstreamer_editing_services::{prelude::*, Pipeline, UriClip};
use tokio::sync::mpsc::Sender;

pub struct Timeline {
    timeline: gstreamer_editing_services::Timeline,
    pipeline: Pipeline,
}

impl Timeline {
    pub fn new(video_tx: Sender<Vec<u8>>, audio_tx: Sender<Vec<u8>>) -> Result<Self> {
        let timeline = gstreamer_editing_services::Timeline::new_audio_video();
        let pipeline = Pipeline::new();
        let (video_sink, video_app_sink) = Self::create_video_sink()?;
        let (audio_sink, audio_app_sink) = Self::create_audio_sink()?;
        pipeline.set_timeline(&timeline)?;
        pipeline.preview_set_video_sink(Some(&video_sink));
        pipeline.preview_set_audio_sink(Some(&audio_sink));

        Self::set_callback(video_app_sink, video_tx);
        Self::set_callback(audio_app_sink, audio_tx);

        let this = Self { timeline, pipeline };
        this.add_uri_clip("file:///Users/itome/Downloads/earth.mp4".to_string())?;
        this.play()?;
        Ok(this)
        // FIXME(itome): Use following
        // Ok(Self { timeline, pipeline };)
    }

    pub fn add_uri_clip(&self, uri: String) -> Result<()> {
        let clip = UriClip::new(&uri)?;
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

    pub fn play(&self) -> Result<()> {
        self.pipeline.set_state(State::Playing)?;
        Ok(())
    }

    pub fn pause(&self) -> Result<()> {
        self.pipeline.set_state(State::Paused)?;
        Ok(())
    }

    fn create_video_sink() -> Result<(Element, AppSink)> {
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

    fn create_audio_sink() -> Result<(Element, AppSink)> {
        let sink = AppSink::builder().build();
        let enc = ElementFactory::make("opusenc").build()?;
        let rtpvp8pay = ElementFactory::make("rtpopuspay").build()?;

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

    fn set_callback(app_sink: AppSink, tx: Sender<Vec<u8>>) {
        app_sink.set_callbacks(
            AppSinkCallbacks::builder()
                .new_sample(move |appsink| {
                    if let Ok(sample) = appsink.pull_sample() {
                        if let Some(buffer) = sample.buffer() {
                            if let Ok(map) = buffer.map_readable() {
                                let rt = tokio::runtime::Runtime::new().unwrap();
                                rt.block_on(async {
                                    tx.send(map.as_slice().to_vec()).await.unwrap();
                                });
                            }
                        }
                    }
                    Ok(gstreamer::FlowSuccess::Ok)
                })
                .build(),
        );
    }
}
