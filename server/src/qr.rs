use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth::DeviceAuth;
use crate::pin::{hash_pin, VerifyPinResponse};
use crate::rate_limit;
use crate::AppState;

// --- Request / Response types ---
#[derive(Deserialize)]
pub struct GenerateQrRequest {
    pub locker_id: String,
}

#[derive(Serialize)]
pub struct GenerateQrResponse {
    pub qr_code: String,
    pub expires_at: String,
}

#[derive(Deserialize)]
pub struct VerifyQrRequest {
    pub qr_code: String,
    pub pin: String,
}

// --- QR Code format ---
// QR code format: "simon:{locker_id}:{qr_nonce}"
// where qr_nonce is a random string that gets hashed for storage

/// POST /qr/generate — generate a QR code for locker verification.
/// Called by the app when user is at the locker.
pub async fn generate_qr(
    State(state): State<AppState>,
    Json(payload): Json<GenerateQrRequest>,
) -> Result<Json<GenerateQrResponse>, Response> {
    let locker_id = payload.locker_id;

    // Verify the locker exists
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM devices WHERE locker_id = $1 AND active = true)",
    )
    .bind(&locker_id)
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

    // Generate a unique QR code
    let qr_nonce = uuid::Uuid::new_v4().to_string();
    let qr_code = format!("simon:{}", qr_nonce);
    let session_code = hex::encode(Sha256::digest(qr_code.as_bytes()));
    
    let now = Utc::now();
    let expires_at = now + Duration::minutes(5); // QR codes expire in 5 minutes

    sqlx::query(
        "INSERT INTO qr_sessions (locker_id, session_code, used, expires_at, created_at) VALUES ($1, $2, false, $3, $4)",
    )
    .bind(&locker_id)
    .bind(&session_code)
    .bind(expires_at)
    .bind(now)
    .execute(&state.db)
    .await
    .map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create QR session").into_response()
    })?;

    Ok(Json(GenerateQrResponse {
        qr_code,
        expires_at: expires_at.to_rfc3339(),
    }))
}

/// POST /qr/verify — verify a QR code + PIN combo.
/// Called by the ESP32 device when user scans QR then enters PIN.
pub async fn verify_qr(
    State(state): State<AppState>,
    device: DeviceAuth,
    Json(payload): Json<VerifyQrRequest>,
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

    // Hash the provided QR code
    let session_code = hex::encode(Sha256::digest(payload.qr_code.as_bytes()));

    // Find the QR session
    let qr_session = sqlx::query_as::<_, (uuid::Uuid, String)>(
        "SELECT id, locker_id FROM qr_sessions WHERE session_code = $1 AND used = false AND expires_at > now()",
    )
    .bind(&session_code)
    .fetch_optional(&state.db)
    .await;

    let (session_id, session_locker_id) = match qr_session {
        Ok(Some((id, lid))) => (id, lid),
        Ok(None) => {
            rate_limit::record_failure(&state.rate_limiter, locker_id);
            return (
                StatusCode::OK,
                Json(VerifyPinResponse {
                    action: "deny",
                    reason: Some("invalid_or_expired_qr"),
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

    // Verify the QR session belongs to this device
    if &session_locker_id != locker_id {
        rate_limit::record_failure(&state.rate_limiter, locker_id);
        return (
            StatusCode::OK,
            Json(VerifyPinResponse {
                action: "deny",
                reason: Some("wrong_locker"),
            }),
        );
    }

    // Now verify the PIN
    let row = sqlx::query_as::<_, (uuid::Uuid, String, String)>(
        "SELECT id, pin_hash, salt FROM pins WHERE locker_id = $1 AND used = false AND expires_at > now() ORDER BY created_at DESC LIMIT 1",
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

    // Both QR and PIN valid - mark both as used
    let now = Utc::now();
    let _ = sqlx::query("UPDATE pins SET used = true WHERE id = $1")
        .bind(pin_id)
        .execute(&state.db)
        .await;
    let _ = sqlx::query("UPDATE qr_sessions SET used = true, used_at = $1 WHERE id = $2")
        .bind(now)
        .bind(session_id)
        .execute(&state.db)
        .await;

    // Queue an "open" command for the ESP32
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
