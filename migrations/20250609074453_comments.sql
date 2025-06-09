-- Comment status
CREATE TYPE comment_status AS ENUM ('active', 'removed', 'deleted', 'spam');

-- Comments table (with nested structure support)
CREATE TABLE comments (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    content TEXT NOT NULL,
    post_id UUID NOT NULL REFERENCES posts (id) ON DELETE CASCADE,
    author_id UUID NOT NULL REFERENCES users (id),
    parent_comment_id UUID REFERENCES comments (id) ON DELETE CASCADE,
    status comment_status DEFAULT 'active',
    is_edited BOOLEAN DEFAULT FALSE,
    upvotes INTEGER DEFAULT 0,
    downvotes INTEGER DEFAULT 0,
    score INTEGER DEFAULT 0,
    reply_count INTEGER DEFAULT 0,
    depth INTEGER DEFAULT 0, -- For nested comment depth
    path TEXT, -- Materialized path for efficient tree queries (e.g., '1.2.3')
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    edited_at TIMESTAMPTZ
);

-- Comment media (for images, videos, audio in comments)
CREATE TABLE comment_media (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    comment_id UUID NOT NULL REFERENCES comments (id) ON DELETE CASCADE,
    media_url VARCHAR(500) NOT NULL,
    thumbnail_url VARCHAR(500),
    media_type VARCHAR(50) NOT NULL, -- image/jpeg, video/mp4, audio/mp3, etc.
    file_size BIGINT,
    width INTEGER,
    height INTEGER,
    duration INTEGER, -- for videos/audio in seconds
    media_order INTEGER DEFAULT 1,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Comment votes
CREATE TABLE comment_votes (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    comment_id UUID NOT NULL REFERENCES comments (id) ON DELETE CASCADE,
    vote_type SMALLINT NOT NULL CHECK (vote_type IN (-1, 1)),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE (user_id, comment_id)
);

-- Comment reports
CREATE TABLE comment_reports (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    comment_id UUID NOT NULL REFERENCES comments (id) ON DELETE CASCADE,
    reported_by UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    reason VARCHAR(100) NOT NULL,
    description TEXT,
    status VARCHAR(20) DEFAULT 'pending',
    reviewed_by UUID REFERENCES users (id),
    reviewed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE (comment_id, reported_by)
);

-- Real-time typing indicators
CREATE TABLE comment_typing_indicators (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    post_id UUID NOT NULL REFERENCES posts (id) ON DELETE CASCADE,
    parent_comment_id UUID REFERENCES comments (id) ON DELETE CASCADE,
    started_typing_at TIMESTAMPTZ DEFAULT NOW(),
    last_activity_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE (
        user_id,
        post_id,
        parent_comment_id
    )
);

-- Indexes for comments (optimized for nested queries)
CREATE INDEX idx_comments_post_id ON comments (post_id);

CREATE INDEX idx_comments_author_id ON comments (author_id);

CREATE INDEX idx_comments_parent_id ON comments (parent_comment_id)
WHERE
    parent_comment_id IS NOT NULL;

CREATE INDEX idx_comments_status ON comments (status);

CREATE INDEX idx_comments_created_at ON comments (created_at DESC);

CREATE INDEX idx_comments_score ON comments (score DESC);

CREATE INDEX idx_comments_path ON comments (path);

-- Composite indexes for sorting
CREATE INDEX idx_comments_post_status_created ON comments (
    post_id,
    status,
    created_at DESC
);

CREATE INDEX idx_comments_post_status_score ON comments (post_id, status, score DESC);

CREATE INDEX idx_comments_post_parent_created ON comments (
    post_id,
    parent_comment_id,
    created_at ASC
);

-- Full-text search index for comments
CREATE INDEX idx_comments_search ON comments USING GIN (
    to_tsvector ('english', content)
);

-- Indexes for comment media
CREATE INDEX idx_comment_media_comment_id ON comment_media (comment_id);

CREATE INDEX idx_comment_media_order ON comment_media (comment_id, media_order);

-- Indexes for comment votes
CREATE INDEX idx_comment_votes_comment_id ON comment_votes (comment_id);

CREATE INDEX idx_comment_votes_user_id ON comment_votes (user_id);

-- Indexes for typing indicators
CREATE INDEX idx_typing_indicators_post_id ON comment_typing_indicators (post_id);

CREATE INDEX idx_typing_indicators_last_activity ON comment_typing_indicators (last_activity_at);

-- Triggers
CREATE TRIGGER update_comments_updated_at BEFORE UPDATE ON comments
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_comment_votes_updated_at BEFORE UPDATE ON comment_votes
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Function to update comment vote counts
CREATE OR REPLACE FUNCTION update_comment_vote_counts()
RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.vote_type = 1 THEN
            UPDATE comments SET upvotes = upvotes + 1, score = score + 1 WHERE id = NEW.comment_id;
        ELSE
            UPDATE comments SET downvotes = downvotes + 1, score = score - 1 WHERE id = NEW.comment_id;
        END IF;
        RETURN NEW;
    ELSIF TG_OP = 'UPDATE' THEN
        IF OLD.vote_type = 1 AND NEW.vote_type = -1 THEN
            UPDATE comments SET upvotes = upvotes - 1, downvotes = downvotes + 1, score = score - 2 WHERE id = NEW.comment_id;
        ELSIF OLD.vote_type = -1 AND NEW.vote_type = 1 THEN
            UPDATE comments SET upvotes = upvotes + 1, downvotes = downvotes - 1, score = score + 2 WHERE id = NEW.comment_id;
        END IF;
        RETURN NEW;
    ELSIF TG_OP = 'DELETE' THEN
        IF OLD.vote_type = 1 THEN
            UPDATE comments SET upvotes = upvotes - 1, score = score - 1 WHERE id = OLD.comment_id;
        ELSE
            UPDATE comments SET downvotes = downvotes - 1, score = score + 1 WHERE id = OLD.comment_id;
        END IF;
        RETURN OLD;
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- Trigger for comment vote counts
CREATE TRIGGER update_comment_vote_counts_trigger
    AFTER INSERT OR UPDATE OR DELETE ON comment_votes
    FOR EACH ROW EXECUTE FUNCTION update_comment_vote_counts();

-- Function to update post comment count
CREATE OR REPLACE FUNCTION update_post_comment_count()
RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        UPDATE posts SET comment_count = comment_count + 1 WHERE id = NEW.post_id;
        RETURN NEW;
    ELSIF TG_OP = 'DELETE' THEN
        UPDATE posts SET comment_count = comment_count - 1 WHERE id = OLD.post_id;
        RETURN OLD;
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- Trigger for post comment count
CREATE TRIGGER update_post_comment_count_trigger
    AFTER INSERT OR DELETE ON comments
    FOR EACH ROW EXECUTE FUNCTION update_post_comment_count();

-- Function to update parent comment reply count
CREATE OR REPLACE FUNCTION update_parent_comment_reply_count()
RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'INSERT' AND NEW.parent_comment_id IS NOT NULL THEN
        UPDATE comments SET reply_count = reply_count + 1 WHERE id = NEW.parent_comment_id;
        RETURN NEW;
    ELSIF TG_OP = 'DELETE' AND OLD.parent_comment_id IS NOT NULL THEN
        UPDATE comments SET reply_count = reply_count - 1 WHERE id = OLD.parent_comment_id;
        RETURN OLD;
    END IF;
    RETURN COALESCE(NEW, OLD);
END;
$$ LANGUAGE plpgsql;

-- Trigger for parent comment reply count
CREATE TRIGGER update_parent_comment_reply_count_trigger
    AFTER INSERT OR DELETE ON comments
    FOR EACH ROW EXECUTE FUNCTION update_parent_comment_reply_count();

-- Function to set comment depth and path
CREATE OR REPLACE FUNCTION set_comment_depth_and_path()
RETURNS TRIGGER AS $$
DECLARE
    parent_depth INTEGER;
    parent_path TEXT;
    new_path TEXT;
BEGIN
    IF NEW.parent_comment_id IS NULL THEN
        -- Top-level comment
        NEW.depth := 0;
        NEW.path := NEW.id::TEXT;
    ELSE
        -- Reply to another comment
        SELECT depth, path INTO parent_depth, parent_path
        FROM comments WHERE id = NEW.parent_comment_id;
        
        NEW.depth := parent_depth + 1;
        NEW.path := parent_path || '.' || NEW.id::TEXT;
    END IF;
    
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to set depth and path before insert
CREATE TRIGGER set_comment_depth_and_path_trigger
    BEFORE INSERT ON comments
    FOR EACH ROW EXECUTE FUNCTION set_comment_depth_and_path();

-- Function to clean up old typing indicators
CREATE OR REPLACE FUNCTION cleanup_old_typing_indicators()
RETURNS void AS $$
BEGIN
    DELETE FROM comment_typing_indicators 
    WHERE last_activity_at < NOW() - INTERVAL '30 seconds';
END;
$$ LANGUAGE plpgsql;