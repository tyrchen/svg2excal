//! Bounded HTTP adapter for `svg2excal-core`.

use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    body::{Bytes, to_bytes},
    extract::{Query, State},
    http::{Request, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use svg2excal_core::{
    CancellationFlag, ConversionError, ConversionOptions, ConversionProfile, ConversionReport,
    ExcalidrawDocument, convert,
};
use tokio::{
    sync::{Semaphore, mpsc},
    task::JoinHandle,
};
use tower_http::limit::RequestBodyLimitLayer;
use tracing::{info, info_span};
use tracing_subscriber::EnvFilter;

const DEFAULT_BODY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServerConfig {
    bind: String,
    max_body_bytes: usize,
    conversion_timeout_ms: u64,
    max_concurrent_conversions: usize,
}

impl ServerConfig {
    fn load() -> Result<Self> {
        let settings = config::Config::builder()
            .set_default("bind", "127.0.0.1:3000")?
            .set_default("maxBodyBytes", i64::try_from(DEFAULT_BODY_BYTES)?)?
            .set_default("conversionTimeoutMs", 30_000_i64)?
            .set_default("maxConcurrentConversions", 4_i64)?
            .add_source(config::File::with_name("config/server").required(false))
            .add_source(config::Environment::with_prefix("SVG2EXCAL").separator("__"))
            .build()
            .context("server configuration could not be loaded")?;
        let value: Self = settings
            .try_deserialize()
            .context("server configuration is invalid")?;
        if value.bind.len() > 256
            || value.max_body_bytes == 0
            || value.max_body_bytes > DEFAULT_BODY_BYTES
            || !(1..=300_000).contains(&value.conversion_timeout_ms)
            || !(1..=16).contains(&value.max_concurrent_conversions)
        {
            anyhow::bail!("server configuration is outside safe bounds");
        }
        Ok(value)
    }
}

#[derive(Debug, Clone)]
struct AppState {
    semaphore: Arc<Semaphore>,
    request_semaphore: Arc<Semaphore>,
    timeout: Duration,
    max_body_bytes: usize,
    supervisor: mpsc::Sender<SupervisedTask>,
}

#[derive(Debug, Clone, Copy)]
struct RequestDeadline(Instant);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConvertQuery {
    #[serde(default)]
    profile: Profile,
    #[serde(default = "default_true")]
    include_report: bool,
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Profile {
    #[default]
    Balanced,
    Editable,
    Fidelity,
    Strict,
}

impl From<Profile> for ConversionProfile {
    fn from(value: Profile) -> Self {
        match value {
            Profile::Balanced => Self::Balanced,
            Profile::Editable => Self::Editable,
            Profile::Fidelity => Self::Fidelity,
            Profile::Strict => Self::Strict,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConvertResponse {
    document: ExcalidrawDocument,
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<ConversionReport>,
}

#[derive(Debug)]
struct BlockingOutput {
    json: Vec<u8>,
    source_elements: usize,
    target_elements: usize,
    fallback_pixels: u64,
}

#[derive(Debug)]
enum BlockingFailure {
    Conversion(ConversionError),
    Serialization,
}

type ConversionTask = JoinHandle<Result<BlockingOutput, BlockingFailure>>;

#[derive(Debug)]
struct SupervisedTask {
    task: ConversionTask,
    span: tracing::Span,
}

#[derive(Debug)]
enum HttpError {
    Busy,
    Timeout,
    PayloadTooLarge,
    InvalidInput,
    Internal,
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Busy => (
                StatusCode::SERVICE_UNAVAILABLE,
                "conversion-capacity-exhausted",
            ),
            Self::Timeout => (StatusCode::REQUEST_TIMEOUT, "conversion-timeout"),
            Self::PayloadTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "payload-too-large"),
            Self::InvalidInput => (StatusCode::UNPROCESSABLE_ENTITY, "invalid-svg"),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal-error"),
        };
        (status, Json(serde_json::json!({ "error": code }))).into_response()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing()?;
    let configuration = ServerConfig::load()?;
    let address: SocketAddr = configuration
        .bind
        .parse()
        .context("configured bind address is invalid")?;
    validate_bind_address(address)?;
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .context("server bind failed")?;
    info!(%address, "svg2excal server listening");
    let (application, supervisor) = router(&configuration);
    let server_result = axum::serve(listener, application)
        .with_graceful_shutdown(shutdown_signal())
        .await;
    supervisor
        .await
        .context("conversion supervisor task failed")?;
    server_result.context("HTTP server failed")
}

fn validate_bind_address(address: SocketAddr) -> Result<()> {
    if address.ip().is_loopback() {
        Ok(())
    } else {
        anyhow::bail!("the unauthenticated v1 server is restricted to loopback addresses")
    }
}

fn router(configuration: &ServerConfig) -> (Router, JoinHandle<()>) {
    let (supervisor, receiver) = mpsc::channel(configuration.max_concurrent_conversions);
    let supervisor_task = tokio::spawn(supervisor_loop(receiver));
    let state = AppState {
        semaphore: Arc::new(Semaphore::new(configuration.max_concurrent_conversions)),
        request_semaphore: Arc::new(Semaphore::new(configuration.max_concurrent_conversions)),
        timeout: Duration::from_millis(configuration.conversion_timeout_ms),
        max_body_bytes: configuration.max_body_bytes,
        supervisor,
    };
    let conversion = Router::new()
        .route("/v1/convert", post(convert_handler))
        .layer(RequestBodyLimitLayer::new(configuration.max_body_bytes))
        .layer(middleware::from_fn_with_state(state.clone(), admit_request));
    let application = Router::new()
        .route("/health", get(health))
        .merge(conversion)
        .with_state(state);
    (application, supervisor_task)
}

async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn admit_request(
    State(state): State<AppState>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let Ok(permit) = Arc::clone(&state.request_semaphore).try_acquire_owned() else {
        return HttpError::Busy.into_response();
    };
    request
        .extensions_mut()
        .insert(RequestDeadline(Instant::now() + state.timeout));
    let response = next.run(request).await;
    drop(permit);
    response
}

async fn convert_handler(
    State(state): State<AppState>,
    Query(query): Query<ConvertQuery>,
    request: Request<axum::body::Body>,
) -> Result<Response, HttpError> {
    let deadline = request
        .extensions()
        .get::<RequestDeadline>()
        .copied()
        .ok_or(HttpError::Internal)?;
    let body = tokio::time::timeout(
        remaining(deadline)?,
        to_bytes(request.into_body(), state.max_body_bytes),
    )
    .await
    .map_err(|_| HttpError::Timeout)?
    .map_err(|_| HttpError::PayloadTooLarge)?;
    let permit = Arc::clone(&state.semaphore)
        .try_acquire_owned()
        .map_err(|_| HttpError::Busy)?;
    let input_bytes = body.len();
    let started = Instant::now();
    let profile = ConversionProfile::from(query.profile);
    let include_report = query.include_report;
    let cancellation = CancellationFlag::default();
    let task_cancellation = cancellation.clone();
    let span = info_span!("conversion", input_bytes, ?profile);
    let task_span = span.clone();
    let mut task = tokio::task::spawn_blocking(move || {
        task_span.in_scope(|| {
            let _permit = permit;
            let options = ConversionOptions::builder()
                .profile(profile)
                .cancellation(task_cancellation)
                .build();
            let result = convert(&body, &options).map_err(BlockingFailure::Conversion)?;
            let response = ConvertResponse {
                document: result.document,
                report: include_report.then_some(result.report.clone()),
            };
            let json = serde_json::to_vec(&response).map_err(|_| BlockingFailure::Serialization)?;
            if json.len() > options.limits.max_serialized_json_bytes() {
                return Err(BlockingFailure::Serialization);
            }
            Ok(BlockingOutput {
                json,
                source_elements: result.report.source_elements,
                target_elements: result.report.target_elements,
                fallback_pixels: result.report.fallback_pixels,
            })
        })
    });
    let result = if let Ok(joined) = tokio::time::timeout(remaining(deadline)?, &mut task).await {
        match joined {
            Ok(Ok(result)) => result,
            Ok(Err(BlockingFailure::Conversion(ConversionError::Cancelled))) => {
                span.in_scope(|| info!(result_code = "cancelled", "conversion finished"));
                return Err(HttpError::Timeout);
            }
            Ok(Err(BlockingFailure::Conversion(_))) => {
                span.in_scope(|| info!(result_code = "invalid-input", "conversion finished"));
                return Err(HttpError::InvalidInput);
            }
            Ok(Err(BlockingFailure::Serialization)) => {
                span.in_scope(|| info!(result_code = "serialization-error", "conversion finished"));
                return Err(HttpError::Internal);
            }
            Err(_) => {
                span.in_scope(|| info!(result_code = "task-panic", "conversion finished"));
                return Err(HttpError::Internal);
            }
        }
    } else {
        cancellation.cancel();
        span.in_scope(|| info!(result_code = "timeout", "conversion finished"));
        supervise(&state, task, span.clone()).await;
        return Err(HttpError::Timeout);
    };
    span.in_scope(|| {
        info!(
            result_code = "ok",
            source_elements = result.source_elements,
            target_elements = result.target_elements,
            fallback_pixels = result.fallback_pixels,
            duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "conversion finished"
        );
    });
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        Bytes::from(result.json),
    )
        .into_response())
}

fn remaining(deadline: RequestDeadline) -> Result<Duration, HttpError> {
    deadline
        .0
        .checked_duration_since(Instant::now())
        .ok_or(HttpError::Timeout)
}

async fn supervise(state: &AppState, task: ConversionTask, span: tracing::Span) {
    let message = SupervisedTask { task, span };
    if let Err(error) = state.supervisor.send(message).await {
        supervise_one(error.0).await;
    }
}

async fn supervisor_loop(mut receiver: mpsc::Receiver<SupervisedTask>) {
    while let Some(task) = receiver.recv().await {
        supervise_one(task).await;
    }
}

async fn supervise_one(supervised: SupervisedTask) {
    let result_code = match supervised.task.await {
        Ok(Ok(_)) => "completed-after-timeout",
        Ok(Err(BlockingFailure::Conversion(ConversionError::Cancelled))) => "cancelled",
        Ok(Err(_)) => "failed-after-timeout",
        Err(_) => "panic-after-timeout",
    };
    supervised
        .span
        .in_scope(|| info!(result_code, "timed-out conversion task reaped"));
}

fn init_tracing() -> Result<()> {
    let filter = logging_filter()?;
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .json()
        .try_init()
        .map_err(|_| anyhow::anyhow!("tracing subscriber initialization failed"))
}

fn logging_filter() -> Result<EnvFilter> {
    let configured = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("svg2excal_server=info"));
    Ok(configured
        .add_directive("usvg=off".parse()?)
        .add_directive("resvg=off".parse()?))
}

