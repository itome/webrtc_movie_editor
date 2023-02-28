mod connection;
mod connection_pool;
mod timeline;
mod timeline_manager;

use std::{marker::Send, net::SocketAddr, sync::Arc, thread};

use anyhow::Result;
use axum::{
    extract::Extension, http::StatusCode, response::IntoResponse, routing::post, Json, Router,
};
use axum_macros::debug_handler;
use connection_pool::ConnectionPool;
use serde_json::json;
use timeline_manager::{Command, TimelineManager};
use tokio::sync::mpsc::{self, Sender};
use tower_http::cors::CorsLayer;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

#[derive(Clone)]
struct AppContext {
    connection_pool: Arc<ConnectionPool>,
    tx: Arc<Sender<Command>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let (tx, mut rx) = mpsc::channel::<Command>(100);
    let tx = Arc::new(tx);
    let connection_pool = Arc::new(ConnectionPool::new(tx.clone())?);
    let context = AppContext {
        connection_pool: connection_pool.clone(),
        tx: tx.clone(),
    };

    let app = Router::new()
        .route("/signaling", post(create_user))
        .layer(Extension(context))
        .layer(CorsLayer::permissive());

    thread::spawn(move || {
        gstreamer::init().unwrap();
        gstreamer_editing_services::init().unwrap();
        let mut manager = TimelineManager::new();
        while let Some(command) = rx.blocking_recv() {
            manager.handle_command(command).unwrap();
        }
    });

    let addr = SocketAddr::from(([127, 0, 0, 1], 8000));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}

#[debug_handler]
async fn create_user(
    Extension(context): Extension<AppContext>,
    Json(payload): Json<RTCSessionDescription>,
) -> impl IntoResponse {
    let (video_tx, video_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(100);
    let (audio_tx, audio_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(100);
    let (id, description) = context
        .connection_pool
        .create_connection(payload)
        .await
        .unwrap();
    context
        .connection_pool
        .set_video_buffer_handler(id.clone(), video_rx)
        .await
        .unwrap();
    context
        .connection_pool
        .set_audio_buffer_handler(id.clone(), audio_rx)
        .await
        .unwrap();
    context
        .tx
        .send(Command::AddPipeline(id.clone(), video_tx, audio_tx))
        .await
        .unwrap();

    (StatusCode::CREATED, Json(json!(description)))
}
