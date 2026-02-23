use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::common::errors::{DomainError, Result};

#[derive(Clone)]
pub struct NextcloudChunkedUploadService {
    pub base_dir: PathBuf,
}

impl NextcloudChunkedUploadService {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn new_stub() -> Self {
        Self {
            base_dir: PathBuf::from("./storage/.uploads/nextcloud"),
        }
    }

    /// Create a new upload session directory.
    pub async fn create_session(&self, user: &str, upload_id: &str) -> Result<()> {
        let session_dir = self.base_dir.join(user).join(upload_id);
        fs::create_dir_all(&session_dir)
            .await
            .map_err(|e| DomainError::internal_error("ChunkedUpload", &e.to_string()))?;
        Ok(())
    }

    /// Store a chunk in the session directory.
    pub async fn store_chunk(
        &self,
        user: &str,
        upload_id: &str,
        chunk_name: &str,
        data: &[u8],
    ) -> Result<()> {
        let chunk_path = self.base_dir.join(user).join(upload_id).join(chunk_name);
        let mut file = fs::File::create(&chunk_path)
            .await
            .map_err(|e| DomainError::internal_error("ChunkedUpload", &e.to_string()))?;
        file.write_all(data)
            .await
            .map_err(|e| DomainError::internal_error("ChunkedUpload", &e.to_string()))?;
        Ok(())
    }

    /// Assemble all chunks in numeric order into a single byte vector.
    pub async fn assemble(&self, user: &str, upload_id: &str) -> Result<Vec<u8>> {
        let session_dir = self.base_dir.join(user).join(upload_id);
        let mut entries: Vec<String> = Vec::new();

        let mut dir = fs::read_dir(&session_dir)
            .await
            .map_err(|e| DomainError::internal_error("ChunkedUpload", &e.to_string()))?;

        while let Some(entry) = dir
            .next_entry()
            .await
            .map_err(|e| DomainError::internal_error("ChunkedUpload", &e.to_string()))?
        {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".file" {
                continue; // Skip the assembly marker.
            }
            entries.push(name);
        }

        // Sort chunks numerically (Nextcloud sends them as "00001", "00002", ...).
        entries.sort();

        let mut assembled = Vec::new();
        for chunk_name in &entries {
            let data = fs::read(session_dir.join(chunk_name))
                .await
                .map_err(|e| DomainError::internal_error("ChunkedUpload", &e.to_string()))?;
            assembled.extend_from_slice(&data);
        }

        Ok(assembled)
    }

    /// Delete the upload session directory.
    pub async fn cleanup(&self, user: &str, upload_id: &str) -> Result<()> {
        let session_dir = self.base_dir.join(user).join(upload_id);
        if session_dir.exists() {
            fs::remove_dir_all(&session_dir)
                .await
                .map_err(|e| DomainError::internal_error("ChunkedUpload", &e.to_string()))?;
        }
        Ok(())
    }

    /// Check if a session directory exists.
    pub async fn session_exists(&self, user: &str, upload_id: &str) -> bool {
        self.base_dir.join(user).join(upload_id).exists()
    }
}
