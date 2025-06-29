-- Add migration script here
-- Create upload type enum
CREATE TYPE upload_type AS ENUM (
    'avatar', 'banner', 'postimage', 'postvideo', 
    'commentimage', 'voicereply', 'videoreply'
);

-- Create media status enum
CREATE TYPE media_status AS ENUM (
    'uploading', 'processing', 'completed', 'failed', 'deleted'
);

-- Core media files table
CREATE TABLE media_files (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    original_name VARCHAR(255) NOT NULL,
    file_path VARCHAR(500) NOT NULL,
    cdn_url VARCHAR(500),
    file_type VARCHAR(50) NOT NULL, -- image, video, audio
    file_size BIGINT NOT NULL,
    mime_type VARCHAR(100) NOT NULL,
    width INTEGER,
    height INTEGER,
    duration INTEGER, -- in seconds for video/audio
    upload_type upload_type NOT NULL,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status media_status DEFAULT 'uploading',
    metadata_json JSONB,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    processed_at TIMESTAMPTZ
);

-- Media variants table (thumbnails, different sizes)
CREATE TABLE media_variants (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    media_file_id UUID NOT NULL REFERENCES media_files (id) ON DELETE CASCADE,
    variant_type VARCHAR(50) NOT NULL, -- thumbnail, small, medium, large
    file_path VARCHAR(500) NOT NULL,
    cdn_url VARCHAR(500),
    width INTEGER,
    height INTEGER,
    file_size BIGINT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Upload sessions table (for chunked uploads)
CREATE TABLE upload_sessions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    session_token VARCHAR(255) NOT NULL UNIQUE,
    original_filename VARCHAR(255) NOT NULL,
    total_size BIGINT NOT NULL,
    uploaded_size BIGINT DEFAULT 0,
    chunk_count INTEGER DEFAULT 0,
    upload_type upload_type NOT NULL,
    status VARCHAR(50) DEFAULT 'active',
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Update post_media table to reference media_files
ALTER TABLE post_media DROP COLUMN IF EXISTS media_url;

ALTER TABLE post_media DROP COLUMN IF EXISTS thumbnail_url;

ALTER TABLE post_media DROP COLUMN IF EXISTS media_type;

ALTER TABLE post_media DROP COLUMN IF EXISTS file_size;

ALTER TABLE post_media DROP COLUMN IF EXISTS width;

ALTER TABLE post_media DROP COLUMN IF EXISTS height;

ALTER TABLE post_media DROP COLUMN IF EXISTS duration;

-- Add reference to media_files
ALTER TABLE post_media
ADD COLUMN IF NOT EXISTS media_file_id UUID REFERENCES media_files (id) ON DELETE CASCADE;

-- Update comment_media table similarly
ALTER TABLE comment_media DROP COLUMN IF EXISTS media_url;

ALTER TABLE comment_media DROP COLUMN IF EXISTS thumbnail_url;

ALTER TABLE comment_media DROP COLUMN IF EXISTS media_type;

ALTER TABLE comment_media DROP COLUMN IF EXISTS file_size;

ALTER TABLE comment_media DROP COLUMN IF EXISTS width;

ALTER TABLE comment_media DROP COLUMN IF EXISTS height;

ALTER TABLE comment_media DROP COLUMN IF EXISTS duration;

-- Add reference to media_files
ALTER TABLE comment_media
ADD COLUMN IF NOT EXISTS media_file_id UUID REFERENCES media_files (id) ON DELETE CASCADE;

-- Indexes for performance
CREATE INDEX idx_media_files_user_id ON media_files (user_id);

CREATE INDEX idx_media_files_upload_type ON media_files (upload_type);

CREATE INDEX idx_media_files_status ON media_files (status);

CREATE INDEX idx_media_files_created_at ON media_files (created_at DESC);

CREATE INDEX idx_media_variants_media_file_id ON media_variants (media_file_id);

CREATE INDEX idx_media_variants_variant_type ON media_variants (variant_type);

CREATE INDEX idx_upload_sessions_user_id ON upload_sessions (user_id);

CREATE INDEX idx_upload_sessions_session_token ON upload_sessions (session_token);

CREATE INDEX idx_upload_sessions_expires_at ON upload_sessions (expires_at);

CREATE INDEX idx_upload_sessions_status ON upload_sessions (status);

CREATE INDEX idx_post_media_media_file_id ON post_media (media_file_id);

CREATE INDEX idx_comment_media_media_file_id ON comment_media (media_file_id);

-- Function to clean up expired upload sessions
CREATE OR REPLACE FUNCTION cleanup_expired_upload_sessions()
RETURNS void AS $$
BEGIN
    -- Delete expired sessions
    DELETE FROM upload_sessions WHERE expires_at < NOW();
    
    -- Clean up orphaned temporary files (this would be handled by the application)
    -- Update failed uploads
    UPDATE media_files 
    SET status = 'failed' 
    WHERE status = 'uploading' 
    AND created_at < NOW() - INTERVAL '1 hour';
END;
$$ LANGUAGE plpgsql;

-- Function to get media file with variants
-- Function to get media file with variants
CREATE OR REPLACE FUNCTION get_media_with_variants(media_file_id UUID)
RETURNS TABLE (
    id UUID,
    original_name VARCHAR,
    file_path VARCHAR,
    cdn_url VARCHAR,
    file_type VARCHAR,
    file_size BIGINT,
    mime_type VARCHAR,
    width INTEGER,
    height INTEGER,
    duration INTEGER,
    upload_type upload_type,
    user_id UUID,
    status media_status,
    metadata_json JSONB,
    created_at TIMESTAMPTZ,
    processed_at TIMESTAMPTZ,
    variants JSONB
) AS $$
BEGIN
    RETURN QUERY
    SELECT 
        mf.id,
        mf.original_name,
        mf.file_path,
        mf.cdn_url,
        mf.file_type,
        mf.file_size,
        mf.mime_type,
        mf.width,
        mf.height,
        mf.duration,
        mf.upload_type,
        mf.user_id,
        mf.status,
        mf.metadata_json,
        mf.created_at,
        mf.processed_at,
        COALESCE(
            json_agg(
                json_build_object(
                    'id', mv.id,
                    'variant_type', mv.variant_type,
                    'file_path', mv.file_path,
                    'cdn_url', mv.cdn_url,
                    'width', mv.width,
                    'height', mv.height,
                    'file_size', mv.file_size
                )
            ) FILTER (WHERE mv.id IS NOT NULL),
            '[]'::json
        )::jsonb as variants
    FROM media_files mf
    LEFT JOIN media_variants mv ON mf.id = mv.media_file_id
    WHERE mf.id = media_file_id
    GROUP BY mf.id, mf.original_name, mf.file_path, mf.cdn_url, mf.file_type, 
             mf.file_size, mf.mime_type, mf.width, mf.height, mf.duration,
             mf.upload_type, mf.user_id, mf.status, mf.metadata_json, 
             mf.created_at, mf.processed_at;
END;
$$ LANGUAGE plpgsql;