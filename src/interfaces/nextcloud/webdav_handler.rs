use axum::{
    body::{self, Body},
    http::{header, HeaderName, Request, StatusCode},
    response::Response,
};
use bytes::Buf;
use chrono::Utc;
use quick_xml::{
    Writer,
    events::{BytesEnd, BytesStart, BytesText, Event},
};
use std::sync::Arc;

use crate::application::adapters::webdav_adapter::{PropFindRequest, WebDavAdapter};
use crate::common::di::AppState;
use crate::interfaces::errors::AppError;
use crate::interfaces::middleware::auth::CurrentUser;

const HEADER_DAV: HeaderName = HeaderName::from_static("dav");

/// Resolve the internal OxiCloud path from a Nextcloud DAV subpath.
///
/// Nextcloud: /remote.php/dav/files/{user}/{subpath}
/// Internal:  My Folder - {username}/{subpath}
///
/// An empty subpath maps to the user's home folder root.
fn nc_to_internal_path(username: &str, subpath: &str) -> String {
    let home = format!("My Folder - {}", username);
    let subpath = subpath.trim_matches('/');
    if subpath.is_empty() {
        home
    } else {
        format!("{}/{}", home, subpath)
    }
}

/// Build the Nextcloud DAV href for a resource.
fn nc_href(username: &str, subpath: &str) -> String {
    let subpath = subpath.trim_matches('/');
    if subpath.is_empty() {
        format!("/remote.php/dav/files/{}/", username)
    } else {
        format!("/remote.php/dav/files/{}/{}", username, subpath)
    }
}

/// Dispatch Nextcloud WebDAV request to the appropriate handler.
///
/// `subpath` is everything after `/remote.php/dav/files/{user}/`.
pub async fn handle_nc_webdav(
    state: Arc<AppState>,
    req: Request<Body>,
    user: CurrentUser,
    subpath: String,
) -> Result<Response<Body>, AppError> {
    let method = req.method().clone();
    match method.as_str() {
        "OPTIONS" => handle_options(),
        "PROPFIND" => handle_propfind(state, req, &user, &subpath).await,
        "GET" => handle_get(state, &user, &subpath).await,
        "PUT" => handle_put(state, req, &user, &subpath).await,
        "MKCOL" => handle_mkcol(state, &user, &subpath).await,
        "DELETE" => handle_delete(state, &user, &subpath).await,
        "MOVE" => handle_move(state, req, &user, &subpath).await,
        "HEAD" => handle_head(state, &user, &subpath).await,
        _ => Ok(Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::empty())
            .unwrap()),
    }
}

// ──────────────────── OPTIONS ────────────────────

fn handle_options() -> Result<Response<Body>, AppError> {
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(HEADER_DAV, "1, 2, 3")
        .header(
            header::ALLOW,
            "OPTIONS, GET, HEAD, PUT, DELETE, MKCOL, MOVE, PROPFIND",
        )
        .body(Body::empty())
        .unwrap())
}

// ──────────────────── PROPFIND ────────────────────

