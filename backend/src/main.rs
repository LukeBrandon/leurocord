use axum::{
    async_trait,
    extract::{FromRef, FromRequestParts},
    http::{request::Parts, StatusCode},
    Router,
};
use tower::ServiceBuilder;

use sqlx::postgres::{PgPool, PgPoolOptions};
use std::{net::SocketAddr, time::Duration};
use tower_http::{add_extension::AddExtensionLayer, cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::EnvFilter;

mod db;
mod routes;
mod configuration;
mod controllers;
mod errors;
mod models;

#[cfg(test)]
mod tests;

#[tokio::main]
async fn main() {
    let config = configuration::load_config();
    let port = config.port.0;

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .pretty()
        .init();
  
    // Build our server
    let app = Router::new()
        .merge(routes::user_router())
        .merge(routes::channel_router())
        .merge(routes::message_router())
        .layer(middleware_stack);

    // Run our service with hyper
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("Listening on {}", addr);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

// we can also write a custom extractor that grabs a connection from the pool
// which setup is appropriate depends on your application
struct DatabaseConnection(sqlx::pool::PoolConnection<sqlx::Postgres>);

#[async_trait]
impl<S> FromRequestParts<S> for DatabaseConnection
where
PgPool: FromRef<S>,
S: Send + Sync,
{
    type Rejection = (StatusCode, String);

    async fn from_request_parts(_parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let pool = PgPool::from_ref(state);

        let conn = pool.acquire().await.map_err(internal_error)?;

        Ok(Self(conn))
    }
}

/// Utility function for mapping any error into a `500 Internal Server Error`
/// response.
fn internal_error<E>(err: E) -> (StatusCode, String)
    where
    E: std::error::Error,
{
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}
