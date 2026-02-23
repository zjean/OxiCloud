use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{Request, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{any, delete, get, post},
};
use std::sync::Arc;

use crate::common::di::AppState;
use crate::interfaces::middleware::auth::CurrentUser;
use crate::interfaces::nextcloud::basic_auth_middleware::basic_auth_middleware;
use crate::interfaces::nextcloud::login_v2_handler;
use crate::interfaces::nextcloud::ocs_handler;
use crate::interfaces::nextcloud::status_handler;
use crate::interfaces::nextcloud::uploads_handler;
use crate::interfaces::nextcloud::webdav_handler;

/// Build the complete Nextcloud-compatible router.
///
/// Public routes (no auth):
///   - GET  /status.php
///   - POST /index.php/login/v2
///   - GET  /login/v2/flow/{token}
///   - POST /login/v2/flow/{token}
///   - POST /login/v2/poll (alias for /index.php/login/v2/poll)
///
/// Protected routes (Basic Auth via app passwords):
///   - /ocs/v1.php/cloud/capabilities
///   - /ocs/v2.php/cloud/capabilities
///   - /ocs/v2.php/cloud/user
///   - /ocs/v2.php/core/apppassword (DELETE)
///   - /ocs/v2.php/apps/notifications/api/v2/notifications
///   - /ocs/v2.php/apps/notifications/api/v2/push
///   - /remote.php/dav/files/{user}/{*path}
///   - /remote.php/dav/uploads/{user}/{upload_id}/{*rest}
///   - /remote.php/webdav/{*path} (legacy redirect)
pub fn nextcloud_routes() -> Router<Arc<AppState>> {
    // Public routes — no auth required.
    let public = Router::new()
        .route("/status.php", get(status_handler::handle_status))
        // Login Flow v2
        .route(
            "/index.php/login/v2",
            post(login_v2_handler::handle_login_initiate),
        )
        .route(
            "/login/v2/flow/{token}",
            get(login_v2_handler::handle_login_page)
                .post(login_v2_handler::handle_login_submit),
        )
        .route(
            "/index.php/login/v2/poll",
            post(login_v2_handler::handle_login_poll),
        )
        .route(
            "/login/v2/poll",
            post(login_v2_handler::handle_login_poll),
        );

    // Protected routes — require Basic Auth via app passwords.
    let protected = Router::new()
        // OCS capabilities
        .route(
            "/ocs/v1.php/cloud/capabilities",
            get(ocs_handler::handle_capabilities_v1),
        )
        .route(
            "/ocs/v2.php/cloud/capabilities",
            get(ocs_handler::handle_capabilities_v2),
        )
        // OCS user info
        .route(
            "/ocs/v2.php/cloud/user",
            get(ocs_handler::handle_user_info),
        )
        // App password revocation
        .route(
            "/ocs/v2.php/core/apppassword",
            delete(ocs_handler::handle_revoke_apppassword),
        )
        // Notifications stubs
        .route(
            "/ocs/v2.php/apps/notifications/api/v2/notifications",
            get(ocs_handler::handle_notifications_list),
        )
        .route(
            "/ocs/v2.php/apps/notifications/api/v2/push",
            post(ocs_handler::handle_notifications_push),
        )
        // WebDAV files
        .route(
            "/remote.php/dav/files/{user}/{*subpath}",
            any(handle_dav_files),
        )
        .route("/remote.php/dav/files/{user}/", any(handle_dav_files_root))
        .route("/remote.php/dav/files/{user}", any(handle_dav_files_root))
        // Chunked uploads
        .route(
            "/remote.php/dav/uploads/{user}/{upload_id}/{*rest}",
            any(handle_dav_uploads),
        )
        .route(
            "/remote.php/dav/uploads/{user}/{upload_id}",
            any(handle_dav_uploads_root),
        )
        // Legacy WebDAV redirect
        .route(
            "/remote.php/webdav/{*subpath}",
            any(handle_legacy_webdav),
        )
        .route("/remote.php/webdav/", any(handle_legacy_webdav_root))
        .route("/remote.php/webdav", any(handle_legacy_webdav_root))
        .layer(middleware::from_fn_with_state(
            // The state will be provided when merged into the main router.
            // We use a type placeholder here — axum resolves it at build time.
            Arc::new(AppState::default()),
            basic_auth_middleware,
        ));

    Router::new().merge(public).merge(protected)
}

