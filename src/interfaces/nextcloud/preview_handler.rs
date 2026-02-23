//! Nextcloud-compatible preview/thumbnail endpoint.
//!
//! Maps Nextcloud preview requests to OxiCloud's thumbnail service.

use axum::{
    body::Body,
    extract::{Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::application::ports::thumbnail_ports::ThumbnailSize;
use crate::common::di::AppState;
use crate::interfaces::middleware::auth::CurrentUser;

#[derive(Debug, Deserialize)]
pub struct PreviewParams {
    #[serde(rename = "fileId")]
    file_id: String,
    x: Option<u32>,
    y: Option<u32>,
    #[serde(rename = "forceIcon")]
    force_icon: Option<u8>,
}

/// Handle Nextcloud preview requests.
///
/// Maps:
/// - `/index.php/core/preview?fileId=X` to thumbnail generation
/// - Size selection based on request dimensions and forceIcon param
pub async fn handle_preview(
    State(state): State<Arc<AppState>>,
    _user: CurrentUser,
    Query(params): Query<PreviewParams>,
) -> impl IntoResponse {
    // Parse the Nextcloud file ID (numeric) to get the OxiCloud UUID
    let nc_file_id: i64 = match params.file_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("Invalid file ID"))
                .unwrap();
        }
    };

    // Look up the OxiCloud file UUID from the Nextcloud ID
    let object_id = match state.nextcloud.as_ref() {
        Some(nc) => match nc.file_ids.get_oxicloud_id(nc_file_id).await {
            Ok(id) => id,
            Err(_) => {
                return Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("File not found"))
                    .unwrap();
            }
        },
        None => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Nextcloud integration not configured"))
                .unwrap();
        }
    };

    // Get file details
    let file = match state
        .applications
        .file_retrieval_service
        .get_file(&object_id)
        .await
    {
        Ok(file) => file,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("File not found"))
                .unwrap();
        }
    };

    // Determine thumbnail size based on request params
    let thumb_size = if params.force_icon == Some(1) {
        ThumbnailSize::Icon
    } else {
        // Map requested dimensions to our thumbnail sizes
        let max_dim = params.x.unwrap_or(400).max(params.y.unwrap_or(400));
        if max_dim <= 150 {
            ThumbnailSize::Icon
        } else if max_dim <= 400 {
            ThumbnailSize::Preview
        } else {
            ThumbnailSize::Large
        }
    };

    // Check if file is an image
    if !state
        .core
        .thumbnail_service
        .is_supported_image(&file.mime_type)
    {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Preview not available for this file type"))
            .unwrap();
    }

    // Get file path
    let storage_root = state.core.path_service.get_root_path();
    let file_path = storage_root.join(&file.path);

    // Generate/get thumbnail
    match state
        .core
        .thumbnail_service
        .get_thumbnail(&object_id, thumb_size, &file_path)
        .await
    {
        Ok(data) => {
            let etag = format!("\"thumb-{}-{:?}\"", object_id, thumb_size);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "image/webp")
                .header(header::CONTENT_LENGTH, data.len())
                .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                .header(header::ETAG, etag)
                .body(Body::from(data))
                .unwrap()
        }
        Err(err) => {
            tracing::error!("Thumbnail generation failed for {}: {}", object_id, err);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Failed to generate thumbnail"))
                .unwrap()
        }
    }
}
