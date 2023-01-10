use std::marker::Send;
use std::sync::Arc;

use anyhow::Result;
use uuid::Uuid;
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
    pub id: String,
    peer_connection: Arc<RTCPeerConnection>,
    video_track: Arc<TrackLocalStaticRTP>,
}

impl Connection {
    pub async fn new(api: Arc<API>) -> Result<Self> {
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

        Ok(Connection {
            id: Uuid::new_v4().to_string(),
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

    pub async fn write(&self, data: &[u8]) -> Result<()> {
        self.video_track.write(data).await?;
        Ok(())
    }
}
