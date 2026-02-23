use sqlx::{PgPool, Row};
use std::sync::Arc;

use crate::common::errors::{DomainError, ErrorKind, Result};

#[derive(Debug, Clone)]
pub struct AppPasswordRecord {
    pub id: String,
    pub user_id: String,
    pub password_hash: String,
}

pub struct AppPasswordRepository {
    pool: Arc<PgPool>,
}

impl AppPasswordRepository {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    pub async fn insert(
        &self,
        user_id: &str,
        label: &str,
        password_hash: &str,
        token_prefix: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO auth.app_passwords (user_id, label, password_hash, token_prefix)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(user_id)
        .bind(label)
        .bind(password_hash)
        .bind(token_prefix)
        .execute(&*self.pool)
        .await
        .map_err(|e| {
            DomainError::new(
                ErrorKind::DatabaseError,
                "NextcloudAppPassword",
                format!("Failed to insert app password: {}", e),
            )
        })?;

        Ok(())
    }

    pub async fn list_by_user_prefix(
        &self,
        user_id: &str,
        token_prefix: &str,
    ) -> Result<Vec<AppPasswordRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id::text, user_id::text, password_hash
            FROM auth.app_passwords
            WHERE user_id = $1 AND token_prefix = $2
            ORDER BY created_at DESC
            "#,
        )
        .bind(user_id)
        .bind(token_prefix)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| {
            DomainError::new(
                ErrorKind::DatabaseError,
                "NextcloudAppPassword",
                format!("Failed to query app passwords: {}", e),
            )
        })?;

        Ok(rows
            .into_iter()
            .map(|row| AppPasswordRecord {
                id: row.get("id"),
                user_id: row.get("user_id"),
                password_hash: row.get("password_hash"),
            })
            .collect())
    }

    pub async fn list_by_prefix(&self, token_prefix: &str) -> Result<Vec<AppPasswordRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id::text, user_id::text, password_hash
            FROM auth.app_passwords
            WHERE token_prefix = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(token_prefix)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| {
            DomainError::new(
                ErrorKind::DatabaseError,
                "NextcloudAppPassword",
                format!("Failed to query app passwords: {}", e),
            )
        })?;

        Ok(rows
            .into_iter()
            .map(|row| AppPasswordRecord {
                id: row.get("id"),
                user_id: row.get("user_id"),
                password_hash: row.get("password_hash"),
            })
            .collect())
    }

    pub async fn touch_last_used(&self, id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE auth.app_passwords
            SET last_used_at = NOW()
            WHERE id = $1::uuid
            "#,
        )
        .bind(id)
        .execute(&*self.pool)
        .await
        .map_err(|e| {
            DomainError::new(
                ErrorKind::DatabaseError,
                "NextcloudAppPassword",
                format!("Failed to update last_used_at: {}", e),
            )
        })?;

        Ok(())
    }

    pub async fn delete_by_id(&self, id: &str) -> Result<()> {
        sqlx::query(
            r#"
            DELETE FROM auth.app_passwords
            WHERE id = $1::uuid
            "#,
        )
        .bind(id)
        .execute(&*self.pool)
        .await
        .map_err(|e| {
            DomainError::new(
                ErrorKind::DatabaseError,
                "NextcloudAppPassword",
                format!("Failed to delete app password: {}", e),
            )
        })?;

        Ok(())
    }
}
