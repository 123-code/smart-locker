use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Json, Response},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

pub struct DeviceAuth {
    pub locker_id: String,
}

pub struct AuthRejection {
    status: StatusCode,
    message: String,
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl<S> FromRequestParts<S> for DeviceAuth
where
    S: Send + Sync,
    PgPool: FromRef<S>,
{
    type Rejection = AuthRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or(AuthRejection {
                status: StatusCode::UNAUTHORIZED,
                message: "Missing Authorization header".into(),
            })?;

        let api_key = header.strip_prefix("Bearer ").ok_or(AuthRejection {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid Authorization format, expected Bearer <key>".into(),
        })?;

        let hash = hex::encode(Sha256::digest(api_key.as_bytes()));
        let pool = PgPool::from_ref(state);

        let row = sqlx::query_scalar::<_, String>(
            "SELECT locker_id FROM devices WHERE api_key_hash = $1 AND active = true",
        )
        .bind(&hash)
        .fetch_optional(&pool)
        .await
        .map_err(|_| AuthRejection {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "Database error".into(),
        })?;

        match row {
            Some(locker_id) => Ok(DeviceAuth { locker_id }),
            None => Err(AuthRejection {
                status: StatusCode::UNAUTHORIZED,
                message: "Invalid API key".into(),
            }),
        }
    }
}

use axum::extract::FromRef;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_auth_struct_creation() {
        let auth = DeviceAuth {
            locker_id: "test_locker".to_string(),
        };
        
        assert_eq!(auth.locker_id, "test_locker");
    }

    #[test]
    fn test_auth_rejection_message() {
        let rejection = AuthRejection {
            status: StatusCode::UNAUTHORIZED,
            message: "Test error message".into(),
        };
        
        assert_eq!(rejection.status, StatusCode::UNAUTHORIZED);
        assert_eq!(rejection.message, "Test error message");
    }
}
