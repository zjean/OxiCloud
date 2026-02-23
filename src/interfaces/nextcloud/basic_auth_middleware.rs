use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::Engine;
use std::sync::Arc;

use crate::common::di::AppState;
use crate::interfaces::middleware::auth::CurrentUser;

#[derive(Debug, thiserror::Error)]
pub enum NextcloudAuthError {
    #[error("Unauthorized")]
    Unauthorized,
    #[error("Nextcloud services unavailable")]
    ServiceUnavailable,
    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for NextcloudAuthError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            NextcloudAuthError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized"),
            NextcloudAuthError::ServiceUnavailable => {
                (StatusCode::SERVICE_UNAVAILABLE, "Nextcloud unavailable")
            }
            NextcloudAuthError::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
            }
        };

        (status, body).into_response()
    }
}

pub async fn basic_auth_middleware(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Result<Response, NextcloudAuthError> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(NextcloudAuthError::Unauthorized)?;

    let (username, password) =
        parse_basic_auth(auth_header).ok_or(NextcloudAuthError::Unauthorized)?;

    let nextcloud = state
        .nextcloud
        .as_ref()
        .ok_or(NextcloudAuthError::ServiceUnavailable)?;

    match nextcloud.app_passwords.validate(&username, &password).await {
        Ok(Some(current_user)) => {
            request.extensions_mut().insert(CurrentUser {
                id: current_user.id,
                username: current_user.username,
                email: current_user.email,
                role: current_user.role,
            });
            Ok(next.run(request).await)
        }
        Ok(None) => Err(NextcloudAuthError::Unauthorized),
        Err(e) => {
            tracing::error!("Nextcloud Basic Auth validation error: {}", e);
            Err(NextcloudAuthError::Internal(e.to_string()))
        }
    }
}

fn parse_basic_auth(header_value: &str) -> Option<(String, String)> {
    let mut parts = header_value.splitn(2, ' ');
    let scheme = parts.next()?.trim();
    let encoded = parts.next()?.trim();

    if !scheme.eq_ignore_ascii_case("Basic") {
        return None;
    }

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (user, pass) = decoded.split_once(':')?;

    Some((user.to_string(), pass.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_basic_auth() {
        let encoded = base64::engine::general_purpose::STANDARD.encode("alice:secret123");
        let header = format!("Basic {}", encoded);
        let (user, pass) = parse_basic_auth(&header).expect("should parse");
        assert_eq!(user, "alice");
        assert_eq!(pass, "secret123");
    }

    #[test]
    fn test_parse_basic_auth_with_colon_in_password() {
        let encoded = base64::engine::general_purpose::STANDARD.encode("user:pass:with:colons");
        let header = format!("Basic {}", encoded);
        let (user, pass) = parse_basic_auth(&header).expect("should parse");
        assert_eq!(user, "user");
        assert_eq!(pass, "pass:with:colons");
    }

    #[test]
    fn test_parse_basic_auth_bearer_scheme_rejected() {
        let encoded = base64::engine::general_purpose::STANDARD.encode("user:pass");
        let header = format!("Bearer {}", encoded);
        assert!(parse_basic_auth(&header).is_none());
    }

    #[test]
    fn test_parse_basic_auth_missing_colon() {
        let encoded = base64::engine::general_purpose::STANDARD.encode("nocolon");
        let header = format!("Basic {}", encoded);
        assert!(parse_basic_auth(&header).is_none());
    }

    #[test]
    fn test_parse_basic_auth_invalid_base64() {
        assert!(parse_basic_auth("Basic not-valid-base64!!!").is_none());
    }

    #[test]
    fn test_parse_basic_auth_case_insensitive_scheme() {
        let encoded = base64::engine::general_purpose::STANDARD.encode("user:pass");
        let header = format!("BASIC {}", encoded);
        let result = parse_basic_auth(&header);
        assert!(result.is_some());
    }
}