async fn handle_propfind(
    state: Arc<AppState>,
    req: Request<Body>,
    user: &CurrentUser,
    subpath: &str,
) -> Result<Response<Body>, AppError> {
    let depth = req
        .headers()
        .get("depth")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("1")
        .to_string();

    // Parse the PROPFIND XML body (or assume allprop if empty).
    let body_bytes = body::to_bytes(req.into_body(), 64 * 1024)
        .await
        .map_err(|e| AppError::bad_request(&format!("Failed to read body: {}", e)))?;

    let propfind = if body_bytes.is_empty() {
        PropFindRequest {
            prop_find_type: crate::application::adapters::webdav_adapter::PropFindType::AllProp,
        }
    } else {
        WebDavAdapter::parse_propfind(body_bytes.reader())
            .map_err(|e| AppError::bad_request(&format!("Invalid PROPFIND XML: {}", e)))?
    };

    let internal_path = nc_to_internal_path(&user.username, subpath);
    let folder_service = &state.applications.folder_service;
    let file_service = &state.applications.file_retrieval_service;

    // Try to resolve as folder first.
    let folder_result = folder_service.get_folder_by_path(&internal_path).await;

    if let Ok(folder) = folder_result {
        // It's a folder.
        let (files, subfolders) = if depth != "0" {
            let files = file_service
                .list_files(Some(&folder.id))
                .await
                .unwrap_or_default();
            let subfolders = folder_service
                .list_folders(Some(&folder.id))
                .await
                .unwrap_or_default();
            (files, subfolders)
        } else {
            (vec![], vec![])
        };

        // Generate Nextcloud-aware XML.
        let nc = state.nextcloud.as_ref();
        let file_id_svc = nc.map(|n| &n.file_ids);

        let mut buf = Vec::new();
        write_nc_multistatus(
            &mut buf,
            Some(&folder),
            &files,
            &subfolders,
            &propfind,
            &depth,
            &user.username,
            subpath,
            file_id_svc,
        )
        .await
        .map_err(|e| AppError::internal_error(&format!("XML generation failed: {}", e)))?;

        return Ok(Response::builder()
            .status(StatusCode::MULTI_STATUS)
            .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
            .body(Body::from(buf))
            .unwrap());
    }

    // Not a folder — try as a file.
    let file_result = file_service.get_file_by_path(&internal_path).await;
    if let Ok(file) = file_result {
        let nc = state.nextcloud.as_ref();
        let file_id_svc = nc.map(|n| &n.file_ids);

        let mut buf = Vec::new();
        write_nc_multistatus(
            &mut buf,
            None,
            &[file],
            &[],
            &propfind,
            "0",
            &user.username,
            subpath,
            file_id_svc,
        )
        .await
        .map_err(|e| AppError::internal_error(&format!("XML generation failed: {}", e)))?;

        return Ok(Response::builder()
            .status(StatusCode::MULTI_STATUS)
            .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
            .body(Body::from(buf))
            .unwrap());
    }

    Err(AppError::not_found("Resource not found"))
}

// ──────────────────── GET ────────────────────

async fn handle_get(
    state: Arc<AppState>,
    user: &CurrentUser,
    subpath: &str,
) -> Result<Response<Body>, AppError> {
    let internal_path = nc_to_internal_path(&user.username, subpath);
    let file_service = &state.applications.file_retrieval_service;

    let file = file_service
        .get_file_by_path(&internal_path)
        .await
        .map_err(|_| AppError::not_found("File not found"))?;

    let content = file_service
        .get_file_content(&file.id)
        .await
        .map_err(|e| AppError::internal_error(&format!("Failed to read file: {}", e)))?;

    let modified_at = chrono::DateTime::<Utc>::from_timestamp(file.modified_at as i64, 0)
        .unwrap_or_else(Utc::now);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, &file.mime_type)
        .header(header::CONTENT_LENGTH, content.len())
        .header(header::ETAG, format!("\"{}\"", file.id))
        .header(header::LAST_MODIFIED, modified_at.to_rfc2822())
        .body(Body::from(content))
        .unwrap())
}

// ──────────────────── HEAD ────────────────────

async fn handle_head(
    state: Arc<AppState>,
    user: &CurrentUser,
    subpath: &str,
) -> Result<Response<Body>, AppError> {
    let internal_path = nc_to_internal_path(&user.username, subpath);
    let file_service = &state.applications.file_retrieval_service;

    let file = file_service
        .get_file_by_path(&internal_path)
        .await
        .map_err(|_| AppError::not_found("File not found"))?;

    let modified_at = chrono::DateTime::<Utc>::from_timestamp(file.modified_at as i64, 0)
        .unwrap_or_else(Utc::now);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, &file.mime_type)
        .header(header::CONTENT_LENGTH, file.size)
        .header(header::ETAG, format!("\"{}\"", file.id))
        .header(header::LAST_MODIFIED, modified_at.to_rfc2822())
        .body(Body::empty())
        .unwrap())
}

