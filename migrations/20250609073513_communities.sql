-- Add migration script here
-- Community types and status
CREATE TYPE community_type AS ENUM ('public', 'restricted', 'private');

CREATE TYPE community_status AS ENUM ('active', 'quarantined', 'banned');

CREATE TYPE membership_role AS ENUM ('member', 'moderator', 'admin', 'owner');

-- Communities table
CREATE TABLE communities (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    name VARCHAR(50) UNIQUE NOT NULL,
    display_name VARCHAR(100) NOT NULL,
    description TEXT,
    rules TEXT,
    icon_url VARCHAR(500),
    banner_url VARCHAR(500),
    community_type community_type DEFAULT 'public',
    status community_status DEFAULT 'active',
    is_nsfw BOOLEAN DEFAULT FALSE,
    subscriber_count INTEGER DEFAULT 0,
    post_count INTEGER DEFAULT 0,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Community memberships
CREATE TABLE community_memberships (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    community_id UUID NOT NULL REFERENCES communities (id) ON DELETE CASCADE,
    role membership_role DEFAULT 'member',
    joined_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE (user_id, community_id)
);

-- Community rules (separate table for better management)
CREATE TABLE community_rules (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    community_id UUID NOT NULL REFERENCES communities (id) ON DELETE CASCADE,
    title VARCHAR(200) NOT NULL,
    description TEXT,
    rule_order INTEGER NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Community bans
CREATE TABLE community_bans (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    community_id UUID NOT NULL REFERENCES communities (id) ON DELETE CASCADE,
    banned_by UUID NOT NULL REFERENCES users (id),
    reason TEXT,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE (user_id, community_id)
);

-- Indexes for communities
CREATE INDEX idx_communities_name ON communities (name);

CREATE INDEX idx_communities_type ON communities (community_type);

CREATE INDEX idx_communities_status ON communities (status);

CREATE INDEX idx_communities_nsfw ON communities (is_nsfw);

CREATE INDEX idx_communities_subscriber_count ON communities (subscriber_count DESC);

CREATE INDEX idx_communities_created_at ON communities (created_at);

CREATE INDEX idx_communities_created_by ON communities (created_by);

-- Full-text search index for communities
CREATE INDEX idx_communities_search ON communities USING GIN (
    to_tsvector (
        'english',
        display_name || ' ' || COALESCE(description, '')
    )
);

-- Indexes for community memberships
CREATE INDEX idx_community_memberships_user_id ON community_memberships (user_id);

CREATE INDEX idx_community_memberships_community_id ON community_memberships (community_id);

CREATE INDEX idx_community_memberships_role ON community_memberships (role);

CREATE INDEX idx_community_memberships_joined_at ON community_memberships (joined_at);

-- Indexes for community rules
CREATE INDEX idx_community_rules_community_id ON community_rules (community_id);

CREATE INDEX idx_community_rules_order ON community_rules (community_id, rule_order);

-- Indexes for community bans
CREATE INDEX idx_community_bans_user_id ON community_bans (user_id);

CREATE INDEX idx_community_bans_community_id ON community_bans (community_id);

CREATE INDEX idx_community_bans_expires_at ON community_bans (expires_at)
WHERE
    expires_at IS NOT NULL;

-- Triggers
CREATE TRIGGER update_communities_updated_at BEFORE UPDATE ON communities
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Function to update community subscriber count
CREATE OR REPLACE FUNCTION update_community_subscriber_count()
RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        UPDATE communities 
        SET subscriber_count = subscriber_count + 1 
        WHERE id = NEW.community_id;
        RETURN NEW;
    ELSIF TG_OP = 'DELETE' THEN
        UPDATE communities 
        SET subscriber_count = subscriber_count - 1 
        WHERE id = OLD.community_id;
        RETURN OLD;
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- Trigger for subscriber count
CREATE TRIGGER update_subscriber_count_trigger
    AFTER INSERT OR DELETE ON community_memberships
    FOR EACH ROW EXECUTE FUNCTION update_community_subscriber_count();