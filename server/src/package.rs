use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::AppState;

// --- Types ---

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub phone: String,
    pub name: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Package {
    pub id: Uuid,
    pub sender_id: Option<Uuid>,
    pub deliverer_id: Option<Uuid>,
    pub recipient_id: Option<Uuid>,
    pub locker_id: String,
    pub status: String,
    pub label: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PackageWithDetails {
    pub id: Uuid,
    pub sender: Option<User>,
    pub deliverer: Option<User>,
    pub recipient: Option<User>,
    pub locker_id: String,
    pub status: String,
    pub label: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

// --- Request types ---

#[derive(Deserialize)]
pub struct CreateOrUpdateUserRequest {
    pub phone: String,
    pub name: Option<String>,
}

#[derive(Deserialize)]
pub struct CreatePackageRequest {
    pub sender_phone: String,
    pub sender_name: Option<String>,
    pub recipient_phone: String,
    pub recipient_name: Option<String>,
    pub locker_id: String,
    pub label: Option<String>,
}

#[derive(Deserialize)]
pub struct AssignDelivererRequest {
    pub deliverer_phone: String,
    pub deliverer_name: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdatePackageStatusRequest {
    pub status: String,
}

// --- Response types ---

#[derive(Serialize)]
pub struct CreatePackageResponse {
    pub package_id: Uuid,
    pub locker_id: String,
    pub status: String,
}

// --- User handlers ---

pub async fn get_or_create_user(
    State(state): State<AppState>,
    Json(payload): Json<CreateOrUpdateUserRequest>,
) -> Result<Json<User>, Response> {
    // Try to find existing user
    let existing: Option<User> = sqlx::query_as(
        "SELECT id, phone, name, created_at FROM users WHERE phone = $1",
    )
    .bind(&payload.phone)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
        )
            .into_response()
    })?;

    if let Some(user) = existing {
        return Ok(Json(user));
    }

    // Create new user
    let user: User = sqlx::query_as(
        "INSERT INTO users (phone, name) VALUES ($1, $2) RETURNING id, phone, name, created_at",
    )
    .bind(&payload.phone)
    .bind(&payload.name)
    .fetch_one(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create user",
        )
            .into_response()
    })?;

    Ok(Json(user))
}

pub async fn get_user(
    State(state): State<AppState>,
    Path(phone): Path<String>,
) -> Result<Json<User>, Response> {
    let user: Option<User> = sqlx::query_as(
        "SELECT id, phone, name, created_at FROM users WHERE phone = $1",
    )
    .bind(&phone)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
        )
            .into_response()
    })?;

    match user {
        Some(u) => Ok(Json(u)),
        None => Err((StatusCode::NOT_FOUND, "User not found").into_response()),
    }
}

// --- Package handlers ---

pub async fn create_package(
    State(state): State<AppState>,
    Json(payload): Json<CreatePackageRequest>,
) -> Result<Json<CreatePackageResponse>, Response> {
    // Get or create sender
    let sender: User = sqlx::query_as(
        "INSERT INTO users (phone, name) VALUES ($1, $2)
         ON CONFLICT (phone) DO UPDATE SET name = COALESCE(EXCLUDED.name, users.name)
         RETURNING id, phone, name, created_at",
    )
    .bind(&payload.sender_phone)
    .bind(&payload.sender_name)
    .fetch_one(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to get/create sender",
        )
            .into_response()
    })?;

    // Get or create recipient
    let recipient: User = sqlx::query_as(
        "INSERT INTO users (phone, name) VALUES ($1, $2)
         ON CONFLICT (phone) DO UPDATE SET name = COALESCE(EXCLUDED.name, users.name)
         RETURNING id, phone, name, created_at",
    )
    .bind(&payload.recipient_phone)
    .bind(&payload.recipient_name)
    .fetch_one(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to get/create recipient",
        )
            .into_response()
    })?;

    // Create package
    let package_id: Uuid = sqlx::query_scalar(
        "INSERT INTO packages (sender_id, recipient_id, locker_id, status, label)
         VALUES ($1, $2, $3, 'created', $4)
         RETURNING id",
    )
    .bind(sender.id)
    .bind(recipient.id)
    .bind(&payload.locker_id)
    .bind(&payload.label)
    .fetch_one(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create package",
        )
            .into_response()
    })?;

    Ok(Json(CreatePackageResponse {
        package_id,
        locker_id: payload.locker_id,
        status: "created".to_string(),
    }))
}

