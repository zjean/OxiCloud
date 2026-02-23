use std::sync::Arc;

use crate::common::errors::{DomainError, ErrorKind, Result};
use crate::infrastructure::repositories::pg::NextcloudObjectIdRepository;

#[derive(Clone)]
pub struct NextcloudFileIdService {
    repo: Option<Arc<NextcloudObjectIdRepository>>,
    instance_id: String,
}

impl NextcloudFileIdService {
    pub fn new(repo: Arc<NextcloudObjectIdRepository>, instance_id: String) -> Self {
        Self {
            repo: Some(repo),
            instance_id,
        }
    }

    pub fn new_stub() -> Self {
        Self {
            repo: None,
            instance_id: "ocnca".to_string(),
        }
    }

    pub async fn get_or_create_file_id(&self, file_id: &str) -> Result<i64> {
        let repo = self.repo.as_ref().ok_or_else(|| {
            DomainError::internal_error("NextcloudFileId", "Repository not initialized")
        })?;
        repo.get_or_create("file", file_id).await
    }

    pub async fn get_or_create_folder_id(&self, folder_id: &str) -> Result<i64> {
        let repo = self.repo.as_ref().ok_or_else(|| {
            DomainError::internal_error("NextcloudFileId", "Repository not initialized")
        })?;
        repo.get_or_create("folder", folder_id).await
    }

    pub fn format_oc_id(&self, id: i64) -> String {
        format!("{:08}{}", id, self.instance_id)
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub fn ensure_ready(&self) -> Result<()> {
        if self.repo.is_none() {
            return Err(DomainError::new(
                ErrorKind::InternalError,
                "NextcloudFileId",
                "Repository not initialized",
            ));
        }
        Ok(())
    }
}
