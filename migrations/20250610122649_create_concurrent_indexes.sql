-- Add migration script here
CREATE INDEX idx_posts_community_hot_active ON posts (
    community_id,
    hot_score DESC,
    status
)
WHERE
    status = 'active';

CREATE INDEX idx_posts_trending ON posts (created_at DESC, score DESC)
WHERE
    status = 'active';

CREATE INDEX idx_comments_post_tree ON comments (post_id, path, status)
WHERE
    status = 'active';

-- Partial indexes for better performance
CREATE INDEX idx_notifications_unread_recent ON notifications (recipient_id, created_at DESC)
WHERE
    is_read = FALSE;

CREATE INDEX idx_typing_indicators_active ON comment_typing_indicators (post_id, parent_comment_id);