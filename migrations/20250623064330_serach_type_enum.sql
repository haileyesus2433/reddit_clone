CREATE TYPE search_type AS ENUM ('all', 'posts', 'comments', 'communities', 'users');

-- Drop the default before altering the type
ALTER TABLE search_history ALTER COLUMN search_type DROP DEFAULT;

-- Clean up any values with extra quotes
UPDATE search_history
SET
    search_type = TRIM(
        BOTH '"'
        FROM search_type
    );

-- Alter the column type
ALTER TABLE search_history
    ALTER COLUMN search_type TYPE search_type
    USING search_type::search_type;

-- Re-add the default as the enum value
ALTER TABLE search_history
ALTER COLUMN search_type
SET DEFAULT 'all';