use std::marker::Send;
use std::sync::Arc;

use anyhow::Result;
use gstreamer::{prelude::*, Bin, Element, ElementFactory, GhostPad};
use gstreamer_app::{AppSink, AppSinkCallbacks};
use gstreamer_editing_services::{traits::GESPipelineExt, Pipeline};
use webrtc::{
    api::{media_engine::MIME_TYPE_VP8, API},
    ice_transport::ice_server::RTCIceServer,
    peer_connection::{
        configuration::RTCConfiguration, sdp::session_description::RTCSessionDescription,
        RTCPeerConnection,
    },
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::{
        track_local_static_rtp::TrackLocalStaticRTP, TrackLocal, TrackLocalWriter,
    },
};

pub struct Connection {
    pipeline: Arc<Pipeline>,
    peer_connection: Arc<RTCPeerConnection>,
    video_track: Arc<TrackLocalStaticRTP>,
}

impl Connection {
    pub async fn new(api: Arc<API>) -> Result<Self> {
        let pipeline = Arc::new(Pipeline::new());
        let (video_sink, appsink) = Self::create_app_sink()?;
        pipeline.preview_set_video_sink(Some(&video_sink));

        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };

        let peer_connection = Arc::new(api.new_peer_connection(config).await?);

        let video_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_VP8.to_owned(),
                ..Default::default()
            },
            "video".to_owned(),
            "webrtc-rs".to_owned(),
        ));

        peer_connection
            .add_track(Arc::clone(&video_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await?;

        let track = video_track.clone();
        appsink.set_callbacks(
            AppSinkCallbacks::builder()
                .new_sample(
                    move |appsink| -> Result<gstreamer::FlowSuccess, gstreamer::FlowError> {
                        if let Ok(sample) = appsink.pull_sample() {
                            if let Some(buffer) = sample.buffer() {
                                if let Ok(map) = buffer.map_readable() {
                                    let data = map.as_slice();
                                    let _ = tokio::runtime::Builder::new_multi_thread()
                                        .enable_all()
                                        .build()
                                        .unwrap()
                                        .block_on(async { track.write(data).await });
                                }
                            }
                        }
                        Ok(gstreamer::FlowSuccess::Ok)
                    },
                )
                .build(),
        );

        Ok(Connection {
            pipeline,
            peer_connection,
            video_track,
        })
    }

    pub async fn connect(&self, offer: RTCSessionDescription) -> Result<RTCSessionDescription> {
        self.peer_connection.set_remote_description(offer).await?;
        let answer = self.peer_connection.create_answer(None).await?;
        let mut gather_complete = self.peer_connection.gathering_complete_promise().await;
        self.peer_connection.set_local_description(answer).await?;
        let _ = gather_complete.recv().await;

        if let Some(local_description) = self.peer_connection.local_description().await {
            Ok(local_description)
        } else {
            return Err(anyhow::anyhow!("Failed to get local description"));
        }
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