// ──────────────────── PUT ────────────────────

async fn handle_put(
    state: Arc<AppState>,
    req: Request<Body>,
    user: &CurrentUser,
    subpath: &str,
) -> Result<Response<Body>, AppError> {
    let internal_path = nc_to_internal_path(&user.username, subpath);
    let file_service = &state.applications.file_retrieval_service;
    let upload_service = &state.applications.file_upload_service;

    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let body_bytes = body::to_bytes(req.into_body(), 512 * 1024 * 1024) // 512 MB limit
        .await
        .map_err(|e| AppError::bad_request(&format!("Failed to read body: {}", e)))?;

    // Check if the file already exists (update vs create).
    let existing = file_service.get_file_by_path(&internal_path).await;

    if existing.is_ok() {
        // Update existing file.
        upload_service
            .update_file(&internal_path, &body_bytes)
            .await
            .map_err(|e| AppError::internal_error(&format!("Failed to update file: {}", e)))?;

        // Re-fetch for etag.
        if let Ok(updated) = file_service.get_file_by_path(&internal_path).await {
            return Ok(Response::builder()
                .status(StatusCode::NO_CONTENT)
                .header(header::ETAG, format!("\"{}\"", updated.id))
                .body(Body::empty())
                .unwrap());
        }

        return Ok(Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .unwrap());
    }

    // Create new file — split subpath into parent dir and filename.
    let (parent_subpath, filename) = match subpath.rsplit_once('/') {
        Some((parent, name)) => (parent, name),
        None => ("", subpath),
    };

    let parent_internal = nc_to_internal_path(&user.username, parent_subpath);

    let file_dto = upload_service
        .create_file(&parent_internal, filename, &body_bytes, &content_type)
        .await
        .map_err(|e| AppError::internal_error(&format!("Failed to create file: {}", e)))?;

    Ok(Response::builder()
        .status(StatusCode::CREATED)
        .header(header::ETAG, format!("\"{}\"", file_dto.id))
        .body(Body::empty())
        .unwrap())
}

// ──────────────────── MKCOL ────────────────────

async fn handle_mkcol(
    state: Arc<AppState>,
    user: &CurrentUser,
    subpath: &str,
) -> Result<Response<Body>, AppError> {
    use crate::application::dtos::folder_dto::CreateFolderDto;

    let folder_service = &state.applications.folder_service;

    // Split into parent + new folder name.
    let (parent_subpath, folder_name) = match subpath.rsplit_once('/') {
        Some((parent, name)) => (parent, name),
        None => ("", subpath),
    };

    let parent_internal = nc_to_internal_path(&user.username, parent_subpath);

    // Resolve parent folder ID.
    let parent_folder = folder_service
        .get_folder_by_path(&parent_internal)
        .await
        .map_err(|_| AppError::not_found("Parent folder not found"))?;

    let dto = CreateFolderDto {
        name: folder_name.to_string(),
        parent_id: Some(parent_folder.id.clone()),
    };

    folder_service
        .create_folder(dto)
        .await
        .map_err(|e| AppError::internal_error(&format!("Failed to create folder: {}", e)))?;

    Ok(Response::builder()
        .status(StatusCode::CREATED)
        .body(Body::empty())
        .unwrap())
}

// ──────────────────── DELETE ────────────────────

