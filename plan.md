# ESP32 + Rust Server Intercommunication Test Plan

## Prerequisites

- ESP32 connected via USB-C (shows up as `/dev/cu.wchusbserial1430`)
- PostgreSQL running locally
- PlatformIO installed (`pip3 install platformio`)
- Rust toolchain installed

## Architecture

```
[ESP32] --WiFi--> [Rust Server @ 0.0.0.0:3000] ---> [PostgreSQL: locker_pins]
```

- **ESP32** firmware: `/Users/joseignacionaranjo/esp-locker/` (PlatformIO project)
- **Rust server**: `/Users/joseignacionaranjo/rust-crud-server/` (Axum + sqlx)
- ESP32 authenticates with `Bearer test-device-key-123`
- Server hashes the key with SHA256 and looks it up in the `devices` table

## Step-by-step

### 1. Find the ESP32 serial port

```bash
ls /dev/cu.wchusbserial* /dev/cu.usbserial* 2>/dev/null
```

If the port differs from `/dev/cu.wchusbserial1430`, update `platformio.ini` upload_port or pass it via `--upload-port`.

### 2. Get local IP

```bash
ifconfig | grep "inet " | grep -v 127.0.0.1
```

If the IP differs from `192.169.58.134`, update `SERVER_URL` in `esp-locker/src/main.cpp`.

### 3. Flash the ESP32

```bash
cd /Users/joseignacionaranjo/esp-locker
python3 -m platformio run --target upload --upload-port /dev/cu.wchusbserial1430
```

### 4. Ensure PostgreSQL is running and database exists

```bash
pg_isready                   # should say "accepting connections"
createdb locker_pins 2>/dev/null   # no-op if already exists
```

### 5. Ensure the Rust server binds to 0.0.0.0

In `rust-crud-server/src/main.rs`, the listener MUST be:

```rust
let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
```

NOT `127.0.0.1:3000` — otherwise the ESP32 can't reach it over WiFi.

### 6. Build and start the Rust server

```bash
# Kill anything on port 3000 first
kill $(lsof -ti :3000) 2>/dev/null

cd /Users/joseignacionaranjo/rust-crud-server
cargo build --release
./target/release/rust-crud-server &
```

Wait for `Server running at http://127.0.0.1:3000` output. Migrations run automatically on startup.

### 7. Register the ESP32 device (once)

The API key `test-device-key-123` has SHA256 hash:
`fdf67064313324311695901cfba3b4acf7fd1097105b9436cdd7316b0673a4d3`

```bash
psql -d locker_pins -c "
INSERT INTO devices (locker_id, api_key_hash, name, active)
VALUES ('locker-001', 'fdf67064313324311695901cfba3b4acf7fd1097105b9436cdd7316b0673a4d3', 'Test ESP32 Locker', true)
ON CONFLICT (locker_id) DO NOTHING;
"
```

### 8. Generate a PIN

```bash
curl -s -X POST http://localhost:3000/pins \
  -H "Content-Type: application/json" \
  -d '{"locker_id": "locker-001"}' | python3 -m json.tool
```

Note the `pin` value from the response (e.g. `427786`).

### 9. Send the PIN to ESP32 via serial

```python
python3 -c "
import serial, time
ser = serial.Serial('/dev/cu.wchusbserial1430', 115200, timeout=5)
time.sleep(2)
while ser.in_waiting:
    print(ser.readline().decode('utf-8', errors='replace').strip())
ser.write(b'XXXXXX\n')  # replace XXXXXX with the PIN from step 8
print('--- Sent PIN ---')
time.sleep(5)
while ser.in_waiting:
    print(ser.readline().decode('utf-8', errors='replace').strip())
ser.close()
"
```

### 10. Verify success

Expected output:

```
Verifying PIN: 427786
POST http://192.169.58.134:3000/pins/verify
Body: {"pin":"427786"}
HTTP 200
Response: {"action":"open"}
```

`"action":"open"` = success. The ESP32 authenticated, the PIN was verified, and the locker would open.

## Troubleshooting

| Problem | Fix |
|---|---|
| `No USB serial devices found` | Check USB cable / driver (CH340) |
| ESP32 prints `WiFi...` forever | Check SSID/password in `main.cpp` |
| `HTTP request failed` | Server not running, wrong IP, or firewall |
| `{"error":"Invalid API key"}` | Device not registered in DB or wrong key |
| `{"action":"deny","reason":"no_active_pin"}` | PIN expired or already used — generate a new one |
| `{"action":"deny","reason":"too_many_attempts"}` | Rate limited — wait 15 minutes or clear `rate_limiter` |
| Port 3000 in use | `kill $(lsof -ti :3000)` before starting server |
