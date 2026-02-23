use std::sync::Arc;

use rand_core::RngCore;

use crate::application::dtos::user_dto::CurrentUser;
use crate::application::ports::auth_ports::{PasswordHasherPort, UserStoragePort};
use crate::common::errors::{DomainError, ErrorKind, Result};
use crate::infrastructure::repositories::pg::AppPasswordRepository;

const APP_PASSWORD_GROUPS: usize = 5;
const APP_PASSWORD_GROUP_LEN: usize = 5;
const APP_PASSWORD_PREFIX_LEN: usize = 8;

#[derive(Clone)]
pub struct NextcloudAppPasswordService {
    repo: Option<Arc<AppPasswordRepository>>,
    hasher: Option<Arc<dyn PasswordHasherPort>>,
    users: Option<Arc<dyn UserStoragePort>>,
}

impl NextcloudAppPasswordService {
    pub fn new(
        repo: Arc<AppPasswordRepository>,
        hasher: Arc<dyn PasswordHasherPort>,
        users: Arc<dyn UserStoragePort>,
    ) -> Self {
        Self {
            repo: Some(repo),
            hasher: Some(hasher),
            users: Some(users),
        }
    }

    pub fn new_stub() -> Self {
        Self {
            repo: None,
            hasher: None,
            users: None,
        }
    }

    pub async fn create(&self, user_id: &str, label: &str) -> Result<String> {
        let (repo, hasher, _users) = self.ensure()?;
        let password = generate_app_password();
        let normalized = normalize_password(&password);
        let token_prefix = token_prefix(&normalized)?;
        let hash = hasher.hash_password(&normalized)?;

        repo.insert(user_id, label, &hash, &token_prefix).await?;

        Ok(password)
    }

    pub async fn validate(&self, username: &str, password: &str) -> Result<Option<CurrentUser>> {
        let (repo, hasher, users) = self.ensure()?;

        let user = match users.get_user_by_username(username).await {
            Ok(user) => user,
            Err(_) => return Ok(None),
        };

        if !user.is_active() {
            return Ok(None);
        }

        let normalized = normalize_password(password);
        let token_prefix = match token_prefix(&normalized) {
            Ok(prefix) => prefix,
            Err(_) => return Ok(None),
        };

        let candidates = repo.list_by_user_prefix(user.id(), &token_prefix).await?;

        for record in candidates {
            let is_valid = hasher.verify_password(&normalized, &record.password_hash)?;
            if is_valid {
                if let Err(e) = repo.touch_last_used(&record.id).await {
                    tracing::warn!(
                        "Failed to update app_password last_used_at for {}: {}",
                        record.id,
                        e
                    );
                }

                let current_user = CurrentUser {
                    id: user.id().to_string(),
                    username: user.username().to_string(),
                    email: user.email().to_string(),
                    role: user.role().to_string(),
                };

                return Ok(Some(current_user));
            }
        }

        Ok(None)
    }

    pub async fn revoke(&self, password: &str) -> Result<()> {
        let (repo, hasher, _users) = self.ensure()?;

        let normalized = normalize_password(password);
        let token_prefix = match token_prefix(&normalized) {
            Ok(prefix) => prefix,
            Err(_) => return Ok(()),
        };

        let candidates = repo.list_by_prefix(&token_prefix).await?;
        for record in candidates {
            let is_valid = hasher.verify_password(&normalized, &record.password_hash)?;
            if is_valid {
                repo.delete_by_id(&record.id).await?;
                break;
            }
        }

        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn ensure(
        &self,
    ) -> std::result::Result<
        (
            &AppPasswordRepository,
            &Arc<dyn PasswordHasherPort>,
            &Arc<dyn UserStoragePort>,
        ),
        DomainError,
    > {
        let repo = self.repo.as_ref().ok_or_else(|| {
            DomainError::internal_error("Nextcloud", "AppPassword repo not ready")
        })?;
        let hasher = self
            .hasher
            .as_ref()
            .ok_or_else(|| DomainError::internal_error("Nextcloud", "Hasher not ready"))?;
        let users = self
            .users
            .as_ref()
            .ok_or_else(|| DomainError::internal_error("Nextcloud", "User repo not ready"))?;
        Ok((repo, hasher, users))
    }
}

fn generate_app_password() -> String {
    let mut rng = rand_core::OsRng;
    let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut groups = Vec::with_capacity(APP_PASSWORD_GROUPS);

    for _ in 0..APP_PASSWORD_GROUPS {
        let mut group = String::with_capacity(APP_PASSWORD_GROUP_LEN);
        for _ in 0..APP_PASSWORD_GROUP_LEN {
            let idx = (rng.next_u32() % chars.len() as u32) as usize;
            group.push(chars[idx] as char);
        }
        groups.push(group);
    }

    groups.join("-")
}

fn normalize_password(password: &str) -> String {
    password
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

fn token_prefix(normalized: &str) -> Result<String> {
    if normalized.len() < APP_PASSWORD_PREFIX_LEN {
        return Err(DomainError::new(
            ErrorKind::InvalidInput,
            "NextcloudAppPassword",
            "App password too short",
        ));
    }
    Ok(normalized[..APP_PASSWORD_PREFIX_LEN].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_app_password_format() {
        let password = generate_app_password();
        let groups: Vec<&str> = password.split('-').collect();
        assert_eq!(groups.len(), APP_PASSWORD_GROUPS);
        for group in &groups {
            assert_eq!(group.len(), APP_PASSWORD_GROUP_LEN);
            assert!(group.chars().all(|c| c.is_ascii_alphanumeric()));
        }
    }

    #[test]
    fn test_normalize_password_strips_dashes_and_whitespace() {
        let normalized = normalize_password("AB12C-DE34F-GH56I");
        assert_eq!(normalized, "AB12CDE34FGH56I");
    }

    #[test]
    fn test_normalize_password_uppercases() {
        let normalized = normalize_password("abc-def");
        assert_eq!(normalized, "ABCDEF");
    }

    #[test]
    fn test_token_prefix_extracts_first_8_chars() {
        let prefix = token_prefix("ABCDEFGHIJKLMNOP").unwrap();
        assert_eq!(prefix, "ABCDEFGH");
    }

    #[test]
    fn test_token_prefix_too_short() {
        let result = token_prefix("SHORT");
        assert!(result.is_err());
    }

    #[test]
    fn test_generated_password_produces_valid_prefix() {
        let password = generate_app_password();
        let normalized = normalize_password(&password);
        let prefix = token_prefix(&normalized);
        assert!(prefix.is_ok());
        assert_eq!(prefix.unwrap().len(), APP_PASSWORD_PREFIX_LEN);
    }
}
