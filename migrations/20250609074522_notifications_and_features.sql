-- Notification types
CREATE TYPE notification_type AS ENUM (
    'comment_reply', 'post_reply', 'mention', 'upvote', 'downvote',
    'community_invite', 'community_ban', 'post_removed', 'comment_removed'
);

-- Notifications table
CREATE TABLE notifications (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    recipient_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    sender_id UUID REFERENCES users (id) ON DELETE SET NULL,
    notification_type notification_type NOT NULL,
    title VARCHAR(255) NOT NULL,
    content TEXT,
    is_read BOOLEAN DEFAULT FALSE,
    post_id UUID REFERENCES posts (id) ON DELETE CASCADE,
    comment_id UUID REFERENCES comments (id) ON DELETE CASCADE,
    community_id UUID REFERENCES communities (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- User preferences
CREATE TABLE user_preferences (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE UNIQUE,
    email_notifications BOOLEAN DEFAULT TRUE,
    push_notifications BOOLEAN DEFAULT TRUE,
    comment_reply_notifications BOOLEAN DEFAULT TRUE,
    post_reply_notifications BOOLEAN DEFAULT TRUE,
    mention_notifications BOOLEAN DEFAULT TRUE,
    upvote_notifications BOOLEAN DEFAULT FALSE,
    community_notifications BOOLEAN DEFAULT TRUE,
    nsfw_content BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- User follows (for following other users)
CREATE TABLE user_follows (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    follower_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    following_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE (follower_id, following_id),
    CHECK (follower_id != following_id)
);

-- User blocks (for blocking other users)
CREATE TABLE user_blocks (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    blocker_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    blocked_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE (blocker_id, blocked_id),
    CHECK (blocker_id != blocked_id)
);

-- Saved posts
CREATE TABLE saved_posts (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    post_id UUID NOT NULL REFERENCES posts (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE (user_id, post_id)
);

-- Saved comments
CREATE TABLE saved_comments (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    comment_id UUID NOT NULL REFERENCES comments (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE (user_id, comment_id)
);

-- Post shares tracking
CREATE TABLE post_shares (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    post_id UUID NOT NULL REFERENCES posts (id) ON DELETE CASCADE,
    user_id UUID REFERENCES users (id) ON DELETE SET NULL,
    ip_address INET,
    shared_at TIMESTAMPTZ DEFAULT NOW()
);

-- User karma history (for tracking karma changes)
CREATE TABLE user_karma_history (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    karma_change INTEGER NOT NULL,
    reason VARCHAR(100) NOT NULL, -- 'post_upvote', 'comment_upvote', etc.
    post_id UUID REFERENCES posts (id) ON DELETE SET NULL,
    comment_id UUID REFERENCES comments (id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Community flairs
CREATE TABLE community_flairs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    community_id UUID NOT NULL REFERENCES communities (id) ON DELETE CASCADE,
    text VARCHAR(100) NOT NULL,
    background_color VARCHAR(7), -- hex color
    text_color VARCHAR(7), -- hex color
    is_mod_only BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- User flairs in communities
CREATE TABLE user_community_flairs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    community_id UUID NOT NULL REFERENCES communities (id) ON DELETE CASCADE,
    flair_id UUID REFERENCES community_flairs (id) ON DELETE SET NULL,
    custom_text VARCHAR(100),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE (user_id, community_id)
);

-- Post flairs
CREATE TABLE post_flairs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    post_id UUID NOT NULL REFERENCES posts (id) ON DELETE CASCADE,
    flair_id UUID NOT NULL REFERENCES community_flairs (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Trending topics/hashtags
CREATE TABLE trending_topics (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    topic VARCHAR(100) NOT NULL UNIQUE,
    mention_count INTEGER DEFAULT 1,
    last_mentioned_at TIMESTAMPTZ DEFAULT NOW(),
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Awards/badges system
CREATE TABLE awards (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    name VARCHAR(100) NOT NULL UNIQUE,
    description TEXT,
    icon_url VARCHAR(500),
    cost INTEGER DEFAULT 0, -- if implementing premium features
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- User awards given to posts/comments
CREATE TABLE user_awards (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    award_id UUID NOT NULL REFERENCES awards (id),
    giver_id UUID REFERENCES users (id) ON DELETE SET NULL,
    recipient_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    post_id UUID REFERENCES posts (id) ON DELETE CASCADE,
    comment_id UUID REFERENCES comments (id) ON DELETE CASCADE,
    message TEXT,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    CHECK (
        (
            post_id IS NOT NULL
            AND comment_id IS NULL
        )
        OR (
            post_id IS NULL
            AND comment_id IS NOT NULL
        )
    )
);

-- Indexes for notifications
CREATE INDEX idx_notifications_recipient_id ON notifications (recipient_id);

CREATE INDEX idx_notifications_sender_id ON notifications (sender_id)
WHERE
    sender_id IS NOT NULL;

CREATE INDEX idx_notifications_type ON notifications (notification_type);

CREATE INDEX idx_notifications_is_read ON notifications (is_read);

CREATE INDEX idx_notifications_created_at ON notifications (created_at DESC);

CREATE INDEX idx_notifications_recipient_unread ON notifications (recipient_id, is_read)
WHERE
    is_read = FALSE;

-- Indexes for user preferences
CREATE INDEX idx_user_preferences_user_id ON user_preferences (user_id);

-- Indexes for user follows
CREATE INDEX idx_user_follows_follower_id ON user_follows (follower_id);

CREATE INDEX idx_user_follows_following_id ON user_follows (following_id);

-- Indexes for user blocks
CREATE INDEX idx_user_blocks_blocker_id ON user_blocks (blocker_id);

CREATE INDEX idx_user_blocks_blocked_id ON user_blocks (blocked_id);

-- Indexes for saved content
CREATE INDEX idx_saved_posts_user_id ON saved_posts (user_id);

CREATE INDEX idx_saved_posts_created_at ON saved_posts (created_at DESC);

CREATE INDEX idx_saved_comments_user_id ON saved_comments (user_id);

CREATE INDEX idx_saved_comments_created_at ON saved_comments (created_at DESC);

-- Indexes for post shares
CREATE INDEX idx_post_shares_post_id ON post_shares (post_id);

CREATE INDEX idx_post_shares_user_id ON post_shares (user_id)
WHERE
    user_id IS NOT NULL;

CREATE INDEX idx_post_shares_shared_at ON post_shares (shared_at);

-- Indexes for karma history
CREATE INDEX idx_user_karma_history_user_id ON user_karma_history (user_id);

CREATE INDEX idx_user_karma_history_created_at ON user_karma_history (created_at DESC);

-- Indexes for flairs
CREATE INDEX idx_community_flairs_community_id ON community_flairs (community_id);

CREATE INDEX idx_user_community_flairs_user_id ON user_community_flairs (user_id);

CREATE INDEX idx_user_community_flairs_community_id ON user_community_flairs (community_id);

CREATE INDEX idx_post_flairs_post_id ON post_flairs (post_id);

-- Indexes for trending topics
CREATE INDEX idx_trending_topics_mention_count ON trending_topics (mention_count DESC);

CREATE INDEX idx_trending_topics_last_mentioned ON trending_topics (last_mentioned_at DESC);

-- Indexes for awards
CREATE INDEX idx_user_awards_recipient_id ON user_awards (recipient_id);

CREATE INDEX idx_user_awards_giver_id ON user_awards (giver_id)
WHERE
    giver_id IS NOT NULL;

CREATE INDEX idx_user_awards_post_id ON user_awards (post_id)
WHERE
    post_id IS NOT NULL;

CREATE INDEX idx_user_awards_comment_id ON user_awards (comment_id)
WHERE
    comment_id IS NOT NULL;

-- Triggers
CREATE TRIGGER update_user_preferences_updated_at BEFORE UPDATE ON user_preferences
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Function to update user karma
CREATE OR REPLACE FUNCTION update_user_karma()
RETURNS TRIGGER AS $$
DECLARE
    karma_change INTEGER;
    target_user_id UUID;
    reason TEXT;
BEGIN
    IF TG_OP = 'INSERT' THEN
        karma_change := CASE WHEN NEW.vote_type = 1 THEN 1 ELSE -1 END;
        
        -- Determine if this is a post or comment vote
        IF TG_TABLE_NAME = 'post_votes' THEN
            SELECT author_id INTO target_user_id FROM posts WHERE id = NEW.post_id;
            reason := CASE WHEN NEW.vote_type = 1 THEN 'post_upvote' ELSE 'post_downvote' END;
            
            -- Insert karma history
            INSERT INTO user_karma_history (user_id, karma_change, reason, post_id)
            VALUES (target_user_id, karma_change, reason, NEW.post_id);
        ELSE
            SELECT author_id INTO target_user_id FROM comments WHERE id = NEW.comment_id;
            reason := CASE WHEN NEW.vote_type = 1 THEN 'comment_upvote' ELSE 'comment_downvote' END;
            
            -- Insert karma history
            INSERT INTO user_karma_history (user_id, karma_change, reason, comment_id)
            VALUES (target_user_id, karma_change, reason, NEW.comment_id);
        END IF;
        
        -- Update user karma
        UPDATE users SET karma_points = karma_points + karma_change WHERE id = target_user_id;
        
        RETURN NEW;
    ELSIF TG_OP = 'UPDATE' THEN
        -- Handle vote change
        karma_change := CASE 
            WHEN OLD.vote_type = 1 AND NEW.vote_type = -1 THEN -2
            WHEN OLD.vote_type = -1 AND NEW.vote_type = 1 THEN 2
            ELSE 0
        END;
        
        IF karma_change != 0 THEN
            IF TG_TABLE_NAME = 'post_votes' THEN
                SELECT author_id INTO target_user_id FROM posts WHERE id = NEW.post_id;
                reason := CASE WHEN NEW.vote_type = 1 THEN 'post_upvote' ELSE 'post_downvote' END;
                
                INSERT INTO user_karma_history (user_id, karma_change, reason, post_id)
                VALUES (target_user_id, karma_change, reason, NEW.post_id);
            ELSE
                SELECT author_id INTO target_user_id FROM comments WHERE id = NEW.comment_id;
                reason := CASE WHEN NEW.vote_type = 1 THEN 'comment_upvote' ELSE 'comment_downvote' END;
                
                INSERT INTO user_karma_history (user_id, karma_change, reason, comment_id)
                VALUES (target_user_id, karma_change, reason, NEW.comment_id);
            END IF;
            
            UPDATE users SET karma_points = karma_points + karma_change WHERE id = target_user_id;
        END IF;
        
        RETURN NEW;
    ELSIF TG_OP = 'DELETE' THEN
        karma_change := CASE WHEN OLD.vote_type = 1 THEN -1 ELSE 1 END;
        
        IF TG_TABLE_NAME = 'post_votes' THEN
            SELECT author_id INTO target_user_id FROM posts WHERE id = OLD.post_id;
            reason := CASE WHEN OLD.vote_type = 1 THEN 'post_upvote_removed' ELSE 'post_downvote_removed' END;
            
            INSERT INTO user_karma_history (user_id, karma_change, reason, post_id)
            VALUES (target_user_id, karma_change, reason, OLD.post_id);
        ELSE
            SELECT author_id INTO target_user_id FROM comments WHERE id = OLD.comment_id;
            reason := CASE WHEN OLD.vote_type = 1 THEN 'comment_upvote_removed' ELSE 'comment_downvote_removed' END;
            
            INSERT INTO user_karma_history (user_id, karma_change, reason, comment_id)
            VALUES (target_user_id, karma_change, reason, OLD.comment_id);
        END IF;
        
        UPDATE users SET karma_points = karma_points + karma_change WHERE id = target_user_id;
        
        RETURN OLD;
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- Triggers for karma updates
CREATE TRIGGER update_user_karma_post_votes
    AFTER INSERT OR UPDATE OR DELETE ON post_votes
    FOR EACH ROW EXECUTE FUNCTION update_user_karma();

CREATE TRIGGER update_user_karma_comment_votes
    AFTER INSERT OR UPDATE OR DELETE ON comment_votes
    FOR EACH ROW EXECUTE FUNCTION update_user_karma();

-- Function to create notification for replies
CREATE OR REPLACE FUNCTION create_reply_notification()
RETURNS TRIGGER AS $$
DECLARE
    recipient_user_id UUID;
    notification_title TEXT;
    post_title TEXT;
BEGIN
    IF TG_OP = 'INSERT' AND NEW.status = 'active' THEN
        IF NEW.parent_comment_id IS NOT NULL THEN
            -- Reply to comment
            SELECT author_id INTO recipient_user_id FROM comments WHERE id = NEW.parent_comment_id;
            SELECT title INTO post_title FROM posts WHERE id = NEW.post_id;
            notification_title := 'New reply to your comment';
            
            -- Don't notify if replying to own comment
            IF recipient_user_id != NEW.author_id THEN
                INSERT INTO notifications (recipient_id, sender_id, notification_type, title, content, comment_id, post_id)
                VALUES (recipient_user_id, NEW.author_id, 'comment_reply', notification_title, 
                       LEFT(NEW.content, 200), NEW.id, NEW.post_id);
            END IF;
        ELSE
            -- Top-level comment (reply to post)
            SELECT author_id, title INTO recipient_user_id, post_title FROM posts WHERE id = NEW.post_id;
            notification_title := 'New comment on your post: ' || LEFT(post_title, 50);
            
            -- Don't notify if commenting on own post
            IF recipient_user_id != NEW.author_id THEN
                INSERT INTO notifications (recipient_id, sender_id, notification_type, title, content, comment_id, post_id)
                VALUES (recipient_user_id, NEW.author_id, 'post_reply', notification_title, 
                       LEFT(NEW.content, 200), NEW.id, NEW.post_id);
            END IF;
        END IF;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger for reply notifications
CREATE TRIGGER create_reply_notification_trigger
    AFTER INSERT ON comments
    FOR EACH ROW EXECUTE FUNCTION create_reply_notification();

-- Function to update post share count
CREATE OR REPLACE FUNCTION update_post_share_count()
RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        UPDATE posts SET share_count = share_count + 1 WHERE id = NEW.post_id;
        RETURN NEW;
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- Trigger for post share count
CREATE TRIGGER update_post_share_count_trigger
    AFTER INSERT ON post_shares
    FOR EACH ROW EXECUTE FUNCTION update_post_share_count();

-- Insert default awards
INSERT INTO
    awards (name, description, icon_url)
VALUES (
        'Helpful',
        'For helpful posts and comments',
        '/icons/helpful.svg'
    ),
    (
        'Wholesome',
        'For wholesome content',
        '/icons/wholesome.svg'
    ),
    (
        'Silver',
        'Silver award',
        '/icons/silver.svg'
    ),
    (
        'Gold',
        'Gold award',
        '/icons/gold.svg'
    ),
    (
        'Platinum',
        'Platinum award',
        '/icons/platinum.svg'
    );

-- Create indexes for performance optimization on large datasets
CREATE INDEX CONCURRENTLY idx_posts_community_hot_active ON posts (
    community_id,
    hot_score DESC,
    status
)
WHERE
    status = 'active';

CREATE INDEX CONCURRENTLY idx_posts_trending ON posts (created_at DESC, score DESC)
WHERE
    created_at > NOW() - INTERVAL '24 hours'
    AND status = 'active';

CREATE INDEX CONCURRENTLY idx_comments_post_tree ON comments (post_id, path, status)
WHERE
    status = 'active';

-- Partial indexes for better performance
CREATE INDEX CONCURRENTLY idx_notifications_unread_recent ON notifications (recipient_id, created_at DESC)
WHERE
    is_read = FALSE
    AND created_at > NOW() - INTERVAL '30 days';

CREATE INDEX CONCURRENTLY idx_typing_indicators_active ON comment_typing_indicators (post_id, parent_comment_id)
WHERE
    last_activity_at > NOW() - INTERVAL '30 seconds';