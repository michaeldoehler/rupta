-- The icon of an organization: the favicon of its website, fetched once
-- when the domain is set. `icon` holds the image (at most 256 KiB),
-- `icon_type` its media type; `icon_updated_at` changes with every new
-- icon so that clients can cache it under a versioned URL.
ALTER TABLE organizations ADD COLUMN domain text;
ALTER TABLE organizations ADD COLUMN icon bytea;
ALTER TABLE organizations ADD COLUMN icon_type text;
ALTER TABLE organizations ADD COLUMN icon_updated_at timestamptz;
