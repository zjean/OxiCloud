use sqlx::{PgPool, Row};
use std::sync::Arc;

use crate::common::errors::{DomainError, ErrorKind, Result};

pub struct NextcloudObjectIdRepository {
    pool: Arc<PgPool>,
}

impl NextcloudObjectIdRepository {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    pub async fn get_or_create(&self, object_type: &str, object_id: &str) -> Result<i64> {
        let row = sqlx::query(
            r#"
            INSERT INTO storage.nextcloud_object_ids (object_type, object_id)
            VALUES ($1, $2::uuid)
            ON CONFLICT (object_type, object_id)
            DO UPDATE SET object_id = EXCLUDED.object_id
            RETURNING id
            "#,
        )
        .bind(object_type)
        .bind(object_id)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| {
            DomainError::new(
                ErrorKind::DatabaseError,
                "NextcloudFileId",
                format!("Failed to get/create Nextcloud ID: {}", e),
            )
        })?;

        Ok(row.get::<i64, _>("id"))
    }
}
