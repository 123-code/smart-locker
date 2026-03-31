CREATE TABLE IF NOT EXISTS devices (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    locker_id VARCHAR(64) NOT NULL UNIQUE,
    api_key_hash VARCHAR(128) NOT NULL,
    name VARCHAR(255),
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_devices_api_key_hash ON devices(api_key_hash);
