use std::{
    num::NonZeroUsize,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use chrono::Utc;
use lru::LruCache;

use super::{AdminAccessTokenContext, BearerTokenContext};

const DEFAULT_AUTH_TOKEN_CACHE_CAPACITY: usize = 4096;
const DEFAULT_AUTH_TOKEN_CACHE_TTL_SECONDS: u64 = 30;
const TOKEN_PREFIX_LEN: usize = 12;

#[derive(Clone)]
pub struct AuthTokenCache {
    inner: Arc<RwLock<AuthTokenCacheInner>>,
    ttl: Duration,
}

struct AuthTokenCacheInner {
    entries: Option<LruCache<AuthTokenCacheKey, AuthTokenCacheEntry>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct AuthTokenCacheKey {
    token_prefix: String,
    token_hash: String,
}

struct AuthTokenCacheEntry {
    context: BearerTokenContext,
    inserted_at: Instant,
}

impl AuthTokenCache {
    pub fn new() -> Self {
        Self::with_capacity_and_ttl(
            DEFAULT_AUTH_TOKEN_CACHE_CAPACITY,
            Duration::from_secs(DEFAULT_AUTH_TOKEN_CACHE_TTL_SECONDS),
        )
    }

    pub fn with_capacity_and_ttl(capacity: usize, ttl: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(AuthTokenCacheInner {
                entries: NonZeroUsize::new(capacity).map(LruCache::new),
            })),
            ttl,
        }
    }

    pub fn get_bearer_context(&self, raw_token: &str) -> Option<BearerTokenContext> {
        let key = build_cache_key(raw_token)?;
        let now = Instant::now();
        let inner = self
            .inner
            .read()
            .expect("auth token cache rwlock should not be poisoned");
        let entry = inner.entries.as_ref()?.peek(&key)?;

        if is_expired(entry, now, self.ttl) || is_context_token_expired(&entry.context) {
            drop(inner);
            self.remove_if_still_expired(&key, now);
            return None;
        }

        drop(inner);

        let mut inner = self
            .inner
            .write()
            .expect("auth token cache rwlock should not be poisoned");
        let entries = inner.entries.as_mut()?;
        let entry = entries.peek(&key)?;

        if is_expired(entry, now, self.ttl) || is_context_token_expired(&entry.context) {
            entries.pop(&key);
            return None;
        }

        let context = entry.context.clone();
        entries.promote(&key);
        Some(context)
    }

    pub fn get_admin_context(&self, raw_token: &str) -> Option<AdminAccessTokenContext> {
        match self.get_bearer_context(raw_token)? {
            BearerTokenContext::AdminUser(context) => Some(context),
            BearerTokenContext::ApiClient(_) => None,
        }
    }

    pub fn insert_bearer_context(&self, raw_token: &str, context: BearerTokenContext) {
        let Some(key) = build_cache_key(raw_token) else {
            return;
        };
        let now = Instant::now();
        let mut inner = self
            .inner
            .write()
            .expect("auth token cache rwlock should not be poisoned");
        let Some(entries) = inner.entries.as_mut() else {
            return;
        };

        if !entries.contains(&key) && entries.len() >= entries.cap().get() {
            evict_prefer_expired_entry(entries, now, self.ttl);
        }

        entries.put(
            key,
            AuthTokenCacheEntry {
                context,
                inserted_at: now,
            },
        );
    }

    pub fn invalidate_raw_token(&self, raw_token: &str) {
        let Some(key) = build_cache_key(raw_token) else {
            return;
        };
        self.invalidate_key(&key);
    }

    pub fn invalidate_hashed_token(&self, token_prefix: &str, token_hash: &str) {
        self.invalidate_key(&AuthTokenCacheKey {
            token_prefix: token_prefix.to_string(),
            token_hash: token_hash.to_string(),
        });
    }

    fn invalidate_key(&self, key: &AuthTokenCacheKey) {
        let mut inner = self
            .inner
            .write()
            .expect("auth token cache rwlock should not be poisoned");
        if let Some(entries) = inner.entries.as_mut() {
            entries.pop(key);
        }
    }

    fn remove_if_still_expired(&self, key: &AuthTokenCacheKey, now: Instant) {
        let mut inner = self
            .inner
            .write()
            .expect("auth token cache rwlock should not be poisoned");

        let Some(entries) = inner.entries.as_mut() else {
            return;
        };
        let Some(entry) = entries.peek(key) else {
            return;
        };

        if is_expired(entry, now, self.ttl) || is_context_token_expired(&entry.context) {
            entries.pop(key);
        }
    }
}

