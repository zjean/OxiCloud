use axum::Json;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use base64::Engine;
use serde_json::json;
use std::sync::Arc;

use crate::common::di::AppState;
use crate::interfaces::middleware::auth::CurrentUser;

pub async fn handle_capabilities_v1(State(state): State<Arc<AppState>>) -> Response {
    Json(capabilities_payload(&state, 1)).into_response()
}

pub async fn handle_capabilities_v2(State(state): State<Arc<AppState>>) -> Response {
    Json(capabilities_payload(&state, 2)).into_response()
}

pub async fn handle_user_info(State(state): State<Arc<AppState>>, user: CurrentUser) -> Response {
    let quota = match state.storage_usage_service.as_ref() {
        Some(service) => match service.get_user_storage_info(&user.id).await {
            Ok((used, total)) => (used, total),
            Err(_) => (0, 0),
        },
        None => (0, 0),
    };

    let free = quota.1.saturating_sub(quota.0);
    let relative = if quota.1 > 0 {
        (quota.0 as f64 / quota.1 as f64) * 100.0
    } else {
        0.0
    };

    Json(json!({
        "ocs": {
            "meta": { "status": "ok", "statuscode": 200, "message": "OK" },
            "data": {
                "enabled": true,
                "id": user.username,
                "displayname": user.username,
                "email": user.email,
                "quota": {
                    "used": quota.0,
                    "total": quota.1,
                    "free": free,
                    "relative": relative
                }
            }
        }
    }))
    .into_response()
}

pub async fn handle_revoke_apppassword(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    headers: axum::http::HeaderMap,
) -> Response {
    let nextcloud = match state.nextcloud.as_ref() {
        Some(nextcloud) => nextcloud,
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    let app_password = match extract_basic_password(&headers) {
        Some(password) => password,
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };

    if let Err(e) = nextcloud.app_passwords.revoke(&app_password).await {
        tracing::warn!("Failed to revoke app password for {}: {}", user.id, e);
    }

    Json(json!({
        "ocs": {
            "meta": { "status": "ok", "statuscode": 200, "message": "OK" },
            "data": {}
        }
    }))
    .into_response()
}

pub async fn handle_notifications_list() -> Response {
    Json(json!({
        "ocs": {
            "meta": { "status": "ok", "statuscode": 200, "message": "OK" },
            "data": []
        }
    }))
    .into_response()
}

pub async fn handle_notifications_push() -> Response {
    Json(json!({
        "ocs": {
            "meta": { "status": "ok", "statuscode": 200, "message": "OK" },
            "data": {}
        }
    }))
    .into_response()
}

fn capabilities_payload(state: &AppState, ocs_version: u8) -> serde_json::Value {
    let statuscode = if ocs_version == 1 { 100 } else { 200 };
    let base_url = state.core.config.base_url();

    json!({
        "ocs": {
            "meta": {
                "status": "ok",
                "statuscode": statuscode,
                "message": "OK"
            },
            "data": {
                "version": {
                    "major": 28,
                    "minor": 0,
                    "micro": 4,
                    "string": "28.0.4",
                    "edition": "",
                    "extendedSupport": false
                },
                "capabilities": {
                    "files": {
                        "bigfilechunking": true
                    },
                    "dav": {
                        "chunking": "1.0"
                    },
                    "checksums": {
                        "preferredUploadType": "SHA1",
                        "supportedTypes": ["SHA1", "MD5"]
                    },
                    "theming": {
                        "name": "Nextcloud",
                        "url": base_url,
                        "logo": format!("{}/logo.png", base_url),
                        "color": "#0082c9",
                        "color-text": "#ffffff",
                        "color-element": "#0082c9",
                        "color-element-bright": "#0082c9",
                        "color-element-dark": "#0082c9",
                        "background": "#0082c9",
                        "background-plain": true,
                        "background-default": true,
                        "logoheader": format!("{}/logo.png", base_url),
                        "favicon": format!("{}/favicon.ico", base_url)
                    }
                }
            }
        }
    })
}

fn extract_basic_password(headers: &axum::http::HeaderMap) -> Option<String> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let mut parts = value.splitn(2, ' ');
    let scheme = parts.next()?.trim();
    let encoded = parts.next()?.trim();
    if !scheme.eq_ignore_ascii_case("Basic") {
        return None;
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (_user, pass) = decoded.split_once(':')?;
    Some(pass.to_string())
}