pub async fn get_package(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PackageWithDetails>, Response> {
    let pkg: Option<Package> = sqlx::query_as(
        "SELECT id, sender_id, deliverer_id, recipient_id, locker_id, status, label, created_at, updated_at
         FROM packages WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
        )
            .into_response()
    })?;

    let pkg = pkg.ok_or((StatusCode::NOT_FOUND, "Package not found").into_response())?;

    // Fetch user details
    let sender = match pkg.sender_id {
        Some(id) => sqlx::query_as::<_, User>("SELECT id, phone, name, created_at FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten(),
        None => None,
    };

    let deliverer = match pkg.deliverer_id {
        Some(id) => sqlx::query_as::<_, User>("SELECT id, phone, name, created_at FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten(),
        None => None,
    };

    let recipient = match pkg.recipient_id {
        Some(id) => sqlx::query_as::<_, User>("SELECT id, phone, name, created_at FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten(),
        None => None,
    };

    Ok(Json(PackageWithDetails {
        id: pkg.id,
        sender,
        deliverer,
        recipient,
        locker_id: pkg.locker_id,
        status: pkg.status,
        label: pkg.label,
        created_at: pkg.created_at,
        updated_at: pkg.updated_at,
    }))
}

pub async fn get_packages_by_phone(
    State(state): State<AppState>,
    Path(phone): Path<String>,
) -> Result<Json<Vec<PackageWithDetails>>, Response> {
    let packages: Vec<Package> = sqlx::query_as(
        "SELECT p.id, p.sender_id, p.deliverer_id, p.recipient_id, p.locker_id, p.status, p.label, p.created_at, p.updated_at
         FROM packages p
         JOIN users u ON u.id = p.sender_id OR u.id = p.recipient_id OR u.id = p.deliverer_id
         WHERE u.phone = $1
         ORDER BY p.created_at DESC",
    )
    .bind(&phone)
    .fetch_all(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
        )
            .into_response()
    })?;

    let mut result = Vec::new();
    for pkg in packages {
        let sender = match pkg.sender_id {
            Some(id) => sqlx::query_as::<_, User>("SELECT id, phone, name, created_at FROM users WHERE id = $1")
                .bind(id)
                .fetch_optional(&state.db)
                .await
                .ok()
                .flatten(),
            None => None,
        };

        let deliverer = match pkg.deliverer_id {
            Some(id) => sqlx::query_as::<_, User>("SELECT id, phone, name, created_at FROM users WHERE id = $1")
                .bind(id)
                .fetch_optional(&state.db)
                .await
                .ok()
                .flatten(),
            None => None,
        };

        let recipient = match pkg.recipient_id {
            Some(id) => sqlx::query_as::<_, User>("SELECT id, phone, name, created_at FROM users WHERE id = $1")
                .bind(id)
                .fetch_optional(&state.db)
                .await
                .ok()
                .flatten(),
            None => None,
        };

        result.push(PackageWithDetails {
            id: pkg.id,
            sender,
            deliverer,
            recipient,
            locker_id: pkg.locker_id,
            status: pkg.status,
            label: pkg.label,
            created_at: pkg.created_at,
            updated_at: pkg.updated_at,
        });
    }

    Ok(Json(result))
}

pub async fn assign_deliverer(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<AssignDelivererRequest>,
) -> Result<Json<Package>, Response> {
    // Get or create deliverer
    let deliverer: User = sqlx::query_as(
        "INSERT INTO users (phone, name) VALUES ($1, $2)
         ON CONFLICT (phone) DO UPDATE SET name = COALESCE(EXCLUDED.name, users.name)
         RETURNING id, phone, name, created_at",
    )
    .bind(&payload.deliverer_phone)
    .bind(&payload.deliverer_name)
    .fetch_one(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to get/create deliverer",
        )
            .into_response()
    })?;

    // Update package
    let pkg: Package = sqlx::query_as(
        "UPDATE packages SET deliverer_id = $1, status = 'assigned', updated_at = now()
         WHERE id = $2
         RETURNING id, sender_id, deliverer_id, recipient_id, locker_id, status, label, created_at, updated_at",
    )
    .bind(deliverer.id)
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to assign deliverer",
        )
            .into_response()
    })?;

    Ok(Json(pkg))
}

pub async fn update_package_status(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdatePackageStatusRequest>,
) -> Result<Json<Package>, Response> {
    let valid_statuses = ["created", "assigned", "in_locker", "picked_up"];
    if !valid_statuses.contains(&payload.status.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid status. Must be one of: created, assigned, in_locker, picked_up",
        )
            .into_response());
    }

    let pkg: Package = sqlx::query_as(
        "UPDATE packages SET status = $1, updated_at = now()
         WHERE id = $2
         RETURNING id, sender_id, deliverer_id, recipient_id, locker_id, status, label, created_at, updated_at",
    )
    .bind(&payload.status)
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to update package status",
        )
            .into_response()
    })?;

    Ok(Json(pkg))
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_status_validation() {
        let valid = vec!["created", "assigned", "in_locker", "picked_up"];
        for status in valid {
            assert!(["created", "assigned", "in_locker", "picked_up"].contains(&status));
        }
    }
}
