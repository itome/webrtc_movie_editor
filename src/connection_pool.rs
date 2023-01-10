use std::sync::{Arc, Mutex};

use anyhow::Result;
use tokio::sync::mpsc::Receiver;
use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors, media_engine::MediaEngine, APIBuilder,
        API,
    },
    interceptor::registry::Registry,
    peer_connection::sdp::session_description::RTCSessionDescription,
};

use crate::connection::Connection;

pub struct ConnectionPool {
    connections: Arc<Mutex<Vec<Arc<Connection>>>>,
    api: Arc<API>,
}

impl ConnectionPool {
    pub fn new() -> Result<Self> {
        let mut media_engine = MediaEngine::default();
        media_engine.register_default_codecs()?;
        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine)?;

        let api = Arc::new(
            APIBuilder::new()
                .with_media_engine(media_engine)
                .with_interceptor_registry(registry)
                .build(),
        );

        Ok(ConnectionPool {
            api,
            connections: Arc::new(Mutex::new(vec![])),
        })
    }

    pub async fn create_connection(
        &self,
        offer: RTCSessionDescription,
    ) -> Result<(String, RTCSessionDescription)> {
        let connection = Connection::new(self.api.clone()).await?;
        let description = connection.connect(offer).await?;
        let id = connection.id.clone();
        self.connections.lock().unwrap().push(Arc::new(connection));
        Ok((id, description))
    }

    pub async fn handle_sample(&self, id: String, mut rx: Receiver<Vec<u8>>) -> Result<()> {
        let connection = self
            .connections
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .clone();
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                connection.write(message.as_ref()).await.unwrap();
            }
        });
        Ok(())
    }
}
