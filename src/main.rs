mod connection;
mod connection_pool;
mod project;

use std::{marker::Send, net::SocketAddr, sync::Arc};

use anyhow::Result;
use axum::{
    extract::Extension, http::StatusCode, response::IntoResponse, routing::post, Json, Router,
};
use axum_macros::debug_handler;
use connection_pool::ConnectionPool;
use project::ProjectManager;
use serde_json::json;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

#[tokio::main]
async fn main() -> Result<()> {
    gstreamer::init()?;
    gstreamer_editing_services::init()?;
    tracing_subscriber::fmt::init();

    let project_manager = Arc::new(ProjectManager::new());
    let connection_pool = Arc::new(ConnectionPool::new()?);
    project_manager.add_uri_clip("file:///Users/itome/Downloads/bun33s.mp4")?;

    let app = Router::new()
        .route("/signaling", post(create_user))
        .layer(Extension(connection_pool));

    let handles = [tokio::spawn(async {
        let addr = SocketAddr::from(([127, 0, 0, 1], 8000));
        tracing::info!("listening on {}", addr);
        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await
            .unwrap();
    })];

    futures::future::join_all(handles).await;

    Ok(())
}

#[debug_handler]
async fn create_user(
    Extension(context): Extension<Arc<ConnectionPool>>,
    Json(payload): Json<RTCSessionDescription>,
) -> impl IntoResponse {
    if let Ok(description) = context.connection_pool.create_connection(payload).await {
        (StatusCode::CREATED, Json(json!(description)))
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "failed to create connection"})),
        )
    }
}
