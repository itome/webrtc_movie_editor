mod connection;
mod connection_pool;
mod project;

use std::{marker::Send, net::SocketAddr, sync::Arc, thread};

use anyhow::Result;
use axum::{
    extract::Extension, http::StatusCode, response::IntoResponse, routing::post, Json, Router,
};
use axum_macros::debug_handler;
use connection_pool::ConnectionPool;
use project::{EditorCommand, ProjectManager};
use serde_json::json;
use tokio::sync::mpsc::{self, Sender};
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

#[derive(Clone)]
struct AppContext {
    connection_pool: Arc<ConnectionPool>,
    tx: Arc<Sender<EditorCommand>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let (tx, mut rx) = mpsc::channel::<EditorCommand>(100);
    let tx = Arc::new(tx);
    let connection_pool = Arc::new(ConnectionPool::new()?);
    let context = AppContext {
        connection_pool: connection_pool.clone(),
        tx: tx.clone(),
    };

    let app = Router::new()
        .route("/signaling", post(create_user))
        .layer(Extension(context));

    thread::spawn(move || {
        let mut project_manager = ProjectManager::new();
        while let Some(command) = rx.blocking_recv() {
            project_manager.handle_command(command).unwrap();
        }
    });

    let handles = [tokio::spawn(async {
        let addr = SocketAddr::from(([127, 0, 0, 1], 8000));
        tracing::info!("listening on {}", addr);
        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await
            .unwrap();
    })];

    tx.send(EditorCommand::AddUriClip(
        "file:///Users/itome/Downloads/bun33s.mp4".to_string(),
    ))
    .await?;

    futures::future::join_all(handles).await;

    Ok(())
}

#[debug_handler]
async fn create_user(
    Extension(context): Extension<AppContext>,
    Json(payload): Json<RTCSessionDescription>,
) -> impl IntoResponse {
    let (tx, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(100);
    let (id, description) = context
        .connection_pool
        .create_connection(payload)
        .await
        .unwrap();
    context
        .connection_pool
        .handle_sample(id.clone(), rx)
        .await
        .unwrap();
    context
        .tx
        .send(EditorCommand::AddPipeline(id.clone(), tx))
        .await
        .unwrap();
    context.tx.send(EditorCommand::Play(id)).await.unwrap();
    (StatusCode::CREATED, Json(json!(description)))
}
