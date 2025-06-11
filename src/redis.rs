use crate::error::Result;
use redis::{Client, Connection};

pub struct RedisClient {
    client: Client,
}

impl RedisClient {
    pub fn new(redis_url: &str) -> Result<Self> {
        let client = Client::open(redis_url)?;
        Ok(Self { client })
    }

    pub fn get_connection(&self) -> Result<Connection> {
        Ok(self.client.get_connection()?)
    }

    // Rate limiting
    pub async fn check_rate_limit(
        &self,
        key: &str,
        limit: u32,
        window_seconds: u32,
    ) -> Result<bool> {
        let mut conn = self.get_connection()?;

        let current: u32 = redis::cmd("GET").arg(key).query(&mut conn).unwrap_or(0);

        if current >= limit {
            return Ok(false);
        }

        let _: () = redis::cmd("INCR").arg(key).query(&mut conn)?;

        let _: () = redis::cmd("EXPIRE")
            .arg(key)
            .arg(window_seconds)
            .query(&mut conn)?;

        Ok(true)
    }

    // Session management
    pub async fn store_session(
        &self,
        session_id: &str,
        user_id: &str,
        ttl_seconds: u32,
    ) -> Result<()> {
        let mut conn = self.get_connection()?;

        let _: () = redis::cmd("SETEX")
            .arg(format!("session:{}", session_id))
            .arg(ttl_seconds)
            .arg(user_id)
            .query(&mut conn)?;

        Ok(())
    }

    pub async fn get_session(&self, session_id: &str) -> Result<Option<String>> {
        let mut conn = self.get_connection()?;

        let user_id: Option<String> = redis::cmd("GET")
            .arg(format!("session:{}", session_id))
            .query(&mut conn)?;

        Ok(user_id)
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let mut conn = self.get_connection()?;

        let _: () = redis::cmd("DEL")
            .arg(format!("session:{}", session_id))
            .query(&mut conn)?;

        Ok(())
    }

    // Caching
    pub async fn cache_set(&self, key: &str, value: &str, ttl_seconds: u32) -> Result<()> {
        let mut conn = self.get_connection()?;

        let _: () = redis::cmd("SETEX")
            .arg(key)
            .arg(ttl_seconds)
            .arg(value)
            .query(&mut conn)?;

        Ok(())
    }

    pub async fn cache_get(&self, key: &str) -> Result<Option<String>> {
        let mut conn = self.get_connection()?;

        let value: Option<String> = redis::cmd("GET").arg(key).query(&mut conn)?;

        Ok(value)
    }

    pub async fn cache_delete(&self, key: &str) -> Result<()> {
        let mut conn = self.get_connection()?;

        let _: () = redis::cmd("DEL").arg(key).query(&mut conn)?;

        Ok(())
    }

    // Real-time features
    pub async fn publish(&self, channel: &str, message: &str) -> Result<()> {
        let mut conn = self.get_connection()?;

        let _: () = redis::cmd("PUBLISH")
            .arg(channel)
            .arg(message)
            .query(&mut conn)?;

        Ok(())
    }

    // Typing indicators
    pub async fn set_typing_indicator(&self, post_id: &str, user_id: &str) -> Result<()> {
        let mut conn = self.get_connection()?;
        let key = format!("typing:{}:{}", post_id, user_id);

        let _: () = redis::cmd("SETEX")
            .arg(key)
            .arg(30) // 30 seconds TTL
            .arg("typing")
            .query(&mut conn)?;

        Ok(())
    }

    pub async fn get_typing_users(&self, post_id: &str) -> Result<Vec<String>> {
        let mut conn = self.get_connection()?;
        let pattern = format!("typing:{}:*", post_id);

        let keys: Vec<String> = redis::cmd("KEYS").arg(pattern).query(&mut conn)?;

        let user_ids: Vec<String> = keys
            .into_iter()
            .filter_map(|key| key.split(':').nth(2).map(|s| s.to_string()))
            .collect();

        Ok(user_ids)
    }
}
