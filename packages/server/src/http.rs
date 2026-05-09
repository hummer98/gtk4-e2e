//! HTTP layer (axum). Step 1 only serves `GET /test/info`.

use axum::{extract::State, routing::get, Json, Router};
use std::sync::Arc;

use crate::proto::Info;

/// Shared state injected into axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub info: Arc<Info>,
}

/// Build the router exposed by the in-process server.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/test/info", get(get_info))
        .with_state(state)
}

async fn get_info(State(state): State<AppState>) -> Json<Info> {
    Json((*state.info).clone())
}
