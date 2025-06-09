-- Post types and status
CREATE TYPE post_type AS ENUM ('text', 'link', 'image', 'video');

CREATE TYPE post_status AS ENUM ('active', 'removed', 'deleted', 'spam');

-- Posts table
CREATE TABLE posts (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    title VARCHAR(300) NOT NULL,
    content TEXT,
    url VARCHAR(2000),
    post_type post_type NOT NULL,
    status post_status DEFAULT 'active',
    is_nsfw BOOLEAN DEFAULT FALSE,
    is_spoiler BOOLEAN DEFAULT FALSE,
    is_locked BOOLEAN DEFAULT FALSE,
    is_pinned BOOLEAN DEFAULT FALSE,
    author_id UUID NOT NULL REFERENCES users(id),
    community_id UUID NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
    upvotes INTEGER DEFAULT 0,
    downvotes INTEGER DEFAULT 0,
    score INTEGER DEFAULT 0, -- upvotes - downvotes
    comment_count INTEGER DEFAULT 0,
    view_count INTEGER DEFAULT 0,
    share_count INTEGER DEFAULT 0,
    hot_score DECIMAL(10,4) DEFAULT 0, -- For hot sorting algorithm
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),

-- Constraints based on post type
CONSTRAINT posts_content_check CHECK (
        (post_type = 'text' AND content IS NOT NULL) OR
        (post_type = 'link' AND url IS NOT NULL) OR
        (post_type IN ('image', 'video'))
    )
);

-- Post media (for image/video posts)
CREATE TABLE post_media (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    post_id UUID NOT NULL REFERENCES posts (id) ON DELETE CASCADE,
    media_url VARCHAR(500) NOT NULL,
    thumbnail_url VARCHAR(500),
    media_type VARCHAR(50) NOT NULL, -- image/jpeg, video/mp4, etc.
    file_size BIGINT,
    width INTEGER,
    height INTEGER,
    duration INTEGER, -- for videos/audio in seconds
    media_order INTEGER DEFAULT 1,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Post votes
CREATE TABLE post_votes (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    post_id UUID NOT NULL REFERENCES posts (id) ON DELETE CASCADE,
    vote_type SMALLINT NOT NULL CHECK (vote_type IN (-1, 1)), -- -1 downvote, 1 upvote
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE (user_id, post_id)
);

-- Post views (for analytics)
CREATE TABLE post_views (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    post_id UUID NOT NULL REFERENCES posts (id) ON DELETE CASCADE,
    user_id UUID REFERENCES users (id) ON DELETE SET NULL, -- nullable for anonymous views
    ip_address INET,
    user_agent TEXT,
    viewed_at TIMESTAMPTZ DEFAULT NOW()
);

-- Post reports
CREATE TABLE post_reports (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    post_id UUID NOT NULL REFERENCES posts (id) ON DELETE CASCADE,
    reported_by UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    reason VARCHAR(100) NOT NULL,
    description TEXT,
    status VARCHAR(20) DEFAULT 'pending',
    reviewed_by UUID REFERENCES users (id),
    reviewed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE (post_id, reported_by)
);

-- Indexes for posts (optimized for Reddit-like queries)
CREATE INDEX idx_posts_community_id ON posts (community_id);

CREATE INDEX idx_posts_author_id ON posts (author_id);

CREATE INDEX idx_posts_status ON posts (status);

CREATE INDEX idx_posts_created_at ON posts (created_at DESC);

CREATE INDEX idx_posts_score ON posts (score DESC);

CREATE INDEX idx_posts_hot_score ON posts (hot_score DESC);

CREATE INDEX idx_posts_comment_count ON posts (comment_count DESC);

-- Composite indexes for feed queries
CREATE INDEX idx_posts_community_status_created ON posts (
    community_id,
    status,
    created_at DESC
);

CREATE INDEX idx_posts_community_status_score ON posts (
    community_id,
    status,
    score DESC
);

CREATE INDEX idx_posts_community_status_hot ON posts (
    community_id,
    status,
    hot_score DESC
);

CREATE INDEX idx_posts_status_created ON posts (status, created_at DESC)
WHERE
    status = 'active';

CREATE INDEX idx_posts_status_score ON posts (status, score DESC)
WHERE
    status = 'active';

CREATE INDEX idx_posts_status_hot ON posts (status, hot_score DESC)
WHERE
    status = 'active';

-- Full-text search index
CREATE INDEX idx_posts_search ON posts USING GIN (
    to_tsvector (
        'english',
        title || ' ' || COALESCE(content, '')
    )
);

-- Indexes for post media
CREATE INDEX idx_post_media_post_id ON post_media (post_id);

CREATE INDEX idx_post_media_order ON post_media (post_id, media_order);

-- Indexes for post votes
CREATE INDEX idx_post_votes_post_id ON post_votes (post_id);

CREATE INDEX idx_post_votes_user_id ON post_votes (user_id);

CREATE INDEX idx_post_votes_created_at ON post_votes (created_at);

-- Indexes for post views
CREATE INDEX idx_post_views_post_id ON post_views (post_id);

CREATE INDEX idx_post_views_user_id ON post_views (user_id)
WHERE
    user_id IS NOT NULL;

CREATE INDEX idx_post_views_viewed_at ON post_views (viewed_at);

-- Indexes for post reports
CREATE INDEX idx_post_reports_post_id ON post_reports (post_id);

CREATE INDEX idx_post_reports_status ON post_reports (status);

-- Triggers
CREATE TRIGGER update_posts_updated_at BEFORE UPDATE ON