/// Build Nextcloud routes with a pre-built `Arc<AppState>` for the middleware layer.
///
/// This is the preferred entry point — pass the real state so the Basic Auth
/// middleware can look up app passwords from the database.
pub fn nextcloud_routes_with_state(state: Arc<AppState>) -> Router<Arc<AppState>> {
    // Public routes — no auth required.
    let public = Router::new()
        .route("/status.php", get(status_handler::handle_status))
        .route(
            "/index.php/login/v2",
            post(login_v2_handler::handle_login_initiate),
        )
        .route(
            "/login/v2/flow/{token}",
            get(login_v2_handler::handle_login_page)
                .post(login_v2_handler::handle_login_submit),
        )
        .route(
            "/index.php/login/v2/poll",
            post(login_v2_handler::handle_login_poll),
        )
        .route(
            "/login/v2/poll",
            post(login_v2_handler::handle_login_poll),
        );

    // Protected routes — require Basic Auth via app passwords.
    let protected = Router::new()
        .route(
            "/ocs/v1.php/cloud/capabilities",
            get(ocs_handler::handle_capabilities_v1),
        )
        .route(
            "/ocs/v2.php/cloud/capabilities",
            get(ocs_handler::handle_capabilities_v2),
        )
        .route(
            "/ocs/v2.php/cloud/user",
            get(ocs_handler::handle_user_info),
        )
        .route(
            "/ocs/v2.php/core/apppassword",
            delete(ocs_handler::handle_revoke_apppassword),
        )
        .route(
            "/ocs/v2.php/apps/notifications/api/v2/notifications",
            get(ocs_handler::handle_notifications_list),
        )
        .route(
            "/ocs/v2.php/apps/notifications/api/v2/push",
            post(ocs_handler::handle_notifications_push),
        )
        .route(
            "/remote.php/dav/files/{user}/{*subpath}",
            any(handle_dav_files),
        )
        .route("/remote.php/dav/files/{user}/", any(handle_dav_files_root))
        .route("/remote.php/dav/files/{user}", any(handle_dav_files_root))
        .route(
            "/remote.php/dav/uploads/{user}/{upload_id}/{*rest}",
            any(handle_dav_uploads),
        )
        .route(
            "/remote.php/dav/uploads/{user}/{upload_id}",
            any(handle_dav_uploads_root),
        )
        .route(
            "/remote.php/webdav/{*subpath}",
            any(handle_legacy_webdav),
        )
        .route("/remote.php/webdav/", any(handle_legacy_webdav_root))
        .route("/remote.php/webdav", any(handle_legacy_webdav_root))
        .layer(middleware::from_fn_with_state(
            state,
            basic_auth_middleware,
        ));

    Router::new().merge(public).merge(protected)
}

// ──────────────── Handler glue ────────────────

async fn handle_dav_files(
    State(state): State<Arc<AppState>>,
    Path((_user, subpath)): Path<(String, String)>,
    user_ext: CurrentUser,
    req: Request<Body>,
) -> Result<Response, Response> {
    webdav_handler::handle_nc_webdav(state, req, user_ext, subpath)
        .await
        .map_err(|e| e.into_response())
}

async fn handle_dav_files_root(
    State(state): State<Arc<AppState>>,
    Path(_user): Path<String>,
    user_ext: CurrentUser,
    req: Request<Body>,
) -> Result<Response, Response> {
    webdav_handler::handle_nc_webdav(state, req, user_ext, String::new())
        .await
        .map_err(|e| e.into_response())
}

async fn handle_dav_uploads(
    State(state): State<Arc<AppState>>,
    Path((_user, upload_id, rest)): Path<(String, String, String)>,
    user_ext: CurrentUser,
    req: Request<Body>,
) -> Result<Response, Response> {
    uploads_handler::handle_nc_uploads(state, req, user_ext, upload_id, rest)
        .await
        .map_err(|e| e.into_response())
}

async fn handle_dav_uploads_root(
    State(state): State<Arc<AppState>>,
    Path((_user, upload_id)): Path<(String, String)>,
    user_ext: CurrentUser,
    req: Request<Body>,
) -> Result<Response, Response> {
    uploads_handler::handle_nc_uploads(state, req, user_ext, upload_id, String::new())
        .await
        .map_err(|e| e.into_response())
}

/// Legacy /remote.php/webdav/* — redirect to /remote.php/dav/files/{user}/*
async fn handle_legacy_webdav(
    Path(subpath): Path<String>,
    user_ext: CurrentUser,
) -> Response {
    let location = format!(
        "/remote.php/dav/files/{}/{}",
        user_ext.username, subpath
    );
    Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header("location", location)
        .body(Body::empty())
        .unwrap()
}

async fn handle_legacy_webdav_root(user_ext: CurrentUser) -> Response {
    let location = format!("/remote.php/dav/files/{}/", user_ext.username);
    Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header("location", location)
        .body(Body::empty())
        .unwrap()
}
