use crate::error::Result;
use redis::{
    AsyncCommands, Client,
    aio::{ConnectionManager, PubSub},
};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct RedisClient {
    manager: Arc<Mutex<ConnectionManager>>,
    client: Client,
}

impl RedisClient {
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = Client::open(redis_url)?;
        let manager = ConnectionManager::new(client.clone()).await?;
        Ok(Self {
            manager: Arc::new(Mutex::new(manager)),
            client,
        })
    }

    // Rate limiting
    pub async fn check_rate_limit(
        &self,
        key: &str,
        limit: u32,
        window_seconds: usize,
    ) -> Result<bool> {
        let mut conn = self.manager.lock().await;

        let current: u32 = conn.get(key).await.unwrap_or(0);

        if current >= limit {
            return Ok(false);
        }

        let _: () = conn.incr(key, 1).await?;
        let _: () = conn.expire(key, window_seconds as i64).await?;

        Ok(true)
    }

    // Session management
    pub async fn store_session(
        &self,
        session_id: &str,
        user_id: &str,
        ttl_seconds: usize,
    ) -> Result<()> {
        let mut conn = self.manager.lock().await;
        let key = format!("session:{}", session_id);

        let _: () = conn.set_ex(key, user_id, ttl_seconds as u64).await?;
        Ok(())
    }

    pub async fn get_session(&self, session_id: &str) -> Result<Option<String>> {
        let mut conn = self.manager.lock().await;
        let key = format!("session:{}", session_id);

        let user_id: Option<String> = conn.get(key).await?;
        Ok(user_id)
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let mut conn = self.manager.lock().await;
        let key = format!("session:{}", session_id);

        let _: () = conn.del(key).await?;
        Ok(())
    }

    // Caching
    pub async fn cache_set(&self, key: &str, value: &str, ttl_seconds: usize) -> Result<()> {
        let mut conn = self.manager.lock().await;
        let _: () = conn.set_ex(key, value, ttl_seconds as u64).await?;
        Ok(())
    }

    pub async fn cache_get(&self, key: &str) -> Result<Option<String>> {
        let mut conn = self.manager.lock().await;
        let value: Option<String> = conn.get(key).await?;
        Ok(value)
    }

    pub async fn cache_delete(&self, key: &str) -> Result<()> {
        let mut conn = self.manager.lock().await;
        let _: () = conn.del(key).await?;
        Ok(())
    }

    // Real-time features
    pub async fn publish(&self, channel: &str, message: &str) -> Result<()> {
        let mut conn = self.manager.lock().await;
        let _: () = conn.publish(channel, message).await?;
        Ok(())
    }

    // Typing indicators
    pub async fn set_typing_indicator(&self, post_id: &str, user_id: &str) -> Result<()> {
        let mut conn = self.manager.lock().await;
        let key = format!("typing:{}:{}", post_id, user_id);

        let _: () = conn.set_ex(key, "typing", 30).await?;
        Ok(())
    }

    pub async fn get_typing_users(&self, post_id: &str) -> Result<Vec<String>> {
        let mut conn = self.manager.lock().await;
        let pattern = format!("typing:{}:*", post_id);

        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(pattern)
            .query_async(&mut *conn)
            .await?;
        let user_ids: Vec<String> = keys
            .into_iter()
            .filter_map(|key| key.split(':').nth(2).map(|s| s.to_string()))
            .collect();

        Ok(user_ids)
    }

    // Community online users tracking
    pub async fn add_user_to_community_online(
        &self,
        community_id: &str,
        user_id: &str,
    ) -> Result<()> {
        let mut conn = self.manager.lock().await;
        let key = format!("community_online:{}", community_id);

        let _: () = conn.sadd(&key, user_id).await?;
        let _: () = conn.expire(&key, 300).await?;
        Ok(())
    }

    pub async fn remove_user_from_community_online(
        &self,
        community_id: &str,
        user_id: &str,
    ) -> Result<()> {
        let mut conn = self.manager.lock().await;
        let key = format!("community_online:{}", community_id);

        let _: () = conn.srem(&key, user_id).await?;
        Ok(())
    }

    pub async fn get_community_online_count(&self, community_id: &str) -> Result<u32> {
        let mut conn = self.manager.lock().await;
        let key = format!("community_online:{}", community_id);

        let count: u32 = conn.scard(&key).await?;
        Ok(count)
    }

    pub async fn get_community_online_users(&self, community_id: &str) -> Result<Vec<String>> {
        let mut conn = self.manager.lock().await;
        let key = format!("community_online:{}", community_id);

        let users: Vec<String> = conn.smembers(&key).await?;
        Ok(users)
    }

    // User presence tracking
    pub async fn set_user_online(&self, user_id: &str) -> Result<()> {
        let mut conn = self.manager.lock().await;
        let key = format!("user_online:{}", user_id);

        let _: () = conn.set_ex(&key, "online", 300).await?;
        Ok(())
    }

    pub async fn is_user_online(&self, user_id: &str) -> Result<bool> {
        let mut conn = self.manager.lock().await;
        let key = format!("user_online:{}", user_id);

        let exists: bool = conn.exists(&key).await?;
        Ok(exists)
    }

    // Notification queuing for offline users
    pub async fn queue_notification(&self, user_id: &str, notification: &str) -> Result<()> {
        let mut conn = self.manager.lock().await;
        let key = format!("notification_queue:{}", user_id);

        let _: () = conn.lpush(&key, notification).await?;
        let _: () = conn.expire(&key, 86400).await?;
        Ok(())
    }

    pub async fn get_queued_notifications(&self, user_id: &str) -> Result<Vec<String>> {
        let mut conn = self.manager.lock().await;
        let key = format!("notification_queue:{}", user_id);

        let notifications: Vec<String> = conn.lrange(&key, 0, -1).await?;
        let _: () = conn.del(&key).await?;
        Ok(notifications)
    }

    // Real-time post view tracking
    pub async fn track_post_view(&self, post_id: &str, user_id: Option<&str>) -> Result<()> {
        let mut conn = self.manager.lock().await;

        let view_key = format!("post_views:{}", post_id);
        let _: () = conn.incr(&view_key, 1).await?;
        let _: () = conn.expire(&view_key, 3600).await?;

        if let Some(uid) = user_id {
            let unique_key = format!("post_unique_views:{}", post_id);
            let _: () = conn.sadd(&unique_key, uid).await?;
            let _: () = conn.expire(&unique_key, 3600).await?;
        }

        Ok(())
    }

    pub async fn get_post_view_count(&self, post_id: &str) -> Result<u32> {
        let mut conn = self.manager.lock().await;
        let key = format!("post_views:{}", post_id);

        let count: Option<u32> = conn.get(&key).await?;
        Ok(count.unwrap_or(0))
    }

    // Trending topics tracking
    pub async fn track_search_term(&self, term: &str) -> Result<()> {
        let mut conn = self.manager.lock().await;
        let key = "trending_searches";

        let _: () = conn.zincr(&key, term, 1).await?;
        let _: () = conn.expire(&key, 86400).await?;
        Ok(())
    }

    pub async fn get_trending_searches(&self, limit: usize) -> Result<Vec<String>> {
        let mut conn = self.manager.lock().await;
        let key = "trending_searches";

        let trending: Vec<String> = conn.zrevrange(&key, 0, (limit as isize) - 1).await?;
        Ok(trending)
    }

    // WebSocket connection tracking
    pub async fn register_websocket_connection(
        &self,
        user_id: &str,
        connection_id: &str,
    ) -> Result<()> {
        let mut conn = self.manager.lock().await;
        let key = format!("ws_connections:{}", user_id);

        let _: () = conn.sadd(&key, connection_id).await?;
        let _: () = conn.expire(&key, 3600).await?;
        Ok(())
    }

    pub async fn unregister_websocket_connection(
        &self,
        user_id: &str,
        connection_id: &str,
    ) -> Result<()> {
        let mut conn = self.manager.lock().await;
        let key = format!("ws_connections:{}", user_id);

        let _: () = conn.srem(&key, connection_id).await?;
        Ok(())
    }

    pub async fn get_user_websocket_connections(&self, user_id: &str) -> Result<Vec<String>> {
        let mut conn = self.manager.lock().await;
        let key = format!("ws_connections:{}", user_id);

        let connections: Vec<String> = conn.smembers(&key).await?;
        Ok(connections)
    }
    pub async fn subscribe(&self, channels: Vec<&str>) -> Result<PubSub> {
        let mut pubsub = self.client.get_async_pubsub().await?;

        for channel in channels {
            pubsub.subscribe(channel).await?;
        }

        Ok(pubsub)
    }
}
