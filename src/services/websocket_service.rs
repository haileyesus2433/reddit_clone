use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::Response,
};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{error::Result, redis::RedisClient, services::typing_service::TypingService};

#[derive(Clone)]
pub struct WebSocketService {
    connections: Arc<Mutex<HashMap<Uuid, broadcast::Sender<String>>>>,
    typing_service: TypingService,
    redis: Arc<RedisClient>,
}

impl WebSocketService {
    pub fn new(typing_service: TypingService, redis: Arc<RedisClient>) -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            typing_service,
            redis,
        }
    }

    /// Handle WebSocket upgrade
    pub fn handle_websocket(self: Arc<Self>, ws: WebSocketUpgrade, user_id: Uuid) -> Response {
        ws.on_upgrade(move |socket| {
            let this = Arc::clone(&self);
            async move { this.handle_socket(socket, user_id).await }
        })
    }

    /// Handle individual WebSocket connection
    async fn handle_socket(&self, socket: WebSocket, user_id: Uuid) {
        let (mut sender, mut receiver) = socket.split();
        let connection_id = Uuid::new_v4().to_string();

        // Register connection in Redis
        let _ = self
            .redis
            .register_websocket_connection(&user_id.to_string(), &connection_id)
            .await;
        let _ = self.redis.set_user_online(&user_id.to_string()).await;

        // Create broadcast channel for this user
        let (tx, mut rx) = broadcast::channel(100);
        // Store connection
        {
            let mut connections = self.connections.lock().unwrap();
            connections.insert(user_id, tx.clone());
        }

        // Send initial connection confirmation and queued notifications
        let welcome_msg = serde_json::json!({
            "type": "connected",
            "user_id": user_id,
            "connection_id": connection_id,
            "timestamp": chrono::Utc::now()
        });

        if sender
            .send(Message::Text(welcome_msg.to_string().into()))
            .await
            .is_err()
        {
            return;
        }

        // Send any queued notifications
        if let Ok(queued) = self
            .redis
            .get_queued_notifications(&user_id.to_string())
            .await
        {
            for notification in queued {
                let _ = sender
                    .send(Message::Text(notification.to_string().into()))
                    .await;
            }
        }

        // Spawn task to handle Redis pub/sub messages
        let redis_clone = self.redis.clone();
        let tx_clone = tx.clone();
        let user_id_str = user_id.to_string();
        let pubsub_task = tokio::spawn(async move {
            if let Ok(mut pubsub) = redis_clone
                .subscribe(vec![
                    &format!("user_notifications:{}", user_id_str),
                    &format!("global_notifications"),
                ])
                .await
            {
                while let Some(msg) = pubsub.on_message().next().await {
                    if let Ok(payload) = msg.get_payload::<String>() {
                        let _ = tx_clone.send(payload);
                    }
                }
            }
        });

        // Spawn task to handle outgoing messages
        let outgoing_task = tokio::spawn(async move {
            while let Ok(msg) = rx.recv().await {
                if sender
                    .send(Message::Text(msg.to_string().into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        // Handle incoming messages
        let typing_service = self.typing_service.clone();
        let redis = self.redis.clone();
        let incoming_task = tokio::spawn(async move {
            while let Some(msg) = receiver.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(ws_message) = serde_json::from_str::<WebSocketMessage>(&text) {
                            Self::handle_websocket_message(
                                ws_message,
                                user_id,
                                &typing_service,
                                &redis,
                            )
                            .await;
                        }
                    }
                    Ok(Message::Ping(_data)) => {
                        // Respond to ping with pong to keep connection alive
                        // This would be handled automatically by the WebSocket implementation
                    }
                    Ok(Message::Close(_)) => break,
                    _ => {}
                }
            }
        });

        // Wait for any task to complete
        tokio::select! {
            _ = outgoing_task => {},
            _ = incoming_task => {},
            _ = pubsub_task => {},
        }

        // Clean up connection
        {
            let mut connections = self.connections.lock().unwrap();
            connections.remove(&user_id);
        }

        // Clean up Redis tracking
        let _ = self
            .redis
            .unregister_websocket_connection(&user_id.to_string(), &connection_id)
            .await;
        let _ = self
            .typing_service
            .cleanup_user_typing_indicators(user_id)
            .await;
    }

    /// Handle incoming WebSocket messages
    async fn handle_websocket_message(
        message: WebSocketMessage,
        user_id: Uuid,
        typing_service: &TypingService,
        redis: &RedisClient,
    ) {
        match message.message_type.as_str() {
            "start_typing" => {
                if let (Some(post_id), parent_comment_id) =
                    (message.post_id, message.parent_comment_id)
                {
                    let _ = typing_service
                        .start_typing(user_id, post_id, parent_comment_id)
                        .await;
                }
            }
            "stop_typing" => {
                if let (Some(post_id), parent_comment_id) =
                    (message.post_id, message.parent_comment_id)
                {
                    let _ = typing_service
                        .stop_typing(user_id, post_id, parent_comment_id)
                        .await;
                }
            }
            "typing_heartbeat" => {
                if let (Some(post_id), parent_comment_id) =
                    (message.post_id, message.parent_comment_id)
                {
                    let _ = typing_service
                        .update_typing_activity(user_id, post_id, parent_comment_id)
                        .await;
                }
            }
            "subscribe_to_post" => {
                if let Some(post_id) = message.post_id {
                    // Subscribe user to post notifications via Redis
                    let _channel = format!("post_updates:{}", post_id);
                    // This would be handled by the pub/sub system
                }
            }
            "join_community" => {
                if let Some(community_id) = message.community_id {
                    let _ = redis
                        .add_user_to_community_online(
                            &community_id.to_string(),
                            &user_id.to_string(),
                        )
                        .await;
                }
            }
            "leave_community" => {
                if let Some(community_id) = message.community_id {
                    let _ = redis
                        .remove_user_from_community_online(
                            &community_id.to_string(),
                            &user_id.to_string(),
                        )
                        .await;
                }
            }
            "heartbeat" => {
                // Update user online status
                let _ = redis.set_user_online(&user_id.to_string()).await;
            }
            _ => {}
        }
    }

    /// Send notification to user via WebSocket or queue if offline
    pub async fn send_notification_to_user(&self, user_id: Uuid, notification: &str) -> Result<()> {
        // Try to send via active WebSocket connection first
        {
            let connections = self.connections.lock().unwrap();
            if let Some(tx) = connections.get(&user_id) {
                if tx.send(notification.to_string()).is_ok() {
                    return Ok(());
                }
            }
        }

        // If no active connection, queue the notification in Redis
        self.redis
            .queue_notification(&user_id.to_string(), notification)
            .await?;

        Ok(())
    }

    /// Broadcast to all users in a community
    pub async fn broadcast_to_community(&self, community_id: Uuid, message: &str) -> Result<()> {
        let channel = format!("community_updates:{}", community_id);
        self.redis.publish(&channel, message).await?;
        Ok(())
    }

    /// Send message to specific user
    pub async fn send_to_user(&self, user_id: Uuid, message: &str) -> Result<()> {
        let channel = format!("user_notifications:{}", user_id);
        self.redis.publish(&channel, message).await?;
        Ok(())
    }

    /// Broadcast message to all connected users

    pub async fn broadcast_global(&self, message: &str) -> Result<()> {
        self.redis.publish("global_notifications", message).await?;
        Ok(())
    }

    /// Get online user count
    pub fn get_online_count(&self) -> usize {
        let connections = self.connections.lock().unwrap();
        connections.len()
    }

    /// Get online users for a community

    pub async fn get_community_online_users(&self, community_id: Uuid) -> Result<Vec<String>> {
        self.redis
            .get_community_online_users(&community_id.to_string())
            .await
    }

    /// Get community online count
    pub async fn get_community_online_count(&self, community_id: Uuid) -> Result<u32> {
        self.redis
            .get_community_online_count(&community_id.to_string())
            .await
    }
}

#[derive(Debug, Deserialize)]
struct WebSocketMessage {
    #[serde(rename = "type")]
    message_type: String,
    post_id: Option<Uuid>,
    parent_comment_id: Option<Uuid>,
    community_id: Option<Uuid>,
    data: Option<serde_json::Value>,
}
