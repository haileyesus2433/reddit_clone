-- Search history table
CREATE TABLE search_history (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    query VARCHAR(200) NOT NULL,
    search_type VARCHAR(20) NOT NULL DEFAULT 'all',
    results_count INTEGER DEFAULT 0,
    clicked_result_id UUID,
    clicked_result_type VARCHAR(20), -- 'post', 'comment', 'community', 'user'
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Add search vectors to existing tables for better full-text search
ALTER TABLE posts ADD COLUMN search_vector tsvector;

ALTER TABLE communities ADD COLUMN search_vector tsvector;

ALTER TABLE users ADD COLUMN search_vector tsvector;

-- Create indexes for search vectors
CREATE INDEX idx_posts_search_vector ON posts USING GIN (search_vector);

CREATE INDEX idx_communities_search_vector ON communities USING GIN (search_vector);

CREATE INDEX idx_users_search_vector ON users USING GIN (search_vector);

-- Create indexes for search history
CREATE INDEX idx_search_history_user_id ON search_history (user_id);

CREATE INDEX idx_search_history_query ON search_history (query);

CREATE INDEX idx_search_history_created_at ON search_history (created_at DESC);

-- Function to update search vectors
CREATE OR REPLACE FUNCTION update_post_search_vector()
RETURNS TRIGGER AS $$
BEGIN
    NEW.search_vector := to_tsvector('english', 
        NEW.title || ' ' || COALESCE(NEW.content, '')
    );
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION update_community_search_vector()
RETURNS TRIGGER AS $$
BEGIN
    NEW.search_vector := to_tsvector('english', 
        NEW.name || ' ' || NEW.display_name || ' ' || COALESCE(NEW.description, '')
    );
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION update_user_search_vector()
RETURNS TRIGGER AS $$
BEGIN
    NEW.search_vector := to_tsvector('english', 
        NEW.username || ' ' || COALESCE(NEW.display_name, '') || ' ' || COALESCE(NEW.bio, '')
    );
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create triggers to automatically update search vectors
CREATE TRIGGER update_post_search_vector_trigger
    BEFORE INSERT OR UPDATE ON posts
    FOR EACH ROW EXECUTE FUNCTION update_post_search_vector();

CREATE TRIGGER update_community_search_vector_trigger
    BEFORE INSERT OR UPDATE ON communities
    FOR EACH ROW EXECUTE FUNCTION update_community_search_vector();

CREATE TRIGGER update_user_search_vector_trigger
    BEFORE INSERT OR UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION update_user_search_vector();

-- Update existing records with search vectors
UPDATE posts
SET
    search_vector = to_tsvector (
        'english',
        title || ' ' || COALESCE(content, '')
    );

UPDATE communities
SET
    search_vector = to_tsvector (
        'english',
        name || ' ' || display_name || ' ' || COALESCE(description, '')
    );

UPDATE users
SET
    search_vector = to_tsvector (
        'english',
        username || ' ' || COALESCE(display_name, '') || ' ' || COALESCE(bio, '')
    );

-- Create materialized view for trending content (optional, for performance)
CREATE MATERIALIZED VIEW trending_posts AS
SELECT
    p.id,
    p.title,
    p.score,
    p.comment_count,
    p.created_at,
    p.community_id,
    p.author_id,
    -- Simple trending score: (upvotes - downvotes) / age_in_hours^1.5
    CASE
        WHEN EXTRACT(
            EPOCH
            FROM (NOW() - p.created_at)
        ) / 3600 > 0 THEN p.score / POWER(
            EXTRACT(
                EPOCH
                FROM (NOW() - p.created_at)
            ) / 3600,
            1.5
        )
        ELSE p.score
    END as trending_score
FROM posts p
WHERE
    p.status = 'active'
    AND p.created_at > NOW() - INTERVAL '7 days';

-- Create index on trending score
CREATE INDEX idx_trending_posts_score ON trending_posts (trending_score DESC);

-- Create function to refresh trending posts
CREATE OR REPLACE FUNCTION refresh_trending_posts()
RETURNS void AS $$
BEGIN
    REFRESH MATERIALIZED VIEW CONCURRENTLY trending_posts;
END;
$$ LANGUAGE plpgsql;

-- Create popular searches materialized view
CREATE MATERIALIZED VIEW popular_searches AS
SELECT
    query,
    COUNT(*) as search_count,
    MAX(created_at) as last_searched
FROM search_history
WHERE
    created_at > NOW() - INTERVAL '7 days'
GROUP BY
    query
HAVING
    COUNT(*) >= 3
ORDER BY search_count DESC, last_searched DESC;

CREATE INDEX idx_popular_searches_count ON popular_searches (search_count DESC);

CREATE INDEX idx_popular_searches_query ON popular_searches USING GIN (
    to_tsvector ('english', query)
);