async fn shutdown_signal() {
    if tokio::signal::ctrl_c().await.is_err() {
        info!(
            result_code = "signal-error",
            "shutdown signal listener ended"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Write as _, sync::mpsc as std_mpsc};

    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    use super::*;

    #[derive(Clone)]
    struct ChannelWriter(std_mpsc::Sender<Vec<u8>>);

    impl std::io::Write for ChannelWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0
                .send(bytes.to_vec())
                .map_err(|_| std::io::Error::other("log receiver closed"))?;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn test_config() -> ServerConfig {
        ServerConfig {
            bind: "127.0.0.1:0".to_owned(),
            max_body_bytes: 1024,
            conversion_timeout_ms: 5_000,
            max_concurrent_conversions: 1,
        }
    }

    #[tokio::test]
    async fn test_should_convert_bounded_request() -> Result<()> {
        let request = Request::builder()
            .method("POST")
            .uri("/v1/convert?profile=balanced&includeReport=true")
            .body(Body::from(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#fff"/></svg>"##,
            ))?;
        let response = router(&test_config()).0.oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await?.to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(
            json.get("document")
                .and_then(|value| value.get("type"))
                .and_then(serde_json::Value::as_str),
            Some("excalidraw")
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_should_reject_body_above_adapter_limit() -> Result<()> {
        let request = Request::builder()
            .method("POST")
            .uri("/v1/convert")
            .body(Body::from(vec![b'x'; 1025]))?;
        let response = router(&test_config()).0.oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        Ok(())
    }

    #[test]
    fn test_should_reject_non_loopback_bind() -> Result<()> {
        let address: SocketAddr = "0.0.0.0:3000".parse()?;
        assert!(validate_bind_address(address).is_err());
        Ok(())
    }

    #[test]
    fn test_should_unconditionally_filter_source_bearing_dependency_logs() -> Result<()> {
        let (sender, receiver) = std_mpsc::channel();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_env_filter(
                EnvFilter::new("trace")
                    .add_directive("usvg=off".parse()?)
                    .add_directive("resvg=off".parse()?),
            )
            .with_writer(move || ChannelWriter(sender.clone()))
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::error!(
                target: "usvg",
                payload = "secret-id\nhttps://attacker.invalid/\u{0007}",
                "upstream source warning"
            );
            tracing::info!(target: "svg2excal_server", result_code = "ok", "safe record");
        });
        let mut captured = Vec::new();
        for bytes in receiver.try_iter() {
            captured.write_all(&bytes)?;
        }
        let rendered = String::from_utf8(captured)?;
        assert!(rendered.contains("safe record"));
        assert!(!rendered.contains("secret-id"));
        assert!(!rendered.contains("attacker.invalid"));
        assert!(!rendered.contains('\u{0007}'));
        Ok(())
    }
}
