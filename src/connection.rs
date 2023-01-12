use std::marker::Send;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::sync::mpsc::Sender;
use uuid::Uuid;
use webrtc::{
    api::{
        media_engine::{MIME_TYPE_OPUS, MIME_TYPE_VP8},
        API,
    },
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

use crate::timeline_manager::Command;

#[derive(Serialize, Deserialize)]
struct CommandJson {
    name: String,
    payload: Option<Map<String, Value>>,
}

pub struct Connection {
    pub id: String,
    peer_connection: Arc<RTCPeerConnection>,
    video_track: Arc<TrackLocalStaticRTP>,
    audio_track: Arc<TrackLocalStaticRTP>,
}

impl Connection {
    pub async fn new(api: Arc<API>, tx: Arc<Sender<Command>>) -> Result<Self> {
        let id = Uuid::new_v4().to_string();
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

        let audio_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_owned(),
                ..Default::default()
            },
            "audio".to_owned(),
            "webrtc-rs".to_owned(),
        ));
        peer_connection
            .add_track(Arc::clone(&video_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await?;
        peer_connection
            .add_track(Arc::clone(&audio_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await?;

        let id2 = id.clone();
        peer_connection.on_data_channel(Box::new(move |data_channel| {
            let id2 = id2.clone();
            let tx = tx.clone();
            Box::pin(async move {
                data_channel.on_open(Box::new(move || {
                    Box::pin(async move {
                        println!("data channel open");
                    })
                }));
                data_channel.on_message(Box::new(move |msg| {
                    let id2 = id2.clone();
                    let tx = tx.clone();
                    let msg = String::from_utf8(msg.data.to_vec()).unwrap();
                    let cmd: CommandJson = serde_json::from_str(msg.as_str()).unwrap();
                    let command = match cmd.name.as_str() {
                        "play" => Some(Command::Play(id2)),
                        "pause" => Some(Command::Pause(id2)),
                        "add_uri_clip" => {
                            let map = cmd.payload.unwrap();
                            let uri = map.get("uri").unwrap().as_str().unwrap();
                            Some(Command::AddUriClip(String::from(uri.clone())))
                        }
                        _ => None,
                    };
                    Box::pin(async move {
                        match command {
                            Some(command) => tx.send(command).await.unwrap(),
                            None => {}
                        }
                    })
                }));
            })
        }));

        Ok(Connection {
            id,
            peer_connection,
            video_track,
            audio_track,
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

    pub async fn write_video_buffer(&self, data: &[u8]) -> Result<()> {
        self.video_track.write(data).await?;
        Ok(())
    }

    pub async fn write_audio_buffer(&self, data: &[u8]) -> Result<()> {
        self.audio_track.write(data).await?;
        Ok(())
    }
}
