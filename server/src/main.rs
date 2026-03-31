use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{delete, get, post, put},
    Router,
};
use chrono::DateTime;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

mod auth;
mod db;
mod package;
mod pin;
mod qr;
mod rate_limit;
mod sms;

// --- Existing CRUD types ---
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Item {
    id: String,
    name: String,
    description: String,
}

#[derive(Clone)]
pub struct AppState {
    items: Arc<RwLock<HashMap<String, Item>>>,
    pub db: PgPool,
    pub rate_limiter: Arc<DashMap<String, Vec<DateTime<chrono::Utc>>>>,
    pub pending_commands: Arc<DashMap<String, String>>,
    pub twilio: Option<Arc<sms::TwilioConfig>>,
}

impl axum::extract::FromRef<AppState> for PgPool {
    fn from_ref(state: &AppState) -> Self {
        state.db.clone()
    }
}

// --- Existing CRUD handlers ---
async fn create_item(
    State(state): State<AppState>,
    Json(payload): Json<CreateItemRequest>,
) -> (StatusCode, Json<Item>) {
    let id = uuid::Uuid::new_v4().to_string();
    let item = Item {
        id: id.clone(),
        name: payload.name,
        description: payload.description,
    };
    state.items.write().await.insert(id, item.clone());
    (StatusCode::CREATED, Json(item))
}

async fn get_items(State(state): State<AppState>) -> Json<Vec<Item>> {
    let items: Vec<Item> = state.items.read().await.values().cloned().collect();
    Json(items)
}

async fn get_item(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Item>, StatusCode> {
    let items = state.items.read().await;
    items
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn update_item(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateItemRequest>,
) -> Result<Json<Item>, StatusCode> {
    let mut items = state.items.write().await;
    if let Some(item) = items.get_mut(&id) {
        if let Some(name) = payload.name {
            item.name = name;
        }
        if let Some(description) = payload.description {
            item.description = description;
        }
        Ok(Json(item.clone()))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn delete_item(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    if state.items.write().await.remove(&id).is_some() {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

#[derive(Deserialize)]
struct CreateItemRequest {
    name: String,
    description: String,
}

#[derive(Deserialize)]
struct UpdateItemRequest {
    name: Option<String>,
    description: Option<String>,
}

// --- Main ---
#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let pool = db::init_pool().await;
    let twilio = sms::TwilioConfig::from_env().map(Arc::new);
    if twilio.is_some() {
        println!("Twilio SMS enabled");
    } else {
        println!("Twilio SMS disabled (set TWILIO_ACCOUNT_SID, TWILIO_AUTH_TOKEN, TWILIO_FROM_NUMBER to enable)");
    }

    let state = AppState {
        items: Arc::new(RwLock::new(HashMap::new())),
        db: pool.clone(),
        rate_limiter: Arc::new(DashMap::new()),
        pending_commands: Arc::new(DashMap::new()),
        twilio,
    };

    // Background task: clean up expired PINs every 5 minutes
    let cleanup_pool = pool.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            let _ = sqlx::query(
                "DELETE FROM pins WHERE expires_at < now() - INTERVAL '1 hour'",
            )
            .execute(&cleanup_pool)
            .await;
        }
    });

    let app = Router::new()
        // Existing CRUD
        .route("/items", post(create_item))
        .route("/items", get(get_items))
        .route("/items/{id}", get(get_item))
        .route("/items/{id}", put(update_item))
        .route("/items/{id}", delete(delete_item))
        // PIN system
        .route("/pins", post(pin::create_pin))
        .route("/pins/verify", post(pin::verify_pin))
        // QR verification system
        .route("/qr/generate", post(qr::generate_qr))
        .route("/qr/verify", post(qr::verify_qr))
        // Command polling (ESP32 polls this)
        .route("/commands/poll", get(pin::poll_command))
        // Package system
        .route("/users", post(package::get_or_create_user))
        .route("/users/{phone}", get(package::get_user))
        .route("/packages", post(package::create_package))
        .route("/packages/{id}", get(package::get_package))
        .route("/packages/phone/{phone}", get(package::get_packages_by_phone))
        .route("/packages/{id}/deliverer", post(package::assign_deliverer))
        .route("/packages/{id}/status", put(package::update_package_status))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();

    println!("Server running at http://127.0.0.1:3000");
    println!("CRUD Endpoints:");
    println!("  POST /items - Create item");
    println!("  GET /items - List all items");
    println!("  GET /items/{{id}} - Get item by id");
    println!("  PUT /items/{{id}} - Update item");
    println!("  DELETE /items/{{id}} - Delete item");
    println!("PIN Endpoints:");
    println!("  POST /pins - Generate a one-time PIN");
    println!("  POST /pins/verify - Verify PIN (ESP32 device)");
    println!("QR Endpoints:");
    println!("  POST /qr/generate - Generate QR code for locker verification");
    println!("  POST /qr/verify - Verify QR + PIN combo (ESP32 device)");
    println!("  GET /commands/poll - Poll for pending commands (ESP32)");

    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_serialization() {
        let item = Item {
            id: "test-id".to_string(),
            name: "Test Item".to_string(),
            description: "A test description".to_string(),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("test-id"));
        assert!(json.contains("Test Item"));
    }

    #[test]
    fn test_item_deserialization() {
        let json = r#"{"id":"test-id","name":"Test","description":"Desc"}"#;
        let item: Item = serde_json::from_str(json).unwrap();
        assert_eq!(item.id, "test-id");
        assert_eq!(item.name, "Test");
        assert_eq!(item.description, "Desc");
    }

    #[test]
    fn test_create_item_request() {
        let req = CreateItemRequest {
            name: "Test".to_string(),
            description: "Description".to_string(),
        };
        assert_eq!(req.name, "Test");
        assert_eq!(req.description, "Description");
    }

    #[test]
    fn test_update_item_request_partial() {
        let req = UpdateItemRequest {
            name: Some("New Name".to_string()),
            description: None,
        };
        assert_eq!(req.name, Some("New Name".to_string()));
        assert_eq!(req.description, None);
    }
}
