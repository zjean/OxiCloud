use axum::Json;
use axum::response::{IntoResponse, Response};
use serde_json::json;

pub async fn handle_status() -> Response {
    Json(json!({
        "installed": true,
        "maintenance": false,
        "needsDbUpgrade": false,
        "version": "28.0.4.1",
        "versionstring": "28.0.4",
        "productname": "Nextcloud",
        "edition": ""
    }))
    .into_response()
}