impl Default for AuthTokenCache {
    fn default() -> Self {
        Self::new()
    }
}

fn build_cache_key(raw_token: &str) -> Option<AuthTokenCacheKey> {
    if raw_token.is_empty() {
        return None;
    }

    Some(AuthTokenCacheKey {
        token_prefix: raw_token.chars().take(TOKEN_PREFIX_LEN).collect(),
        token_hash: crate::auth::service::hash_token(raw_token),
    })
}

fn is_expired(entry: &AuthTokenCacheEntry, now: Instant, ttl: Duration) -> bool {
    now.duration_since(entry.inserted_at) >= ttl
}

fn is_context_token_expired(context: &BearerTokenContext) -> bool {
    match context {
        BearerTokenContext::ApiClient(context) => context.token.expires_at <= Utc::now(),
        BearerTokenContext::AdminUser(context) => context.token.expires_at <= Utc::now(),
    }
}

fn evict_prefer_expired_entry(
    entries: &mut LruCache<AuthTokenCacheKey, AuthTokenCacheEntry>,
    now: Instant,
    ttl: Duration,
) {
    let expired_key = entries.iter().find_map(|(key, entry)| {
        (is_expired(entry, now, ttl) || is_context_token_expired(&entry.context))
            .then_some(key.clone())
    });

    if let Some(key_to_remove) = expired_key {
        entries.pop(&key_to_remove);
        return;
    }

    let _ = entries.pop_lru();
}

#[cfg(test)]
mod tests {
    use super::AuthTokenCache;
    use crate::{
        api::permissions::{PermissionKey, PermissionSet},
        auth::{AccessTokenContext, AdminAccessTokenContext, BearerTokenContext},
        domain::{
            admin_auth::AdminAccessToken,
            admin_users::AdminUser,
            api_auth::{AccessToken, ApiClient, ApiClientType},
        },
    };
    use chrono::{Duration as ChronoDuration, Utc};
    use sqlx::types::Uuid;
    use std::{thread::sleep, time::Duration};

    #[test]
    fn token_cache_returns_cached_bearer_context_until_ttl_expires() {
        let cache = AuthTokenCache::with_capacity_and_ttl(8, Duration::from_millis(20));
        let raw_token = "1234567890abcdef1234567890abcdef";

        cache.insert_bearer_context(raw_token, sample_api_bearer_context());

        assert!(cache.get_bearer_context(raw_token).is_some());
        sleep(Duration::from_millis(25));
        assert!(cache.get_bearer_context(raw_token).is_none());
    }

    #[test]
    fn token_cache_invalidates_by_raw_token() {
        let cache = AuthTokenCache::with_capacity_and_ttl(8, Duration::from_secs(30));
        let raw_token = "abcdef1234567890abcdef1234567890";

        cache.insert_bearer_context(raw_token, sample_api_bearer_context());
        cache.invalidate_raw_token(raw_token);

        assert!(cache.get_bearer_context(raw_token).is_none());
    }

    #[test]
    fn token_cache_eviction_keeps_newly_inserted_entry() {
        let cache = AuthTokenCache::with_capacity_and_ttl(1, Duration::from_secs(30));
        let first = "11111111111111111111111111111111";
        let second = "22222222222222222222222222222222";

        cache.insert_bearer_context(first, sample_api_bearer_context());
        cache.insert_bearer_context(second, sample_admin_bearer_context());

        assert!(cache.get_bearer_context(first).is_none());
        assert!(cache.get_bearer_context(second).is_some());
    }

