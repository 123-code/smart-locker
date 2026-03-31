#include <WiFi.h>
#include <HTTPClient.h>

constexpr int RELAY_PIN = 26;
constexpr unsigned long RELAY_OPEN_MS = 4000;

const char* WIFI_SSID = "FANARMA";
const char* WIFI_PASS = "SucoMAX2021";

const char* SERVER_URL = "http://192.168.1.153:3000/pins/verify";
const char* QR_VERIFY_URL = "http://192.168.1.153:3000/qr/verify";
const char* POLL_URL = "http://192.168.1.153:3000/commands/poll";

const char* API_KEY = "test-device-key-123";

unsigned long lastPoll = 0;
constexpr unsigned long POLL_INTERVAL_MS = 3000;

// Simple QR code buffer - stores scanned QR code until PIN is entered
String pendingQrCode = "";
unsigned long qrTimestamp = 0;
constexpr unsigned long QR_TIMEOUT_MS = 60000; // 60 seconds to enter PIN after QR scan

void setRelay(bool isOpen) {
  if (isOpen) {
    pinMode(RELAY_PIN, OUTPUT);
    digitalWrite(RELAY_PIN, LOW);
  } else {
    pinMode(RELAY_PIN, INPUT);
  }
  Serial.printf("Lock %s\n", isOpen ? "OPEN" : "CLOSED");
}

void pulseRelay() {
  setRelay(true);
  delay(RELAY_OPEN_MS);
  setRelay(false);
}

bool shouldOpenLock(const String& response) {
  return response.indexOf("\"action\":\"allow\"") >= 0 ||
         response.indexOf("\"action\":\"open\"") >= 0 ||
         response.indexOf("\"allowed\":true") >= 0;
}

void testVerify(const char* pin) {
  if (WiFi.status() != WL_CONNECTED) {
    Serial.println("ERROR: WiFi no conectado");
    return;
  }

  HTTPClient http;
  http.begin(SERVER_URL);
  http.addHeader("Content-Type", "application/json");
  String authHeader = "Bearer ";
  authHeader += API_KEY;
  http.addHeader("Authorization", authHeader);

  String body = "{\"pin\":\"";
  body += pin;
  body += "\"}";

  Serial.printf("POST %s\n", SERVER_URL);
  Serial.printf("PIN %s\n", pin);

  const int httpCode = http.POST(body);
  if (httpCode > 0) {
    const String response = http.getString();
    Serial.printf("HTTP %d\n", httpCode);
    Serial.printf("Response: %s\n", response.c_str());

    if (httpCode == 200 && shouldOpenLock(response)) {
      pulseRelay();
    } else {
      Serial.println("Lock stays closed");
    }
  } else {
    Serial.printf("HTTP request failed: %s\n", http.errorToString(httpCode).c_str());
  }
  http.end();
}

// Verify QR + PIN combo
void verifyQrAndPin(const char* qrCode, const char* pin) {
  if (WiFi.status() != WL_CONNECTED) {
    Serial.println("ERROR: WiFi no conectado");
    return;
  }

  HTTPClient http;
  http.begin(QR_VERIFY_URL);
  http.addHeader("Content-Type", "application/json");
  String authHeader = "Bearer ";
  authHeader += API_KEY;
  http.addHeader("Authorization", authHeader);

  // Build JSON body with QR and PIN
  String body = "{\"qr_code\":\"";
  body += qrCode;
  body += "\",\"pin\":\"";
  body += pin;
  body += "\"}";

  Serial.printf("POST %s\n", QR_VERIFY_URL);
  Serial.printf("QR code: %s\n", qrCode);

  const int httpCode = http.POST(body);
  if (httpCode > 0) {
    const String response = http.getString();
    Serial.printf("HTTP %d\n", httpCode);
    Serial.printf("Response: %s\n", response.c_str());

    if (httpCode == 200 && shouldOpenLock(response)) {
      Serial.println("QR + PIN verified! Opening locker...");
      pulseRelay();
      // Clear pending QR
      pendingQrCode = "";
      qrTimestamp = 0;
    } else {
      Serial.println("Verification failed - Lock stays closed");
    }
  } else {
    Serial.printf("HTTP request failed: %s\n", http.errorToString(httpCode).c_str());
  }
  http.end();
}

