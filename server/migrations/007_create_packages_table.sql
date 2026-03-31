CREATE TABLE IF NOT EXISTS packages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    sender_id UUID REFERENCES users(id),
    deliverer_id UUID REFERENCES users(id),
    recipient_id UUID REFERENCES users(id),
    locker_id VARCHAR(64) NOT NULL REFERENCES devices(locker_id),
    status VARCHAR(32) NOT NULL DEFAULT 'created',
    label VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_packages_sender ON packages(sender_id);
CREATE INDEX idx_packages_deliverer ON packages(deliverer_id);
CREATE INDEX idx_packages_recipient ON packages(recipient_id);
CREATE INDEX idx_packages_locker ON packages(locker_id);
CREATE INDEX idx_packages_status ON packages(status);
