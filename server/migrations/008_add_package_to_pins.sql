ALTER TABLE pins ADD COLUMN IF NOT EXISTS package_id UUID REFERENCES packages(id);
CREATE INDEX IF NOT EXISTS idx_pins_package ON pins(package_id);