void pollCommand() {
  if (WiFi.status() != WL_CONNECTED) return;

  HTTPClient http;
  http.begin(POLL_URL);
  String authHeader = "Bearer ";
  authHeader += API_KEY;
  http.addHeader("Authorization", authHeader);

  int httpCode = http.GET();
  if (httpCode == 200) {
    String response = http.getString();
    if (response.indexOf("\"command\":\"open\"") >= 0) {
      Serial.println("Remote command: OPEN");
      pulseRelay();
    }
  }
  http.end();
}

void setup() {
  Serial.begin(115200);
  delay(1000);

  setRelay(false);
  Serial.println("\n=== ESP32 Locker with QR Verification ===");
  Serial.println("Commands:");
  Serial.println("  open           - Open lock directly");
  Serial.println("  close          - Close lock");
  Serial.println("  pulse          - Pulse lock open");
  Serial.println("  qr:<code>      - Scan QR code (e.g., qr:simon:abc123)");
  Serial.println("  pin:<6-digits> - Enter PIN after QR scan");
  Serial.println("  <6-digits>     - Verify PIN only (legacy mode)");

  Serial.print("Connecting to WiFi");
  WiFi.begin(WIFI_SSID, WIFI_PASS);
  while (WiFi.status() != WL_CONNECTED) {
    delay(500);
    Serial.print(".");
  }
  Serial.printf("\nConnected! IP: %s\n", WiFi.localIP().toString().c_str());
  Serial.println("Ready for input");
}

void loop() {
  // Poll server for remote commands every 3 seconds
  if (millis() - lastPoll >= POLL_INTERVAL_MS) {
    lastPoll = millis();
    pollCommand();
  }

  // Check if pending QR has timed out
  if (pendingQrCode.length() > 0 && (millis() - qrTimestamp > QR_TIMEOUT_MS)) {
    Serial.println("QR code expired - please scan again");
    pendingQrCode = "";
    qrTimestamp = 0;
  }

  if (Serial.available()) {
    String command = Serial.readStringUntil('\n');
    command.trim();

    if (command.length() == 0) return;

    // Convert to lowercase for command comparison
    String lowerCmd = command;
    lowerCmd.toLowerCase();

    if (lowerCmd == "open") {
      setRelay(true);
    } else if (lowerCmd == "close") {
      setRelay(false);
    } else if (lowerCmd == "pulse") {
      pulseRelay();
    } else if (lowerCmd.startsWith("qr:")) {
      // QR code scanned - store it and wait for PIN
      pendingQrCode = command.substring(3); // Remove "qr:" prefix
      qrTimestamp = millis();
      Serial.printf("QR code stored: %s\n", pendingQrCode.c_str());
      Serial.println("Enter PIN within 60 seconds...");
    } else if (lowerCmd.startsWith("pin:")) {
      // PIN entered after QR scan
      String pin = command.substring(4); // Remove "pin:" prefix
      if (pendingQrCode.length() > 0) {
        verifyQrAndPin(pendingQrCode.c_str(), pin.c_str());
      } else {
        Serial.println("No QR code scanned. Please scan QR code first.");
      }
    } else if (command.length() == 6 && command.toInt() > 0) {
      // Legacy: 6-digit PIN only (for backward compatibility)
      if (pendingQrCode.length() > 0) {
        // If QR is pending, verify both
        verifyQrAndPin(pendingQrCode.c_str(), command.c_str());
      } else {
        // Otherwise verify PIN only
        testVerify(command.c_str());
      }
    } else {
      Serial.println("Unknown command. Use: open, close, pulse, qr:<code>, pin:<6-digits>, or just 6-digit PIN");
    }
  }
}
