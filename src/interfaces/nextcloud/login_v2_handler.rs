use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json, Response},
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

use crate::common::di::AppState;
use crate::common::errors::DomainError;

pub async fn handle_login_initiate(State(state): State<Arc<AppState>>) -> Response {
    let nextcloud = match state.nextcloud.as_ref() {
        Some(nextcloud) => nextcloud,
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    let base_url = state.core.config.base_url();
    let flow = nextcloud.login_flow.initiate(&base_url);

    Json(json!({
        "poll": {
            "token": flow.poll_token,
            "endpoint": flow.poll_endpoint,
        },
        "login": flow.login_url,
    }))
    .into_response()
}

pub async fn handle_login_poll(State(state): State<Arc<AppState>>, body: String) -> Response {
    let nextcloud = match state.nextcloud.as_ref() {
        Some(nextcloud) => nextcloud,
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    let token = match parse_form_value(&body, "token") {
        Some(token) => token,
        None => return StatusCode::BAD_REQUEST.into_response(),
    };

    match nextcloud.login_flow.poll(&token) {
        Some(result) => Json(json!({
            "server": result.server,
            "loginName": result.login_name,
            "appPassword": result.app_password,
        }))
        .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub async fn handle_login_page(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Response {
    let nextcloud = match state.nextcloud.as_ref() {
        Some(nextcloud) => nextcloud,
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    if !nextcloud.login_flow.flow_exists(&token) {
        return StatusCode::NOT_FOUND.into_response();
    }

    Html(format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <title>Login</title>
</head>
<body>
  <h2>Login to OxiCloud</h2>
  <form method="POST" action="/login/v2/flow/{token}">
    <label>Username: <input name="user" type="text" required /></label><br />
    <label>Password: <input name="password" type="password" required /></label><br />
    <button type="submit">Grant Access</button>
  </form>
</body>
</html>"#
    ))
    .into_response()
}

pub async fn handle_login_submit(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
    body: String,
) -> Response {
    let nextcloud = match state.nextcloud.as_ref() {
        Some(nextcloud) => nextcloud,
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    if !nextcloud.login_flow.flow_exists(&token) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let params = parse_form(&body);
    let username = match params.get("user") {
        Some(value) if !value.is_empty() => value,
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    let password = match params.get("password") {
        Some(value) if !value.is_empty() => value,
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };

    let auth = match state.auth_service.as_ref() {
        Some(auth) => auth,
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    let current_user = match auth
        .auth_application_service
        .verify_credentials(username, password)
        .await
    {
        Ok(user) => user,
        Err(e) => return login_failed_response(e),
    };

    let app_password = match nextcloud
        .app_passwords
        .create(&current_user.id, "Nextcloud")
        .await
    {
        Ok(password) => password,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let base_url = state.core.config.base_url();
    nextcloud
        .login_flow
        .complete(&token, &current_user.username, &base_url, &app_password);

    Html(
        "<html><body><h2>Login successful</h2><p>You can close this window and return to the app.</p></body></html>"
            .to_string(),
    )
    .into_response()
}

fn login_failed_response(_err: DomainError) -> Response {
    Html("<html><body><h2>Login failed</h2><p>Invalid credentials.</p></body></html>".to_string())
        .into_response()
}

fn parse_form(body: &str) -> HashMap<String, String> {
    body.split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            let key = urlencoding::decode(key).ok()?.to_string();
            let value = urlencoding::decode(value).ok()?.to_string();
            Some((key, value))
        })
        .collect()
}

fn parse_form_value(body: &str, key: &str) -> Option<String> {
    parse_form(body).remove(key)
}
