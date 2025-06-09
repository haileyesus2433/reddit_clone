-- Enable necessary extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE EXTENSION IF NOT EXISTS "pg_trgm";

CREATE EXTENSION IF NOT EXISTS "btree_gin";

-- Create custom types
CREATE TYPE auth_provider AS ENUM ('email', 'phone', 'google', 'apple');

CREATE TYPE user_status AS ENUM ('active', 'suspended', 'deleted');

-- Users table
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    username VARCHAR(50) UNIQUE NOT NULL,
    email VARCHAR(255) UNIQUE,
    phone VARCHAR(20) UNIQUE,
    password_hash VARCHAR(255),
    display_name VARCHAR(100),
    bio TEXT,
    avatar_url VARCHAR(500),
    banner_url VARCHAR(500),
    karma_points INTEGER DEFAULT 0,
    is_verified BOOLEAN DEFAULT FALSE,
    status user_status DEFAULT 'active',
    auth_provider auth_provider NOT NULL,
    oauth_id VARCHAR(255),
    email_verified BOOLEAN DEFAULT FALSE,
    phone_verified BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    last_login_at TIMESTAMPTZ,

-- Constraints
CONSTRAINT users_auth_check CHECK (
        (auth_provider = 'email' AND email IS NOT NULL AND password_hash IS NOT NULL) OR
        (auth_provider = 'phone' AND phone IS NOT NULL AND password_hash IS NOT NULL) OR
        (auth_provider IN ('google', 'apple') AND oauth_id IS NOT NULL)
    )
);

-- Password reset tokens
CREATE TABLE password_reset_tokens (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token VARCHAR(255) NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    used BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Phone verification codes
CREATE TABLE phone_verification_codes (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    phone VARCHAR(20) NOT NULL,
    code VARCHAR(10) NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    verified BOOLEAN DEFAULT FALSE,
    attempts INTEGER DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- User sessions for JWT blacklisting
CREATE TABLE user_sessions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4 (),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_jti VARCHAR(255) NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Indexes for users table
CREATE INDEX idx_users_email ON users (email)
WHERE
    email IS NOT NULL;

CREATE INDEX idx_users_phone ON users (phone)
WHERE
    phone IS NOT NULL;

CREATE INDEX idx_users_username ON users (username);

CREATE INDEX idx_users_oauth ON users (auth_provider, oauth_id)
WHERE
    oauth_id IS NOT NULL;

CREATE INDEX idx_users_status ON users (status);

CREATE INDEX idx_users_created_at ON users (created_at);

-- Indexes for related tables
CREATE INDEX idx_password_reset_tokens_user_id ON password_reset_tokens (user_id);

CREATE INDEX idx_password_reset_tokens_token ON password_reset_tokens (token);

CREATE INDEX idx_password_reset_tokens_expires_at ON password_reset_tokens (expires_at);

CREATE INDEX idx_phone_verification_phone ON phone_verification_codes (phone);

CREATE INDEX idx_phone_verification_expires_at ON phone_verification_codes (expires_at);

CREATE INDEX idx_user_sessions_user_id ON user_sessions (user_id);

CREATE INDEX idx_user_sessions_expires_at ON user_sessions (expires_at);

-- Function to update updated_at timestamp
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Trigger for users table
CREATE TRIGGER update_users_updated_at BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();