async fn handle_delete(
    state: Arc<AppState>,
    user: &CurrentUser,
    subpath: &str,
) -> Result<Response<Body>, AppError> {
    let internal_path = nc_to_internal_path(&user.username, subpath);
    let folder_service = &state.applications.folder_service;
    let file_service = &state.applications.file_retrieval_service;
    let file_mgmt = &state.applications.file_management_service;

    // Try as folder first.
    if let Ok(folder) = folder_service.get_folder_by_path(&internal_path).await {
        folder_service
            .delete_folder(&folder.id, &user.id)
            .await
            .map_err(|e| AppError::internal_error(&format!("Failed to delete folder: {}", e)))?;

        return Ok(Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .unwrap());
    }

    // Try as file.
    if let Ok(file) = file_service.get_file_by_path(&internal_path).await {
        file_mgmt
            .delete_file(&file.id)
            .await
            .map_err(|e| AppError::internal_error(&format!("Failed to delete file: {}", e)))?;

        return Ok(Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .unwrap());
    }

    Err(AppError::not_found("Resource not found"))
}

// ──────────────────── MOVE ────────────────────

async fn handle_move(
    state: Arc<AppState>,
    req: Request<Body>,
    user: &CurrentUser,
    subpath: &str,
) -> Result<Response<Body>, AppError> {
    let destination = req
        .headers()
        .get("destination")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::bad_request("Missing Destination header"))?
        .to_string();

    // Parse destination path: extract subpath after /remote.php/dav/files/{user}/
    let dest_subpath = extract_nc_subpath_from_dest(&destination, &user.username)
        .ok_or_else(|| AppError::bad_request("Invalid Destination URL"))?;

    let src_internal = nc_to_internal_path(&user.username, subpath);
    let folder_service = &state.applications.folder_service;
    let file_service = &state.applications.file_retrieval_service;
    let file_mgmt = &state.applications.file_management_service;

    // Try as file first.
    if let Ok(file) = file_service.get_file_by_path(&src_internal).await {
        let (dest_parent_sub, dest_name) = match dest_subpath.rsplit_once('/') {
            Some((parent, name)) => (parent, name),
            None => ("", dest_subpath.as_str()),
        };
        let dest_parent_internal = nc_to_internal_path(&user.username, dest_parent_sub);

        // Rename if only the name changes (same parent).
        let src_parent_sub = match subpath.rsplit_once('/') {
            Some((parent, _)) => parent,
            None => "",
        };

        if src_parent_sub == dest_parent_sub {
            // Same parent → rename.
            file_mgmt
                .rename_file(&file.id, dest_name)
                .await
                .map_err(|e| AppError::internal_error(&format!("Rename failed: {}", e)))?;
        } else {
            // Different parent → move.
            let dest_parent = folder_service
                .get_folder_by_path(&dest_parent_internal)
                .await
                .map_err(|_| AppError::not_found("Destination folder not found"))?;

            file_mgmt
                .move_file(&file.id, Some(dest_parent.id.clone()))
                .await
                .map_err(|e| AppError::internal_error(&format!("Move failed: {}", e)))?;

            // If the filename changed too, rename after move.
            if file.name != dest_name {
                file_mgmt
                    .rename_file(&file.id, dest_name)
                    .await
                    .map_err(|e| AppError::internal_error(&format!("Rename failed: {}", e)))?;
            }
        }

        return Ok(Response::builder()
            .status(StatusCode::CREATED)
            .body(Body::empty())
            .unwrap());
    }

    // Try as folder.
    if let Ok(folder) = folder_service.get_folder_by_path(&src_internal).await {
        let (dest_parent_sub, dest_name) = match dest_subpath.rsplit_once('/') {
            Some((parent, name)) => (parent, name),
            None => ("", dest_subpath.as_str()),
        };
        let dest_parent_internal = nc_to_internal_path(&user.username, dest_parent_sub);

        let src_parent_sub = match subpath.rsplit_once('/') {
            Some((parent, _)) => parent,
            None => "",
        };

        if src_parent_sub == dest_parent_sub {
            // Same parent → rename.
            use crate::application::dtos::folder_dto::RenameFolderDto;
            folder_service
                .rename_folder(
                    &folder.id,
                    RenameFolderDto {
                        name: dest_name.to_string(),
                    },
                    &user.id,
                )
                .await
                .map_err(|e| AppError::internal_error(&format!("Rename failed: {}", e)))?;
        } else {
            // Different parent → move.
            let dest_parent = folder_service
                .get_folder_by_path(&dest_parent_internal)
                .await
                .map_err(|_| AppError::not_found("Destination parent not found"))?;

            use crate::application::dtos::folder_dto::MoveFolderDto;
            folder_service
                .move_folder(
                    &folder.id,
                    MoveFolderDto {
                        parent_id: Some(dest_parent.id.clone()),
                    },
                    &user.id,
                )
                .await
                .map_err(|e| AppError::internal_error(&format!("Move failed: {}", e)))?;

            // If the name changed too, rename.
            if folder.name != dest_name {
                use crate::application::dtos::folder_dto::RenameFolderDto;
                folder_service
                    .rename_folder(
                        &folder.id,
                        RenameFolderDto {
                            name: dest_name.to_string(),
                        },
                        &user.id,
                    )
                    .await
                    .map_err(|e| AppError::internal_error(&format!("Rename failed: {}", e)))?;
            }
        }

        return Ok(Response::builder()
            .status(StatusCode::CREATED)
            .body(Body::empty())
            .unwrap());
    }

    Err(AppError::not_found("Source resource not found"))
}

