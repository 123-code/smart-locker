-- QR verification sessions for locker verification
-- Each session represents a pickup attempt that needs QR + PIN verification
CREATE TABLE IF NOT EXISTS qr_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    locker_id VARCHAR(64) NOT NULL REFERENCES devices(locker_id),
    session_code VARCHAR(64) NOT NULL UNIQUE,  -- The QR code content (hashed)
    pin_id UUID REFERENCES pins(id),            -- Associated PIN if known
    used BOOLEAN NOT NULL DEFAULT false,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    used_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_qr_sessions_code ON qr_sessions(session_code);
CREATE INDEX IF NOT EXISTS idx_qr_sessions_locker_active ON qr_sessions(locker_id, used, expires_at);
