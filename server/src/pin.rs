use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth::DeviceAuth;
use crate::rate_limit;
use crate::AppState;

// --- Request / Response types ---

#[derive(Deserialize)]
pub struct CreatePinRequest {
    pub locker_id: String,
    pub recipient_phone: Option<String>,
}

#[derive(Serialize)]
pub struct CreatePinResponse {
    pub pin: String,
    pub expires_at: String,
}

#[derive(Deserialize)]
pub struct VerifyPinRequest {
    pub pin: String,
}

#[derive(Serialize)]
pub struct VerifyPinResponse {
    pub action: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

// --- Helpers ---

fn hash_pin(pin: &str, salt: &str) -> String {
    let input = format!("{}{}", pin, salt);
    hex::encode(Sha256::digest(input.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_pin_produces_consistent_output() {
        let pin = "123456";
        let salt = "test_salt";
        
        let hash1 = hash_pin(pin, salt);
        let hash2 = hash_pin(pin, salt);
        
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_pin_different_pins_produce_different_hashes() {
        let salt = "test_salt";
        
        let hash1 = hash_pin("123456", salt);
        let hash2 = hash_pin("123457", salt);
        
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_hash_pin_different_salts_produce_different_hashes() {
        let pin = "123456";
        
        let hash1 = hash_pin(pin, "salt1");
        let hash2 = hash_pin(pin, "salt2");
        
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_hash_pin_produces_valid_hex() {
        let hash = hash_pin("000000", "salt");
        
        // Should be valid hex (64 chars for SHA256)
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

// --- Handlers ---

/// POST /pins — generate a one-time PIN for a locker.
/// Called by the app/web on behalf of a user.
pub async fn create_pin(
    State(state): State<AppState>,
    Json(payload): Json<CreatePinRequest>,
) -> Result<Json<CreatePinResponse>, Response> {
    let locker_id = &payload.locker_id;

    // Verify the locker exists
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM devices WHERE locker_id = $1 AND active = true)",
    )
    .bind(locker_id)
    .fetch_one(&state.db)
    .await
    .map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response()
    })?;

    if !exists {
        return Err(
            (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Locker not found"}))).into_response()
        );
    }

    // Invalidate any existing unused PINs for this locker
    let _ = sqlx::query("UPDATE pins SET used = true WHERE locker_id = $1 AND used = false")
        .bind(locker_id)
        .execute(&state.db)
        .await;

    // Generate PIN
    let pin_raw: u32 = rand::random_range(0..1_000_000);
    let pin_str = format!("{:06}", pin_raw);

    let now = Utc::now();
    let expires_at = now + Duration::minutes(10);
    let salt = format!("{}:{}", locker_id, now.timestamp());

    let pin_hash = hash_pin(&pin_str, &salt);

    sqlx::query(
        "INSERT INTO pins (locker_id, pin_hash, salt, used, expires_at, created_at)
         VALUES ($1, $2, $3, false, $4, $5)",
    )
    .bind(locker_id)
    .bind(&pin_hash)
    .bind(&salt)
    .bind(expires_at)
    .bind(now)
    .execute(&state.db)
    .await
    .map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create PIN").into_response()
    })?;

    // Send SMS if Twilio is configured and a phone number was provided
    if let (Some(twilio), Some(phone)) = (&state.twilio, &payload.recipient_phone) {
        let twilio = twilio.clone();
        let pin_for_sms = pin_str.clone();
        let phone = phone.clone();
        tokio::spawn(async move {
            if let Err(e) = twilio.send_pin_sms(&phone, &pin_for_sms).await {
                tracing::error!("Failed to send SMS: {}", e);
            }
        });
    }

    Ok(Json(CreatePinResponse {
        pin: pin_str,
        expires_at: expires_at.to_rfc3339(),
    }))
}

/// POST /pins/verify — verify a PIN and open the locker.
/// Called by the ESP32 device.
pub async fn verify_pin(
    State(state): State<AppState>,
    device: DeviceAuth,
    Json(payload): Json<VerifyPinRequest>,
) -> (StatusCode, Json<VerifyPinResponse>) {
    let locker_id = &device.locker_id;

    // Check rate limit
    if !rate_limit::check_rate_limit(&state.rate_limiter, locker_id) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(VerifyPinResponse {
                action: "deny",
                reason: Some("too_many_attempts"),
            }),
        );
    }

    // Find the latest valid PIN for this locker
    let row = sqlx::query_as::<_, (uuid::Uuid, String, String)>(
        "SELECT id, pin_hash, salt FROM pins
         WHERE locker_id = $1 AND used = false AND expires_at > now()
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(locker_id)
    .fetch_optional(&state.db)
    .await;

    let row = match row {
        Ok(Some(r)) => r,
        Ok(None) => {
            rate_limit::record_failure(&state.rate_limiter, locker_id);
            return (
                StatusCode::OK,
                Json(VerifyPinResponse {
                    action: "deny",
                    reason: Some("no_active_pin"),
                }),
            );
        }
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(VerifyPinResponse {
                    action: "deny",
                    reason: Some("server_error"),
                }),
            );
        }
    };

    let (pin_id, stored_hash, salt) = row;
    let submitted_hash = hash_pin(&payload.pin, &salt);

    if submitted_hash != stored_hash {
        rate_limit::record_failure(&state.rate_limiter, locker_id);
        return (
            StatusCode::OK,
            Json(VerifyPinResponse {
                action: "deny",
                reason: Some("invalid_pin"),
            }),
        );
    }

    // PIN matches — mark as used
    let _ = sqlx::query("UPDATE pins SET used = true WHERE id = $1")
        .bind(pin_id)
        .execute(&state.db)
        .await;

    // Queue an "open" command for the ESP32 to pick up via polling
    state
        .pending_commands
        .insert(locker_id.clone(), "open".to_string());

    (
        StatusCode::OK,
        Json(VerifyPinResponse {
            action: "open",
            reason: None,
        }),
    )
}

// --- Command polling ---

#[derive(Serialize)]
pub struct PollCommandResponse {
    pub command: String,
}

/// GET /commands/poll — ESP32 polls this to check for pending commands.
pub async fn poll_command(
    State(state): State<AppState>,
    device: DeviceAuth,
) -> Json<PollCommandResponse> {
    let command = state
        .pending_commands
        .remove(&device.locker_id)
        .map(|(_, cmd)| cmd)
        .unwrap_or_else(|| "none".to_string());

    Json(PollCommandResponse { command })
}