    #[test]
    fn token_cache_evicts_least_recently_used_entry_when_full() {
        let cache = AuthTokenCache::with_capacity_and_ttl(2, Duration::from_secs(30));
        let first = "11111111111111111111111111111111";
        let second = "22222222222222222222222222222222";
        let third = "33333333333333333333333333333333";

        cache.insert_bearer_context(first, sample_api_bearer_context());
        cache.insert_bearer_context(second, sample_admin_bearer_context());
        assert!(cache.get_bearer_context(first).is_some());

        cache.insert_bearer_context(third, sample_api_bearer_context());

        assert!(cache.get_bearer_context(first).is_some());
        assert!(cache.get_bearer_context(second).is_none());
        assert!(cache.get_bearer_context(third).is_some());
    }

    #[test]
    fn token_cache_replaces_existing_entry_without_eviction() {
        let cache = AuthTokenCache::with_capacity_and_ttl(1, Duration::from_secs(30));
        let raw_token = "11111111111111111111111111111111";

        cache.insert_bearer_context(raw_token, sample_api_bearer_context());
        cache.insert_bearer_context(raw_token, sample_admin_bearer_context());

        assert!(cache.get_bearer_context(raw_token).is_some());
        assert!(cache.get_admin_context(raw_token).is_some());
    }

    #[test]
    fn token_cache_eviction_prefers_expired_entry_over_valid_entry() {
        let cache = AuthTokenCache::with_capacity_and_ttl(2, Duration::from_millis(20));
        let expired = "11111111111111111111111111111111";
        let valid = "22222222222222222222222222222222";
        let inserted = "33333333333333333333333333333333";

        cache.insert_bearer_context(expired, sample_api_bearer_context());
        std::thread::sleep(Duration::from_millis(25));
        cache.insert_bearer_context(valid, sample_admin_bearer_context());
        cache.insert_bearer_context(inserted, sample_api_bearer_context());

        assert!(cache.get_bearer_context(expired).is_none());
        assert!(cache.get_bearer_context(valid).is_some());
        assert!(cache.get_bearer_context(inserted).is_some());
    }

    #[test]
    fn token_cache_with_zero_capacity_ignores_inserts() {
        let cache = AuthTokenCache::with_capacity_and_ttl(0, Duration::from_secs(30));
        let raw_token = "11111111111111111111111111111111";

        cache.insert_bearer_context(raw_token, sample_api_bearer_context());

        assert!(cache.get_bearer_context(raw_token).is_none());
    }

    fn sample_api_bearer_context() -> BearerTokenContext {
        BearerTokenContext::ApiClient(AccessTokenContext {
            token: AccessToken {
                id: 1,
                api_client_id: 1,
                token_hash: "hash".to_string(),
                token_prefix: "123456789012".to_string(),
                expires_at: Utc::now() + ChronoDuration::minutes(5),
                revoked_at: None,
                revoked_reason: None,
                last_used_at: None,
                created_at: Utc::now(),
            },
            client: ApiClient {
                id: 1,
                uuid: Uuid::nil(),
                name: "client".to_string(),
                description: None,
                client_type: ApiClientType::InstallationClient,
                client_id: "client-id".to_string(),
                client_secret_hash: "secret".to_string(),
                secret_prefix: "secret".to_string(),
                is_active: true,
                last_used_at: None,
                created_by_user_id: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            scopes: vec!["sites:read".to_string()],
            permissions: PermissionSet::from_keys([PermissionKey::SitesRead]),
        })
    }

    fn sample_admin_bearer_context() -> BearerTokenContext {
        BearerTokenContext::AdminUser(AdminAccessTokenContext {
            token: AdminAccessToken {
                id: 1,
                admin_user_id: 1,
                token_hash: "hash".to_string(),
                token_prefix: "123456789012".to_string(),
                expires_at: Utc::now() + ChronoDuration::minutes(5),
                revoked_at: None,
                revoked_reason: None,
                last_used_at: None,
                created_at: Utc::now(),
            },
            user: AdminUser {
                id: 1,
                uuid: Uuid::nil(),
                email: "admin@test.com".to_string(),
                display_name: "Admin".to_string(),
                password_hash: "hash".to_string(),
                is_active: true,
                last_login_at: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            roles: vec!["viewer".to_string()],
            permissions: PermissionSet::from_keys([PermissionKey::SitesRead]),
        })
    }
}
