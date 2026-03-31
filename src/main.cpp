#include <WiFi.h>
#include <HTTPClient.h>

constexpr int RELAY_PIN = 26;
constexpr unsigned long RELAY_OPEN_MS = 4000;

const char* WIFI_SSID     = "FANARMA";
const char* WIFI_PASS     = "SucoMAX2021";
const char* SERVER_URL    = "http://192.168.1.153:3000/pins/verify";
const char* POLL_URL      = "http://192.168.1.153:3000/commands/poll";
const char* API_KEY       = "test-device-key-123";

unsigned long lastPoll = 0;
constexpr unsigned long POLL_INTERVAL_MS = 3000;

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

void setup() {
  Serial.begin(115200);
  delay(1000);

  setRelay(false);

  Serial.println("\n=== ESP32 Locker Test ===");
  Serial.println("Commands: open, close, pulse, or 6-digit PIN");
  Serial.print("Connecting to WiFi");

  WiFi.begin(WIFI_SSID, WIFI_PASS);
  while (WiFi.status() != WL_CONNECTED) {
    delay(500);
    Serial.print(".");
  }

  Serial.printf("\nConnected! IP: %s\n", WiFi.localIP().toString().c_str());
  Serial.println("Ready for PIN input");
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

void loop() {
  // Poll server for remote commands every 3 seconds
  if (millis() - lastPoll >= POLL_INTERVAL_MS) {
    lastPoll = millis();
    pollCommand();
  }

  if (Serial.available()) {
    String command = Serial.readStringUntil('\n');
    command.trim();
    command.toLowerCase();

    if (command == "open") {
      setRelay(true);
    } else if (command == "close") {
      setRelay(false);
    } else if (command == "pulse") {
      pulseRelay();
    } else if (command.length() == 6) {
      testVerify(command.c_str());
    } else if (command.length() > 0) {
      Serial.println("Use: open, close, pulse, or a 6-digit PIN");
    }
  }
}