/// Extract the subpath from a Destination header URL.
fn extract_nc_subpath_from_dest(dest: &str, username: &str) -> Option<String> {
    let prefix = format!("/remote.php/dav/files/{}/", username);
    // The destination may be a full URL (https://host/remote.php/...) or a path.
    let path = if let Some(idx) = dest.find("/remote.php/") {
        &dest[idx..]
    } else {
        dest
    };
    let decoded = urlencoding::decode(path).ok()?;
    let decoded = decoded.trim_end_matches('/');
    decoded.strip_prefix(prefix.trim_end_matches('/')).map(|s| {
        s.trim_start_matches('/').to_string()
    })
}

// ────────────── Nextcloud PROPFIND XML Generation ──────────────

use crate::application::dtos::file_dto::FileDto;
use crate::application::dtos::folder_dto::FolderDto;
use crate::application::services::nextcloud_file_id_service::NextcloudFileIdService;

/// Generate a complete Nextcloud-compatible multistatus XML response.
async fn write_nc_multistatus<W: std::io::Write>(
    writer: W,
    folder: Option<&FolderDto>,
    files: &[FileDto],
    subfolders: &[FolderDto],
    _request: &PropFindRequest,
    depth: &str,
    username: &str,
    subpath: &str,
    file_id_svc: Option<&Arc<NextcloudFileIdService>>,
) -> Result<(), String> {
    let mut xml = Writer::new(writer);

    // Root element with all required namespaces.
    let mut ms = BytesStart::new("d:multistatus");
    ms.push_attribute(("xmlns:d", "DAV:"));
    ms.push_attribute(("xmlns:oc", "http://owncloud.org/ns"));
    ms.push_attribute(("xmlns:nc", "http://nextcloud.org/ns"));
    xml.write_event(Event::Start(ms)).map_err(|e| e.to_string())?;

    // Current folder entry.
    if let Some(f) = folder {
        let href = nc_href(username, subpath);
        let file_id = resolve_folder_id(file_id_svc, &f.id).await;
        let oc_id = file_id.map(|id| format_oc_id(id, file_id_svc));
        write_folder_response(&mut xml, f, &href, file_id, oc_id.as_deref(), username)?;
    }

    if depth != "0" {
        // Files.
        for file in files {
            let child_sub = if subpath.is_empty() {
                file.name.clone()
            } else {
                format!("{}/{}", subpath.trim_end_matches('/'), file.name)
            };
            let href = nc_href(username, &child_sub);
            let file_id = resolve_file_id(file_id_svc, &file.id).await;
            let oc_id = file_id.map(|id| format_oc_id(id, file_id_svc));
            write_file_response(&mut xml, file, &href, file_id, oc_id.as_deref(), username)?;
        }

        // Subfolders.
        for sf in subfolders {
            let child_sub = if subpath.is_empty() {
                sf.name.clone()
            } else {
                format!("{}/{}", subpath.trim_end_matches('/'), sf.name)
            };
            let href = format!("{}/", nc_href(username, &child_sub));
            let file_id = resolve_folder_id(file_id_svc, &sf.id).await;
            let oc_id = file_id.map(|id| format_oc_id(id, file_id_svc));
            write_folder_response(&mut xml, sf, &href, file_id, oc_id.as_deref(), username)?;
        }
    }

    xml.write_event(Event::End(BytesEnd::new("d:multistatus")))
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn write_folder_response<W: std::io::Write>(
    xml: &mut Writer<W>,
    folder: &FolderDto,
    href: &str,
    file_id: Option<i64>,
    oc_id: Option<&str>,
    owner: &str,
) -> Result<(), String> {
    xml.write_event(Event::Start(BytesStart::new("d:response")))
        .map_err(|e| e.to_string())?;

    // href
    write_text_element(xml, "d:href", href)?;

    xml.write_event(Event::Start(BytesStart::new("d:propstat")))
        .map_err(|e| e.to_string())?;
    xml.write_event(Event::Start(BytesStart::new("d:prop")))
        .map_err(|e| e.to_string())?;

    // resourcetype
    xml.write_event(Event::Start(BytesStart::new("d:resourcetype")))
        .map_err(|e| e.to_string())?;
    xml.write_event(Event::Empty(BytesStart::new("d:collection")))
        .map_err(|e| e.to_string())?;
    xml.write_event(Event::End(BytesEnd::new("d:resourcetype")))
        .map_err(|e| e.to_string())?;

    write_text_element(xml, "d:displayname", &folder.name)?;

    let created_at = chrono::DateTime::<Utc>::from_timestamp(folder.created_at as i64, 0)
        .unwrap_or_else(Utc::now);
    let modified_at = chrono::DateTime::<Utc>::from_timestamp(folder.modified_at as i64, 0)
        .unwrap_or_else(Utc::now);

    write_text_element(xml, "d:getlastmodified", &modified_at.to_rfc2822())?;
    write_text_element(xml, "d:getetag", &format!("\"{}\"", folder.id))?;
    write_text_element(xml, "d:getcontenttype", "httpd/unix-directory")?;
    write_text_element(xml, "d:getcontentlength", "0")?;
    write_text_element(xml, "d:creationdate", &created_at.to_rfc3339())?;

    // Nextcloud/ownCloud properties
    if let Some(id) = file_id {
        write_text_element(xml, "oc:fileid", &id.to_string())?;
    }
    if let Some(oid) = oc_id {
        write_text_element(xml, "oc:id", oid)?;
    }
    write_text_element(xml, "oc:permissions", "RGDNVCK")?;
    write_text_element(xml, "oc:size", "0")?;
    write_text_element(xml, "oc:owner-id", owner)?;
    write_text_element(xml, "oc:owner-display-name", owner)?;
    write_text_element(xml, "nc:has-preview", "false")?;
    write_text_element(xml, "nc:is-encrypted", "0")?;
    write_text_element(xml, "nc:mount-type", "")?;

    xml.write_event(Event::End(BytesEnd::new("d:prop")))
        .map_err(|e| e.to_string())?;
    write_text_element(xml, "d:status", "HTTP/1.1 200 OK")?;
    xml.write_event(Event::End(BytesEnd::new("d:propstat")))
        .map_err(|e| e.to_string())?;

    xml.write_event(Event::End(BytesEnd::new("d:response")))
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn write_file_response<W: std::io::Write>(
    xml: &mut Writer<W>,
    file: &FileDto,
    href: &str,
    file_id: Option<i64>,
    oc_id: Option<&str>,
    owner: &str,
) -> Result<(), String> {
    xml.write_event(Event::Start(BytesStart::new("d:response")))
        .map_err(|e| e.to_string())?;

    write_text_element(xml, "d:href", href)?;

    xml.write_event(Event::Start(BytesStart::new("d:propstat")))
        .map_err(|e| e.to_string())?;
    xml.write_event(Event::Start(BytesStart::new("d:prop")))
        .map_err(|e| e.to_string())?;

    // resourcetype (empty for files)
    xml.write_event(Event::Empty(BytesStart::new("d:resourcetype")))
        .map_err(|e| e.to_string())?;

    write_text_element(xml, "d:displayname", &file.name)?;
    write_text_element(xml, "d:getcontenttype", &file.mime_type)?;
    write_text_element(xml, "d:getcontentlength", &file.size.to_string())?;

    let created_at = chrono::DateTime::<Utc>::from_timestamp(file.created_at as i64, 0)
        .unwrap_or_else(Utc::now);
    let modified_at = chrono::DateTime::<Utc>::from_timestamp(file.modified_at as i64, 0)
        .unwrap_or_else(Utc::now);

    write_text_element(xml, "d:getlastmodified", &modified_at.to_rfc2822())?;
    write_text_element(xml, "d:getetag", &format!("\"{}\"", file.id))?;
    write_text_element(xml, "d:creationdate", &created_at.to_rfc3339())?;

    // Nextcloud/ownCloud properties
    if let Some(id) = file_id {
        write_text_element(xml, "oc:fileid", &id.to_string())?;
    }
    if let Some(oid) = oc_id {
        write_text_element(xml, "oc:id", oid)?;
    }
    write_text_element(xml, "oc:permissions", "RGDNVW")?;
    write_text_element(xml, "oc:size", &file.size.to_string())?;
    write_text_element(xml, "oc:owner-id", owner)?;
    write_text_element(xml, "oc:owner-display-name", owner)?;
    write_text_element(xml, "nc:has-preview", "false")?;
    write_text_element(xml, "nc:is-encrypted", "0")?;
    write_text_element(xml, "nc:mount-type", "")?;

    xml.write_event(Event::End(BytesEnd::new("d:prop")))
        .map_err(|e| e.to_string())?;
    write_text_element(xml, "d:status", "HTTP/1.1 200 OK")?;
    xml.write_event(Event::End(BytesEnd::new("d:propstat")))
        .map_err(|e| e.to_string())?;

    xml.write_event(Event::End(BytesEnd::new("d:response")))
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn write_text_element<W: std::io::Write>(
    xml: &mut Writer<W>,
    tag: &str,
    value: &str,
) -> Result<(), String> {
    xml.write_event(Event::Start(BytesStart::new(tag)))
        .map_err(|e| e.to_string())?;
    xml.write_event(Event::Text(BytesText::new(value)))
        .map_err(|e| e.to_string())?;
    xml.write_event(Event::End(BytesEnd::new(tag)))
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn resolve_file_id(
    svc: Option<&Arc<NextcloudFileIdService>>,
    file_uuid: &str,
) -> Option<i64> {
    let svc = svc?;
    svc.get_or_create_file_id(file_uuid).await.ok()
}

async fn resolve_folder_id(
    svc: Option<&Arc<NextcloudFileIdService>>,
    folder_uuid: &str,
) -> Option<i64> {
    let svc = svc?;
    svc.get_or_create_folder_id(folder_uuid).await.ok()
}

fn format_oc_id(id: i64, svc: Option<&Arc<NextcloudFileIdService>>) -> String {
    match svc {
        Some(s) => s.format_oc_id(id),
        None => format!("{:08}ocnca", id),
    }
}
