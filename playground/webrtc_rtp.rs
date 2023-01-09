use std::sync::Arc;

use anyhow::Result;

use gstreamer::{Bin, Element, ElementFactory, GhostPad};
use gstreamer_app::{AppSink, AppSinkCallbacks};
use gstreamer_editing_services::{prelude::*, Pipeline, Timeline, UriClip};
use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors,
        media_engine::{MediaEngine, MIME_TYPE_VP8},
        APIBuilder,
    },
    ice_transport::{ice_connection_state::RTCIceConnectionState, ice_server::RTCIceServer},
    interceptor::registry::Registry,
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::{
        track_local_static_rtp::TrackLocalStaticRTP, TrackLocal, TrackLocalWriter,
    },
    Error,
};

#[tokio::main]
async fn main() -> Result<()> {
    gstreamer::init().unwrap();
    gstreamer_editing_services::init().unwrap();

    let mut m = MediaEngine::default();
    m.register_default_codecs()?;
    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut m)?;

    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .build();

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

    let rtp_sender = peer_connection
        .add_track(Arc::clone(&video_track) as Arc<dyn TrackLocal + Send + Sync>)
        .await?;

    tokio::spawn(async move {
        let mut rtcp_buf = vec![0u8; 1500];
        while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
        Result::<()>::Ok(())
    });

    let (done_tx, mut done_rx) = tokio::sync::mpsc::channel::<()>(1);

    let done_tx1 = done_tx.clone();
    peer_connection.on_ice_connection_state_change(Box::new(
        move |connection_state: RTCIceConnectionState| {
            println!("Connection State has changed {}", connection_state);
            if connection_state == RTCIceConnectionState::Failed {
                let _ = done_tx1.try_send(());
            }
            Box::pin(async {})
        },
    ));

    let done_tx2 = done_tx.clone();
    peer_connection.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
        println!("Peer Connection State has changed: {}", s);
        if s == RTCPeerConnectionState::Failed {
            // Wait until PeerConnection has had no network activity for 30 seconds or another failure. It may be reconnected using an ICE Restart.
            // Use webrtc.PeerConnectionStateDisconnected if you are interested in detecting faster timeout.
            // Note that the PeerConnection may come back from PeerConnectionStateDisconnected.
            println!("Peer Connection has gone to failed exiting: Done forwarding");
            let _ = done_tx2.try_send(());
        }

        Box::pin(async {})
    }));

    let line = must_read_stdin()?;
    let desc_data = decode(line.as_str())?;
    let offer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;
    peer_connection.set_remote_description(offer).await?;

    let answer = peer_connection.create_answer(None).await?;
    let mut gather_complete = peer_connection.gathering_complete_promise().await;
    peer_connection.set_local_description(answer).await?;
    let _ = gather_complete.recv().await;

    if let Some(local_desc) = peer_connection.local_description().await {
        let json_str = serde_json::to_string(&local_desc)?;
        let b64 = base64::encode(&json_str);
        println!("{}", b64);
    } else {
        println!("generate local_description failed!");
    }

    let done_tx3 = done_tx.clone();

    // Read RTP packets forever and send them to the WebRTC Client
    tokio::spawn(async move {
        let timeline = Timeline::new_audio_video();
        let layer = timeline.append_layer();
        let pipeline = Pipeline::new();
        pipeline.set_timeline(&timeline).unwrap();
        let clip = UriClip::new("file:///Users/itome/Downloads/bun33s.mp4").unwrap();
        layer.add_clip(&clip).unwrap();

        let bin = Bin::builder().build();
        let sink = AppSink::builder().build();
        let enc = ElementFactory::make("vp8enc").build().unwrap();
        enc.set_property("deadline", 1i64);
        enc.set_property("target-bitrate", 10240000);
        let rtpvp8pay = ElementFactory::make("rtpvp8pay").build().unwrap();
        bin.add_many(&[&enc, &rtpvp8pay, sink.upcast_ref()])
            .unwrap();
        Element::link_many(&[&enc, &rtpvp8pay, sink.upcast_ref()]).unwrap();

        let pad = enc
            .static_pad("sink")
            .expect("Failed to get a static pad from equalizer.");
        let ghost_pad = GhostPad::with_target(Some("sink"), &pad).unwrap();
        ghost_pad.set_active(true).unwrap();
        bin.add_pad(&ghost_pad).unwrap();

        sink.set_callbacks(
            AppSinkCallbacks::builder()
                .new_sample(move |appsink| {
                    if let Ok(sample) = appsink.pull_sample() {
                        let buffer = sample.buffer().unwrap();
                        let map = buffer.map_readable().unwrap();
                        let data = map.as_slice();
                        let result = tokio::runtime::Builder::new_multi_thread()
                            .enable_all()
                            .build()
                            .unwrap()
                            .block_on(async { video_track.write(data).await });
                        if let Err(err) = result {
                            if Error::ErrClosedPipe == err {
                                // The peerConnection has been closed.
                            } else {
                                println!("video_track write err: {}", err);
                            }
                        }
                    }

                    Ok(gstreamer::FlowSuccess::Ok)
                })
                .build(),
        );

        pipeline.preview_set_video_sink(Some(&bin));

        // Start playing
        pipeline
            .set_state(gstreamer::State::Playing)
            .expect("Unable to set the pipeline to the `Playing` state");

        // Wait until error or EOS
        let bus = pipeline.bus().unwrap();
        for msg in bus.iter_timed(gstreamer::ClockTime::NONE) {
            use gstreamer::MessageView;

            match msg.view() {
                MessageView::Eos(..) => break,
                MessageView::Error(err) => {
                    println!(
                        "Error from {:?}: {} ({:?})",
                        err.src().map(|s| s.path_string()),
                        err.error(),
                        err.debug()
                    );
                    break;
                }
                _ => (),
            }
        }

        // Shutdown pipeline
        pipeline
            .set_state(gstreamer::State::Null)
            .expect("Unable to set the pipeline to the `Null` state");

        let done_tx4 = done_tx.clone();
        let _ = done_tx4.try_send(());
    });

    println!("Press ctrl-c to stop");
    tokio::select! {
        _ = done_rx.recv() => {
            println!("received done signal!");
        }
        _ = tokio::signal::ctrl_c() => {
            println!("");
        }
    };

    peer_connection.close().await?;

    Ok(())
}

fn must_read_stdin() -> Result<String> {
    let mut line = String::new();

    std::io::stdin().read_line(&mut line)?;
    line = line.trim().to_owned();
    println!();

    Ok(line)
}

fn decode(s: &str) -> Result<String> {
    let b = base64::decode(s)?;

    //if COMPRESS {
    //    b = unzip(b)
    //}

    let s = String::from_utf8(b)?;
    Ok(s)
}
