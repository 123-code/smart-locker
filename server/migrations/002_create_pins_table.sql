CREATE TABLE IF NOT EXISTS pins (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    locker_id VARCHAR(64) NOT NULL REFERENCES devices(locker_id),
    pin_hash VARCHAR(128) NOT NULL,
    salt VARCHAR(128) NOT NULL,
    used BOOLEAN NOT NULL DEFAULT false,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_pins_locker_active ON pins(locker_id, used, expires_at);
