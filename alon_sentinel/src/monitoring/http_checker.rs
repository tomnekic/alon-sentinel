use std::{
    net::SocketAddr,
    num::NonZeroUsize,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use rustls::{
    ClientConfig, DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use rustls_platform_verifier::ConfigVerifierExt;
use tokio::{net::TcpStream, time::timeout as tokio_timeout};
use tokio_rustls::{TlsConnector, client::TlsStream};

use crate::{
    domain::site_monitors::{HttpHeaderAssertion, JsonPathValueAssertion},
    net,
};

const RUNTIME_HTTP_CLIENT_CACHE_CAPACITY: usize = 256;
const DEFAULT_SSL_EXPIRY_WARNING_DAYS: i32 = 14;
const SSL_EXPIRY_CRITICAL_DAYS: i32 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertificateMetadata {
    pub expires_at: DateTime<Utc>,
    pub days_remaining: i32,
    pub issuer: String,
    pub subject: String,
    pub domain: String,
}

pub struct CheckResult {
    pub is_success: bool,
    pub status_code: Option<i32>,
    pub response_time_ms: Option<i32>,
    pub failure_reason: Option<String>,
    pub error_message: Option<String>,
    pub certificate_metadata: Option<CertificateMetadata>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HttpAssertions<'a> {
    pub body_must_contain: Option<&'a str>,
    pub body_must_not_contain: Option<&'a str>,
    pub body_must_contain_texts: &'a [String],
    pub body_must_not_contain_texts: &'a [String],
    pub json_path_exists: &'a [String],
    pub json_path_equals: &'a [JsonPathValueAssertion],
    pub json_path_not_equals: &'a [JsonPathValueAssertion],
    pub max_response_time_ms: Option<i32>,
    pub required_header_name: Option<&'a str>,
    pub required_header_value: Option<&'a str>,
    pub header_assertions: &'a [HttpHeaderAssertion],
    pub ssl_certificate_checks_enabled: bool,
    pub ssl_expiry_warning_days: Option<i32>,
}

pub struct HttpCheckConfig {
    pub max_response_body_bytes: usize,
    pub validate_public_target: bool,
}

pub async fn check_url(
    url: &str,
    expected_status_code: i32,
    assertions: HttpAssertions<'_>,
    timeout: Duration,
    config: HttpCheckConfig,
) -> CheckResult {
    check_url_internal(url, expected_status_code, assertions, timeout, config).await
}

pub async fn check_ssl_certificate(
    url: &str,
    warning_days: Option<i32>,
    timeout: Duration,
    validate_public_target: bool,
) -> CheckResult {
    check_ssl_certificate_internal(url, warning_days, timeout, validate_public_target).await
}

async fn check_url_internal(
    url: &str,
    expected_status_code: i32,
    assertions: HttpAssertions<'_>,
    timeout: Duration,
    config: HttpCheckConfig,
) -> CheckResult {
    let start: Instant = Instant::now();

    let target = match resolve_runtime_request_target(url, config.validate_public_target).await {
        Ok(target) => target,
        Err((failure_reason, error_message)) => {
            return CheckResult {
                is_success: false,
                status_code: None,
                response_time_ms: Some(start.elapsed().as_millis() as i32),
                failure_reason: Some(failure_reason.to_string()),
                error_message: Some(error_message),
                certificate_metadata: None,
            };
        }
    };

    let certificate_metadata = if assertions.ssl_certificate_checks_enabled {
        match evaluate_ssl_certificate_for_target(
            &target,
            assertions
                .ssl_expiry_warning_days
                .unwrap_or(DEFAULT_SSL_EXPIRY_WARNING_DAYS),
            timeout,
        )
        .await
        {
            Ok(metadata) => Some(metadata),
            Err(result) => {
                return CheckResult {
                    response_time_ms: Some(start.elapsed().as_millis() as i32),
                    ..result
                };
            }
        }
    } else {
        None
    };

    let client = match build_runtime_client(&target).await {
        Ok(client) => client,
        Err((failure_reason, error_message)) => {
            return CheckResult {
                is_success: false,
                status_code: None,
                response_time_ms: Some(start.elapsed().as_millis() as i32),
                failure_reason: Some(failure_reason.to_string()),
                error_message: Some(error_message),
                certificate_metadata,
            };
        }
    };

    match client.get(target.url.clone()).timeout(timeout).send().await {
        Ok(mut response) => {
            let elapsed: i32 = start.elapsed().as_millis() as i32;
            let status_code: i32 = response.status().as_u16() as i32;
            let mut is_success: bool = status_code == expected_status_code;
            let mut failure_reason = if is_success {
                None
            } else if response.status().is_redirection() {
                Some("redirect_blocked".to_string())
            } else {
                Some("unexpected_status".to_string())
            };
            let mut error_message = if is_success {
                None
            } else if response.status().is_redirection() {
                let redirect_target = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("<unknown>");
                Some(format!(
                    "Redirect blocked with HTTP status: {}, location: {}",
                    status_code, redirect_target
                ))
            } else {
                Some(format!(
                    "Unexpected HTTP status: {}, expected: {}",
                    status_code, expected_status_code
                ))
            };

            if is_success
                && let Some(max_response_time_ms) = assertions.max_response_time_ms
                && elapsed > max_response_time_ms
            {
                is_success = false;
                failure_reason = Some(assertion_failure_reason("max_response_time_ms"));
                error_message = Some(format!(
                    "Response time {}ms exceeded max_response_time_ms {}ms",
                    elapsed, max_response_time_ms
                ));
            }

            if is_success && let Some(required_header_name) = assertions.required_header_name {
                let required_header_assertion = HttpHeaderAssertion {
                    name: required_header_name.to_string(),
                    equals: assertions.required_header_value.map(ToOwned::to_owned),
                    contains: None,
                };

                if let Err((new_failure_reason, new_error_message)) = evaluate_header_assertion(
                    response.headers(),
                    &required_header_assertion,
                    "required_header",
                ) {
                    is_success = false;
                    failure_reason = Some(new_failure_reason);
                    error_message = Some(new_error_message);
                }
            }

            if is_success {
                for (index, assertion) in assertions.header_assertions.iter().enumerate() {
                    if let Err((new_failure_reason, new_error_message)) = evaluate_header_assertion(
                        response.headers(),
                        assertion,
                        &format!("header_assertions[{index}]"),
                    ) {
                        is_success = false;
                        failure_reason = Some(new_failure_reason);
                        error_message = Some(new_error_message);
                        break;
                    }
                }
            }

            if is_success
                && (assertions.body_must_contain.is_some()
                    || assertions.body_must_not_contain.is_some()
                    || !assertions.body_must_contain_texts.is_empty()
                    || !assertions.body_must_not_contain_texts.is_empty()
                    || !assertions.json_path_exists.is_empty()
                    || !assertions.json_path_equals.is_empty()
                    || !assertions.json_path_not_equals.is_empty())
            {
                let body_result = async {
                    let mut buf = Vec::with_capacity(config.max_response_body_bytes.min(4096));
                    while let Some(chunk) = response.chunk().await? {
                        let remaining = config.max_response_body_bytes.saturating_sub(buf.len());
                        buf.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
                        if buf.len() >= config.max_response_body_bytes {
                            break;
                        }
                    }
                    Ok::<_, reqwest::Error>(buf)
                }
                .await;
                match body_result {
                    Ok(body) => {
                        let body = String::from_utf8_lossy(&body);
                        if let Some(required) = assertions.body_must_contain
                            && !body.contains(required)
                        {
                            is_success = false;
                            failure_reason = Some(assertion_failure_reason("body_must_contain"));
                            error_message = Some(format!(
                                "Response body did not contain required text: {}",
                                required
                            ));
                        }

                        if is_success {
                            for (index, required) in
                                assertions.body_must_contain_texts.iter().enumerate()
                            {
                                if !body.contains(required) {
                                    is_success = false;
                                    failure_reason = Some(assertion_failure_reason(&format!(
                                        "body_must_contain_texts[{index}]"
                                    )));
                                    error_message = Some(format!(
                                        "Response body did not contain required text: {}",
                                        required
                                    ));
                                    break;
                                }
                            }
                        }

                        if is_success
                            && let Some(forbidden) = assertions.body_must_not_contain
                            && body.contains(forbidden)
                        {
                            is_success = false;
                            failure_reason =
                                Some(assertion_failure_reason("body_must_not_contain"));
                            error_message = Some(format!(
                                "Response body contained forbidden text: {}",
                                forbidden
                            ));
                        }

                        if is_success {
                            for (index, forbidden) in
                                assertions.body_must_not_contain_texts.iter().enumerate()
                            {
                                if body.contains(forbidden) {
                                    is_success = false;
                                    failure_reason = Some(assertion_failure_reason(&format!(
                                        "body_must_not_contain_texts[{index}]"
                                    )));
                                    error_message = Some(format!(
                                        "Response body contained forbidden text: {}",
                                        forbidden
                                    ));
                                    break;
                                }
                            }
                        }

                        if is_success
                            && (!assertions.json_path_exists.is_empty()
                                || !assertions.json_path_equals.is_empty()
                                || !assertions.json_path_not_equals.is_empty())
                        {
                            let json_body: serde_json::Value = match serde_json::from_str(&body) {
                                Ok(json_body) => json_body,
                                Err(error) => {
                                    is_success = false;
                                    failure_reason = Some(assertion_failure_reason("json_body"));
                                    error_message =
                                        Some(format!("Response body was not valid JSON: {error}"));
                                    return CheckResult {
                                        is_success,
                                        status_code: Some(status_code),
                                        response_time_ms: Some(elapsed),
                                        failure_reason,
                                        error_message,
                                        certificate_metadata: certificate_metadata.clone(),
                                    };
                                }
                            };

                            for (index, path) in assertions.json_path_exists.iter().enumerate() {
                                match resolve_json_path(&json_body, path) {
                                    Ok(Some(_)) => {}
                                    Ok(None) => {
                                        is_success = false;
                                        failure_reason = Some(assertion_failure_reason(&format!(
                                            "json_path_exists[{index}]"
                                        )));
                                        error_message =
                                            Some(format!("JSON path {} did not exist", path));
                                        break;
                                    }
                                    Err(error) => {
                                        is_success = false;
                                        failure_reason = Some(assertion_failure_reason(&format!(
                                            "json_path_exists[{index}]"
                                        )));
                                        error_message = Some(error);
                                        break;
                                    }
                                }
                            }

                            if is_success {
                                for (index, assertion) in
                                    assertions.json_path_equals.iter().enumerate()
                                {
                                    match resolve_json_path(&json_body, &assertion.path) {
                                        Ok(Some(actual_value))
                                            if actual_value == &assertion.value => {}
                                        Ok(Some(actual_value)) => {
                                            is_success = false;
                                            failure_reason = Some(assertion_failure_reason(
                                                &format!("json_path_equals[{index}]"),
                                            ));
                                            error_message = Some(format!(
                                                "JSON path {} had value {}, expected {}",
                                                assertion.path, actual_value, assertion.value
                                            ));
                                            break;
                                        }
                                        Ok(None) => {
                                            is_success = false;
                                            failure_reason = Some(assertion_failure_reason(
                                                &format!("json_path_equals[{index}]"),
                                            ));
                                            error_message = Some(format!(
                                                "JSON path {} did not exist",
                                                assertion.path
                                            ));
                                            break;
                                        }
                                        Err(error) => {
                                            is_success = false;
                                            failure_reason = Some(assertion_failure_reason(
                                                &format!("json_path_equals[{index}]"),
                                            ));
                                            error_message = Some(error);
                                            break;
                                        }
                                    }
                                }
                            }

                            if is_success {
                                for (index, assertion) in
                                    assertions.json_path_not_equals.iter().enumerate()
                                {
                                    match resolve_json_path(&json_body, &assertion.path) {
                                        Ok(Some(actual_value))
                                            if actual_value != &assertion.value => {}
                                        Ok(Some(actual_value)) => {
                                            is_success = false;
                                            failure_reason = Some(assertion_failure_reason(
                                                &format!("json_path_not_equals[{index}]"),
                                            ));
                                            error_message = Some(format!(
                                                "JSON path {} unexpectedly had value {}",
                                                assertion.path, actual_value
                                            ));
                                            break;
                                        }
                                        Ok(None) => {
                                            is_success = false;
                                            failure_reason = Some(assertion_failure_reason(
                                                &format!("json_path_not_equals[{index}]"),
                                            ));
                                            error_message = Some(format!(
                                                "JSON path {} did not exist",
                                                assertion.path
                                            ));
                                            break;
                                        }
                                        Err(error) => {
                                            is_success = false;
                                            failure_reason = Some(assertion_failure_reason(
                                                &format!("json_path_not_equals[{index}]"),
                                            ));
                                            error_message = Some(error);
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(error) => {
                        is_success = false;
                        failure_reason = Some("response_read_error".to_string());
                        error_message = Some(error.to_string());
                    }
                }
            }

            CheckResult {
                is_success,
                status_code: Some(status_code),
                response_time_ms: Some(elapsed),
                failure_reason,
                error_message,
                certificate_metadata,
            }
        }
        Err(error) => {
            let elapsed: i32 = start.elapsed().as_millis() as i32;

            CheckResult {
                is_success: false,
                status_code: None,
                response_time_ms: Some(elapsed),
                failure_reason: Some(classify_request_error(&error).to_string()),
                error_message: Some(error.to_string()),
                certificate_metadata,
            }
        }
    }
}

async fn check_ssl_certificate_internal(
    url: &str,
    warning_days: Option<i32>,
    timeout: Duration,
    validate_public_target: bool,
) -> CheckResult {
    let start = Instant::now();

    let target = match resolve_runtime_request_target(url, validate_public_target).await {
        Ok(target) => target,
        Err((failure_reason, error_message)) => {
            return CheckResult {
                is_success: false,
                status_code: None,
                response_time_ms: Some(start.elapsed().as_millis() as i32),
                failure_reason: Some(failure_reason.to_string()),
                error_message: Some(error_message),
                certificate_metadata: None,
            };
        }
    };

    match evaluate_ssl_certificate_for_target(
        &target,
        warning_days.unwrap_or(DEFAULT_SSL_EXPIRY_WARNING_DAYS),
        timeout,
    )
    .await
    {
        Ok(metadata) => CheckResult {
            is_success: true,
            status_code: None,
            response_time_ms: Some(start.elapsed().as_millis() as i32),
            failure_reason: None,
            error_message: None,
            certificate_metadata: Some(metadata),
        },
        Err(result) => CheckResult {
            response_time_ms: Some(start.elapsed().as_millis() as i32),
            ..result
        },
    }
}

async fn evaluate_ssl_certificate_for_target(
    target: &RuntimeRequestTarget,
    warning_days: i32,
    timeout: Duration,
) -> Result<CertificateMetadata, CheckResult> {
    if target.scheme != "https" {
        return Err(CheckResult {
            is_success: false,
            status_code: None,
            response_time_ms: None,
            failure_reason: Some("ssl_certificate_invalid".to_string()),
            error_message: Some("SSL certificate checks require an https target_url".to_string()),
            certificate_metadata: None,
        });
    }

    match probe_tls_certificate(target, timeout).await {
        Ok(metadata) => {
            if let Some((failure_reason, error_message)) =
                evaluate_certificate_expiry(&metadata, warning_days)
            {
                return Err(CheckResult {
                    is_success: false,
                    status_code: None,
                    response_time_ms: None,
                    failure_reason: Some(failure_reason.to_string()),
                    error_message: Some(error_message),
                    certificate_metadata: Some(metadata),
                });
            }

            Ok(metadata)
        }
        Err((failure_reason, error_message, certificate_metadata)) => Err(CheckResult {
            is_success: false,
            status_code: None,
            response_time_ms: None,
            failure_reason: Some(failure_reason.to_string()),
            error_message: Some(error_message),
            certificate_metadata,
        }),
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct RuntimeHttpClientCacheKey {
    host: String,
    resolved_addrs: Vec<SocketAddr>,
}

#[derive(Clone)]
struct RuntimeRequestTarget {
    url: reqwest::Url,
    scheme: String,
    host: String,
    port: u16,
    resolved_addrs: Vec<SocketAddr>,
}

struct RuntimeHttpClientCache {
    entries: Mutex<lru::LruCache<RuntimeHttpClientCacheKey, reqwest::Client>>,
}

impl RuntimeHttpClientCache {
    fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity).expect("cache capacity must be > 0");
        Self {
            entries: Mutex::new(lru::LruCache::new(cap)),
        }
    }

    fn get(&self, key: &RuntimeHttpClientCacheKey) -> Option<reqwest::Client> {
        self.entries
            .lock()
            .expect("runtime HTTP client cache mutex should not be poisoned")
            .get(key)
            .cloned()
    }

    fn insert(&self, key: RuntimeHttpClientCacheKey, client: reqwest::Client) -> reqwest::Client {
        let mut entries = self
            .entries
            .lock()
            .expect("runtime HTTP client cache mutex should not be poisoned");

        if let Some(existing) = entries.get(&key) {
            return existing.clone();
        }

        entries.put(key, client.clone());
        client
    }
}

fn runtime_http_client_cache() -> &'static RuntimeHttpClientCache {
    static CACHE: OnceLock<RuntimeHttpClientCache> = OnceLock::new();

    CACHE.get_or_init(|| RuntimeHttpClientCache::new(RUNTIME_HTTP_CLIENT_CACHE_CAPACITY))
}

async fn resolve_runtime_request_target(
    url: &str,
    validate_public_target: bool,
) -> Result<RuntimeRequestTarget, (&'static str, String)> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| ("request_error", "target_url is not a valid URL".to_string()))?;
    let scheme = parsed.scheme().to_string();
    match scheme.as_str() {
        "http" | "https" => {}
        scheme => {
            return Err((
                "request_error",
                format!("target_url scheme '{scheme}' is not allowed; use http or https"),
            ));
        }
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| ("request_error", "target_url must have a host".to_string()))?;
    let port = parsed.port_or_known_default().unwrap_or(80);
    let resolved = resolve_runtime_target(host, port, validate_public_target).await?;
    let host = host.to_ascii_lowercase();

    Ok(RuntimeRequestTarget {
        url: parsed,
        scheme,
        host,
        port,
        resolved_addrs: resolved,
    })
}

async fn build_runtime_client(
    target: &RuntimeRequestTarget,
) -> Result<reqwest::Client, (&'static str, String)> {
    let cache_key = RuntimeHttpClientCacheKey {
        host: target.host.clone(),
        resolved_addrs: target.resolved_addrs.clone(),
    };

    if let Some(client) = runtime_http_client_cache().get(&cache_key) {
        return Ok(client);
    }

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .resolve_to_addrs(&target.host, &target.resolved_addrs)
        .build()
        .map_err(|error| ("request_error", error.to_string()))?;

    Ok(runtime_http_client_cache().insert(cache_key, client))
}

async fn resolve_runtime_target(
    host: &str,
    port: u16,
    validate_public_target: bool,
) -> Result<Vec<SocketAddr>, (&'static str, String)> {
    let mut resolved = if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        vec![SocketAddr::new(ip, port)]
    } else {
        tokio::net::lookup_host(format!("{host}:{port}"))
            .await
            .map_err(|_| {
                (
                    "connect_error",
                    "target_url hostname could not be resolved".to_string(),
                )
            })?
            .collect::<Vec<_>>()
    };

    if resolved.is_empty() {
        return Err((
            "connect_error",
            "target_url hostname could not be resolved".to_string(),
        ));
    }

    resolved.sort_unstable();
    resolved.dedup();

    if validate_public_target {
        for addr in &resolved {
            if !net::check_ip_is_public(addr.ip()) {
                return Err((
                    "blocked_address",
                    "target_url must not resolve to a loopback, private, or link-local address"
                        .to_string(),
                ));
            }
        }
    }

    Ok(resolved)
}

fn validated_tls_client_config() -> Result<Arc<ClientConfig>, String> {
    static CONFIG: OnceLock<Result<Arc<ClientConfig>, String>> = OnceLock::new();

    match CONFIG.get_or_init(|| {
        ensure_rustls_crypto_provider_installed();
        ClientConfig::with_platform_verifier()
            .map(Arc::new)
            .map_err(|error| error.to_string())
    }) {
        Ok(config) => Ok(config.clone()),
        Err(error) => Err(error.clone()),
    }
}

fn inspection_tls_client_config() -> Arc<ClientConfig> {
    static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();

    CONFIG
        .get_or_init(|| {
            ensure_rustls_crypto_provider_installed();
            Arc::new(
                ClientConfig::builder()
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
                    .with_no_client_auth(),
            )
        })
        .clone()
}

fn ensure_rustls_crypto_provider_installed() {
    static INSTALLED: OnceLock<()> = OnceLock::new();

    let _ = INSTALLED.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

async fn probe_tls_certificate(
    target: &RuntimeRequestTarget,
    timeout: Duration,
) -> Result<CertificateMetadata, (&'static str, String, Option<CertificateMetadata>)> {
    let inspected =
        inspect_tls_certificate(target, timeout, inspection_tls_client_config()).await?;

    if let Err((failure_reason, error_message)) =
        validate_tls_certificate(target, timeout, validated_tls_client_config()).await
    {
        let failure_reason = if inspected.days_remaining < 0 {
            "ssl_certificate_expired"
        } else {
            failure_reason
        };
        return Err((failure_reason, error_message, Some(inspected)));
    }

    Ok(inspected)
}

async fn inspect_tls_certificate(
    target: &RuntimeRequestTarget,
    timeout: Duration,
    client_config: Arc<ClientConfig>,
) -> Result<CertificateMetadata, (&'static str, String, Option<CertificateMetadata>)> {
    let tls_stream = connect_tls_with_config(target, timeout, client_config)
        .await
        .map_err(|(failure_reason, error_message)| (failure_reason, error_message, None))?;
    let certificates = tls_stream.get_ref().1.peer_certificates().ok_or_else(|| {
        (
            "ssl_certificate_invalid",
            "TLS server did not present a certificate".to_string(),
            None,
        )
    })?;
    let leaf = certificates.first().ok_or_else(|| {
        (
            "ssl_certificate_invalid",
            "TLS server did not present a certificate".to_string(),
            None,
        )
    })?;

    parse_certificate_metadata(leaf, &target.host)
        .map_err(|error| ("ssl_certificate_invalid", error, None))
}

async fn validate_tls_certificate(
    target: &RuntimeRequestTarget,
    timeout: Duration,
    client_config: Result<Arc<ClientConfig>, String>,
) -> Result<(), (&'static str, String)> {
    let client_config =
        client_config.map_err(|error| ("ssl_certificate_invalid", error.to_string()))?;
    connect_tls_with_config(target, timeout, client_config)
        .await
        .map(|_| ())
}

async fn connect_tls_with_config(
    target: &RuntimeRequestTarget,
    timeout: Duration,
    client_config: Arc<ClientConfig>,
) -> Result<TlsStream<TcpStream>, (&'static str, String)> {
    let server_name = ServerName::try_from(target.host.clone()).map_err(|_| {
        (
            "ssl_certificate_invalid",
            "target_url host is not a valid TLS server name".to_string(),
        )
    })?;
    let connector = TlsConnector::from(client_config);
    let mut last_error = None;

    for addr in &target.resolved_addrs {
        let tcp_stream = match tokio_timeout(timeout, TcpStream::connect(addr)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => {
                last_error = Some(("connect_error", error.to_string()));
                continue;
            }
            Err(_) => return Err(("timeout", format!("Timed out connecting to {}", addr))),
        };

        match tokio_timeout(timeout, connector.connect(server_name.clone(), tcp_stream)).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(error)) => {
                last_error = Some(("ssl_certificate_invalid", error.to_string()));
            }
            Err(_) => {
                return Err((
                    "timeout",
                    format!("Timed out during TLS handshake to {}", addr),
                ));
            }
        }
    }

    Err(last_error.unwrap_or((
        "connect_error",
        format!(
            "Could not connect to any resolved address for {}:{}",
            target.host, target.port
        ),
    )))
}

fn evaluate_certificate_expiry(
    metadata: &CertificateMetadata,
    warning_days: i32,
) -> Option<(&'static str, String)> {
    let warning_days = warning_days.max(SSL_EXPIRY_CRITICAL_DAYS + 1);

    if metadata.days_remaining <= SSL_EXPIRY_CRITICAL_DAYS {
        return Some((
            "ssl_certificate_expiring_critical",
            format!(
                "TLS certificate for {} expires in {} days at {}",
                metadata.domain,
                metadata.days_remaining,
                metadata.expires_at.to_rfc3339()
            ),
        ));
    }

    if metadata.days_remaining <= warning_days {
        return Some((
            "ssl_certificate_expiring_warning",
            format!(
                "TLS certificate for {} expires in {} days at {}",
                metadata.domain,
                metadata.days_remaining,
                metadata.expires_at.to_rfc3339()
            ),
        ));
    }

    None
}

fn parse_certificate_metadata(
    certificate: &CertificateDer<'_>,
    fallback_host: &str,
) -> Result<CertificateMetadata, String> {
    use x509_parser::prelude::*;

    let (_, cert) = X509Certificate::from_der(certificate.as_ref())
        .map_err(|e| format!("Failed to parse TLS certificate: {e}"))?;

    let expires_at = DateTime::from_timestamp(cert.validity().not_after.timestamp(), 0)
        .ok_or_else(|| "TLS certificate expiry timestamp is out of range".to_string())?;
    let days_remaining = ((expires_at - Utc::now()).num_seconds() / 86_400) as i32;
    let subject = cert.subject().to_string();
    let issuer = cert.issuer().to_string();
    let domain = cert
        .subject_alternative_name()
        .ok()
        .flatten()
        .and_then(|ext| {
            ext.value.general_names.iter().find_map(|name| {
                if let GeneralName::DNSName(dns) = name {
                    Some((*dns).to_string())
                } else {
                    None
                }
            })
        })
        .or_else(|| {
            cert.subject()
                .iter_common_name()
                .next()
                .and_then(|attr| attr.attr_value().as_str().ok().map(str::to_string))
        })
        .unwrap_or_else(|| fallback_host.to_string());

    Ok(CertificateMetadata {
        expires_at,
        days_remaining,
        issuer,
        subject,
        domain,
    })
}

#[derive(Debug)]
struct NoCertificateVerification;

impl ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ED25519,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}

fn classify_request_error(error: &reqwest::Error) -> &'static str {
    if error.is_connect() {
        "connect_error"
    } else if error.is_timeout() {
        "timeout"
    } else if error.is_redirect() {
        "redirect_error"
    } else {
        "request_error"
    }
}

fn assertion_failure_reason(path: &str) -> String {
    format!("assertion_failed:{path}")
}

fn resolve_json_path<'a>(
    value: &'a serde_json::Value,
    path: &str,
) -> Result<Option<&'a serde_json::Value>, String> {
    let tokens = parse_json_path(path)?;
    let mut current = value;

    for token in tokens {
        match token {
            JsonPathToken::Field(field) => match current {
                serde_json::Value::Object(map) => match map.get(&field) {
                    Some(next) => current = next,
                    None => return Ok(None),
                },
                _ => return Ok(None),
            },
            JsonPathToken::Index(index) => match current {
                serde_json::Value::Array(items) => match items.get(index) {
                    Some(next) => current = next,
                    None => return Ok(None),
                },
                _ => return Ok(None),
            },
        }
    }

    Ok(Some(current))
}

#[derive(Debug)]
enum JsonPathToken {
    Field(String),
    Index(usize),
}

fn parse_json_path(path: &str) -> Result<Vec<JsonPathToken>, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("JSON path must not be blank".to_string());
    }

    let mut chars = trimmed.chars().peekable();
    let mut tokens = Vec::new();
    let mut just_closed_index = false;

    if matches!(chars.peek(), Some('$')) {
        chars.next();
        if matches!(chars.peek(), Some('.')) {
            chars.next();
        }
    }

    let mut current_field = String::new();
    while let Some(ch) = chars.next() {
        match ch {
            '.' => {
                if !current_field.is_empty() {
                    tokens.push(JsonPathToken::Field(std::mem::take(&mut current_field)));
                    just_closed_index = false;
                } else if just_closed_index {
                    just_closed_index = false;
                } else {
                    return Err(format!("Invalid JSON path: {trimmed}"));
                }
            }
            '[' => {
                if !current_field.is_empty() {
                    tokens.push(JsonPathToken::Field(std::mem::take(&mut current_field)));
                }

                let mut index = String::new();
                loop {
                    match chars.next() {
                        Some(']') => break,
                        Some(c) if c.is_ascii_digit() => index.push(c),
                        _ => return Err(format!("Invalid JSON path: {trimmed}")),
                    }
                }

                if index.is_empty() {
                    return Err(format!("Invalid JSON path: {trimmed}"));
                }

                let index = index
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid JSON path: {trimmed}"))?;
                tokens.push(JsonPathToken::Index(index));
                just_closed_index = true;
            }
            c => {
                current_field.push(c);
                just_closed_index = false;
            }
        }
    }

    if !current_field.is_empty() {
        tokens.push(JsonPathToken::Field(current_field));
    }

    if tokens.is_empty() {
        return Err(format!("Invalid JSON path: {trimmed}"));
    }

    Ok(tokens)
}

fn evaluate_header_assertion(
    headers: &reqwest::header::HeaderMap,
    assertion: &HttpHeaderAssertion,
    path: &str,
) -> Result<(), (String, String)> {
    let header_name =
        reqwest::header::HeaderName::from_bytes(assertion.name.as_bytes()).map_err(|_| {
            (
                assertion_failure_reason(path),
                format!("{} is not a valid HTTP header name", assertion.name),
            )
        })?;

    let Some(header_value) = headers.get(&header_name) else {
        return Err((
            assertion_failure_reason(&format!("{path}.exists")),
            format!("Response did not include required header: {}", header_name),
        ));
    };

    if let Some(required_value) = assertion.equals.as_deref() {
        match header_value.to_str() {
            Ok(actual_value) if actual_value == required_value => {}
            Ok(actual_value) => {
                return Err((
                    assertion_failure_reason(&format!("{path}.equals")),
                    format!(
                        "Response header {} had value {:?}, expected {:?}",
                        header_name, actual_value, required_value
                    ),
                ));
            }
            Err(_) => {
                return Err((
                    assertion_failure_reason(&format!("{path}.equals")),
                    format!("Response header {} was not valid text", header_name),
                ));
            }
        }
    }

    if let Some(required_substring) = assertion.contains.as_deref() {
        match header_value.to_str() {
            Ok(actual_value) if actual_value.contains(required_substring) => {}
            Ok(actual_value) => {
                return Err((
                    assertion_failure_reason(&format!("{path}.contains")),
                    format!(
                        "Response header {} had value {:?}, expected it to contain {:?}",
                        header_name, actual_value, required_substring
                    ),
                ));
            }
            Err(_) => {
                return Err((
                    assertion_failure_reason(&format!("{path}.contains")),
                    format!("Response header {} was not valid text", header_name),
                ));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CertificateMetadata, DEFAULT_SSL_EXPIRY_WARNING_DAYS, HttpAssertions, HttpCheckConfig,
        RuntimeRequestTarget, SSL_EXPIRY_CRITICAL_DAYS, check_url_internal,
        evaluate_certificate_expiry, inspect_tls_certificate, inspection_tls_client_config,
        validate_tls_certificate,
    };
    use crate::domain::site_monitors::{HttpHeaderAssertion, JsonPathValueAssertion};
    use anyhow::Result;
    use chrono::{Datelike, Utc};
    use rustls::{
        ClientConfig, RootCertStore, ServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    };
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;

    const TEST_TLS_CHAIN_PEM: &str = "-----BEGIN CERTIFICATE-----\nMIIBszCCAVmgAwIBAgIUUg3keFcU1xXWK8BNVb1KynPulV8wCgYIKoZIzj0EAwIw\nJjEkMCIGA1UEAwwbUnVzdGxzIFJvYnVzdCBSb290IC0gUnVuZyAyMCAXDTc1MDEw\nMTAwMDAwMFoYDzQwOTYwMTAxMDAwMDAwWjAhMR8wHQYDVQQDDBZyY2dlbiBzZWxm\nIHNpZ25lZCBjZXJ0MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEud6w4gtZ0xbw\nJ3E69SSMy5TZfdIifl9L5ZY+hgEe4UiUsBWS32f6Y5NR5Jo8FO1f6o13b3+FvVHR\nEHCGdvppL6NoMGYwFQYDVR0RBA4wDIIKZm9vYmFyLmNvbTAdBgNVHSUEFjAUBggr\nBgEFBQcDAQYIKwYBBQUHAwIwHQYDVR0OBBYEFELvxbj5tD75n4pYFvJyr+c8qVEi\nMA8GA1UdEwEB/wQFMAMBAQAwCgYIKoZIzj0EAwIDSAAwRQIhALxSSdUsrRFnwNMu\n/doBqI8i8u5HdohVAheFTDwObkOMAiASSjULUtkWSD15u/7Sr01Wm9J1MpqW1pob\nBVqU3CNRlA==\n-----END CERTIFICATE-----\n-----BEGIN CERTIFICATE-----\nMIIBiTCCATCgAwIBAgIUHWiVYIvMMWoZEFYvSz46COf2FqowCgYIKoZIzj0EAwIw\nHTEbMBkGA1UEAwwSUnVzdGxzIFJvYnVzdCBSb290MCAXDTc1MDEwMTAwMDAwMFoY\nDzQwOTYwMTAxMDAwMDAwWjAmMSQwIgYDVQQDDBtSdXN0bHMgUm9idXN0IFJvb3Qg\nLSBSdW5nIDIwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAATAOCcBD7dXjmAZ3te5\nD47cCJ9ec93PWv7BKYIL826CJsKfXQOGrBTthLm77hXLhHu6uv8E5QXNLZpfowLQ\nDo1ao0MwQTAPBgNVHQ8BAf8EBQMDB4QAMB0GA1UdDgQWBBRdza76r11Ok9vRmlg6\nNn/wL/N+jTAPBgNVHRMBAf8EBTADAQH/MAoGCCqGSM49BAMCA0cAMEQCIFmZrXeK\nhnfkahocvkhhNT3cDv1LWf6WBoFaCiBwZXFPAiARaKRiSCMG7PCHmSqFe82TBVmL\nodHGogAVax1Dh/aYAA==\n-----END CERTIFICATE-----\n";
    const TEST_TLS_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgTbAQpfjAT46fgF4B\nmP15n37woNG5ZNJmwcqsred/7tmhRANCAAS53rDiC1nTFvAncTr1JIzLlNl90iJ+\nX0vllj6GAR7hSJSwFZLfZ/pjk1HkmjwU7V/qjXdvf4W9UdEQcIZ2+mkv\n-----END PRIVATE KEY-----\n";
    const TEST_TLS_ROOT_PEM: &str = "-----BEGIN CERTIFICATE-----\nMIIBgDCCASegAwIBAgIUPHDUu9WL36yvTmFeNFZVe/qhClcwCgYIKoZIzj0EAwIw\nHTEbMBkGA1UEAwwSUnVzdGxzIFJvYnVzdCBSb290MCAXDTc1MDEwMTAwMDAwMFoY\nDzQwOTYwMTAxMDAwMDAwWjAdMRswGQYDVQQDDBJSdXN0bHMgUm9idXN0IFJvb3Qw\nWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAASW/VkDFs5iGDQvH8jaXYT4jMx66jo+\n5CWKyMt4OlTDdBfKfnmQ9LYeK/PsYfJ8wVizuSlPzXi9je8SnyYejGP3o0MwQTAP\nBgNVHQ8BAf8EBQMDB4QAMB0GA1UdDgQWBBRqY/oMENJbNo7y39iL6GW3tDs0rzAP\nBgNVHRMBAf8EBTADAQH/MAoGCCqGSM49BAMCA0cAMEQCIEUbrmSUjANju9nNpFop\nPAl9Wh8tBxI5IY+BPh466+aUAiA1/9+prypt6s3Doo0GDsnoFGJi1UBivUg1qdik\ncy4eNw==\n-----END CERTIFICATE-----\n";

    #[tokio::test]
    async fn check_url_returns_success_when_status_matches_expected() -> Result<()> {
        let url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
        )
        .await?;

        let result = check_url_internal(
            &url,
            200,
            HttpAssertions::default(),
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(result.is_success);
        assert_eq!(result.status_code, Some(200));
        assert!(result.response_time_ms.is_some());
        assert_eq!(result.failure_reason, None);
        assert_eq!(result.error_message, None);

        Ok(())
    }

    #[tokio::test]
    async fn check_url_returns_failure_when_status_differs_from_expected() -> Result<()> {
        let url = spawn_http_server(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 4\r\nConnection: close\r\n\r\nDown",
        )
        .await?;

        let result = check_url_internal(
            &url,
            200,
            HttpAssertions::default(),
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(!result.is_success);
        assert_eq!(result.status_code, Some(503));
        assert_eq!(result.failure_reason.as_deref(), Some("unexpected_status"));
        assert_eq!(
            result.error_message.as_deref(),
            Some("Unexpected HTTP status: 503, expected: 200")
        );

        Ok(())
    }

    #[tokio::test]
    async fn check_url_classifies_redirects_separately() -> Result<()> {
        let url = spawn_http_server(
            "HTTP/1.1 302 Found\r\nLocation: /login\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await?;

        let result = check_url_internal(
            &url,
            200,
            HttpAssertions::default(),
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(!result.is_success);
        assert_eq!(result.status_code, Some(302));
        assert_eq!(result.failure_reason.as_deref(), Some("redirect_blocked"));
        assert_eq!(
            result.error_message.as_deref(),
            Some("Redirect blocked with HTTP status: 302, location: /login")
        );

        Ok(())
    }

    #[tokio::test]
    async fn check_url_classifies_timeouts_and_preserves_elapsed_time() -> Result<()> {
        let url = spawn_hanging_http_server().await?;

        let result = check_url_internal(
            &url,
            200,
            HttpAssertions::default(),
            Duration::from_millis(50),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(!result.is_success);
        assert_eq!(result.status_code, None);
        assert_eq!(result.failure_reason.as_deref(), Some("timeout"));
        assert!(result.response_time_ms.is_some());
        assert!(result.error_message.is_some());

        Ok(())
    }

    #[tokio::test]
    async fn check_url_blocks_loopback_endpoints_before_request() -> Result<()> {
        let result = check_url_internal(
            "http://127.0.0.1:1",
            200,
            HttpAssertions::default(),
            Duration::from_secs(2),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: true,
            },
        )
        .await;

        assert!(!result.is_success);
        assert_eq!(result.status_code, None);
        assert_eq!(result.failure_reason.as_deref(), Some("blocked_address"));
        assert!(result.response_time_ms.is_some());
        assert!(result.error_message.is_some());

        Ok(())
    }

    #[tokio::test]
    async fn inspect_tls_certificate_extracts_metadata_and_validates_chain() -> Result<()> {
        let port = spawn_tls_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
        )
        .await?;
        let target = build_test_tls_target(port);

        let metadata = inspect_tls_certificate(
            &target,
            Duration::from_secs(5),
            inspection_tls_client_config(),
        )
        .await
        .expect("inspection handshake should succeed");
        assert_eq!(metadata.domain, "foobar.com");
        assert!(metadata.subject.contains("rcgen self signed cert"));
        assert!(metadata.issuer.contains("Rustls Robust Root - Rung 2"));
        assert_eq!(metadata.expires_at.year(), 4096);
        assert!(metadata.days_remaining > 1000);

        validate_tls_certificate(
            &target,
            Duration::from_secs(5),
            Ok(build_test_tls_client_config()?),
        )
        .await
        .expect("validated handshake should succeed");

        Ok(())
    }

    #[test]
    fn evaluate_certificate_expiry_distinguishes_warning_and_critical() {
        let base_metadata = CertificateMetadata {
            expires_at: Utc::now(),
            days_remaining: DEFAULT_SSL_EXPIRY_WARNING_DAYS,
            issuer: "issuer".to_string(),
            subject: "subject".to_string(),
            domain: "foobar.com".to_string(),
        };

        let warning = evaluate_certificate_expiry(&base_metadata, DEFAULT_SSL_EXPIRY_WARNING_DAYS)
            .expect("warning threshold should trigger");
        assert_eq!(warning.0, "ssl_certificate_expiring_warning");

        let critical = evaluate_certificate_expiry(
            &CertificateMetadata {
                days_remaining: SSL_EXPIRY_CRITICAL_DAYS,
                ..base_metadata
            },
            DEFAULT_SSL_EXPIRY_WARNING_DAYS,
        )
        .expect("critical threshold should trigger");
        assert_eq!(critical.0, "ssl_certificate_expiring_critical");
    }

    #[tokio::test]
    async fn check_url_reuses_pooled_client_for_same_target() -> Result<()> {
        let (url, accepted_connections) = spawn_keep_alive_http_server().await?;

        let first = check_url_internal(
            &url,
            200,
            HttpAssertions::default(),
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;
        let second = check_url_internal(
            &url,
            200,
            HttpAssertions::default(),
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(first.is_success);
        assert!(second.is_success);
        assert_eq!(accepted_connections.load(Ordering::Relaxed), 1);

        Ok(())
    }

    #[tokio::test]
    async fn check_url_requires_response_body_text_when_configured() -> Result<()> {
        let matching_url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nhello world",
        )
        .await?;

        let result = check_url_internal(
            &matching_url,
            200,
            HttpAssertions {
                body_must_contain: Some("world"),
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(result.is_success);

        let missing_url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nhello world",
        )
        .await?;

        let result = check_url_internal(
            &missing_url,
            200,
            HttpAssertions {
                body_must_contain: Some("missing"),
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(!result.is_success);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("assertion_failed:body_must_contain")
        );

        Ok(())
    }

    #[tokio::test]
    async fn check_url_rejects_forbidden_response_body_text_when_configured() -> Result<()> {
        let url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nhello world",
        )
        .await?;

        let result = check_url_internal(
            &url,
            200,
            HttpAssertions {
                body_must_not_contain: Some("world"),
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(!result.is_success);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("assertion_failed:body_must_not_contain")
        );

        Ok(())
    }

    #[tokio::test]
    async fn check_url_requires_all_response_body_texts_when_configured() -> Result<()> {
        let matching_url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 25\r\nConnection: close\r\n\r\nservice healthy and ready",
        )
        .await?;
        let required_texts = vec!["healthy".to_string(), "ready".to_string()];

        let result = check_url_internal(
            &matching_url,
            200,
            HttpAssertions {
                body_must_contain_texts: &required_texts,
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(result.is_success);

        let missing_url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 15\r\nConnection: close\r\n\r\nservice healthy",
        )
        .await?;

        let result = check_url_internal(
            &missing_url,
            200,
            HttpAssertions {
                body_must_contain_texts: &required_texts,
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(!result.is_success);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("assertion_failed:body_must_contain_texts[1]")
        );
        assert_eq!(
            result.error_message.as_deref(),
            Some("Response body did not contain required text: ready")
        );

        Ok(())
    }

    #[tokio::test]
    async fn check_url_rejects_any_forbidden_response_body_text_when_configured() -> Result<()> {
        let url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 25\r\nConnection: close\r\n\r\nservice healthy but error",
        )
        .await?;
        let forbidden_texts = vec!["panic".to_string(), "error".to_string()];

        let result = check_url_internal(
            &url,
            200,
            HttpAssertions {
                body_must_not_contain_texts: &forbidden_texts,
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(!result.is_success);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("assertion_failed:body_must_not_contain_texts[1]")
        );
        assert_eq!(
            result.error_message.as_deref(),
            Some("Response body contained forbidden text: error")
        );

        Ok(())
    }

    #[tokio::test]
    async fn check_url_validates_json_path_exists_and_equals_assertions() -> Result<()> {
        let url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 56\r\nConnection: close\r\n\r\n{\"status\":\"ok\",\"checks\":[{\"healthy\":true,\"name\":\"api\"}]}",
        )
        .await?;
        let json_path_exists = vec!["$.status".to_string(), "$.checks[0].healthy".to_string()];
        let json_path_equals = vec![JsonPathValueAssertion {
            path: "$.status".to_string(),
            value: serde_json::json!("ok"),
        }];

        let result = check_url_internal(
            &url,
            200,
            HttpAssertions {
                json_path_exists: &json_path_exists,
                json_path_equals: &json_path_equals,
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(result.is_success);

        Ok(())
    }

    #[tokio::test]
    async fn check_url_rejects_missing_json_path() -> Result<()> {
        let url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"status\":\"ok\"}",
        )
        .await?;
        let json_path_exists = vec!["$.checks[0].healthy".to_string()];

        let result = check_url_internal(
            &url,
            200,
            HttpAssertions {
                json_path_exists: &json_path_exists,
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(!result.is_success);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("assertion_failed:json_path_exists[0]")
        );

        Ok(())
    }

    #[tokio::test]
    async fn check_url_rejects_unexpected_json_path_value() -> Result<()> {
        let url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 17\r\nConnection: close\r\n\r\n{\"status\":\"down\"}",
        )
        .await?;
        let json_path_equals = vec![JsonPathValueAssertion {
            path: "$.status".to_string(),
            value: serde_json::json!("ok"),
        }];

        let result = check_url_internal(
            &url,
            200,
            HttpAssertions {
                json_path_equals: &json_path_equals,
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(!result.is_success);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("assertion_failed:json_path_equals[0]")
        );

        Ok(())
    }

    #[tokio::test]
    async fn check_url_rejects_disallowed_json_path_value() -> Result<()> {
        let url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"status\":\"ok\"}",
        )
        .await?;
        let json_path_not_equals = vec![JsonPathValueAssertion {
            path: "$.status".to_string(),
            value: serde_json::json!("ok"),
        }];

        let result = check_url_internal(
            &url,
            200,
            HttpAssertions {
                json_path_not_equals: &json_path_not_equals,
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(!result.is_success);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("assertion_failed:json_path_not_equals[0]")
        );

        Ok(())
    }

    #[tokio::test]
    async fn check_url_requires_response_header_when_configured() -> Result<()> {
        let present_url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nX-Health: healthy\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
        )
        .await?;

        let result = check_url_internal(
            &present_url,
            200,
            HttpAssertions {
                required_header_name: Some("x-health"),
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(result.is_success);

        let missing_url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
        )
        .await?;

        let result = check_url_internal(
            &missing_url,
            200,
            HttpAssertions {
                required_header_name: Some("x-health"),
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(!result.is_success);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("assertion_failed:required_header.exists")
        );

        Ok(())
    }

    #[tokio::test]
    async fn check_url_requires_exact_response_header_value_when_configured() -> Result<()> {
        let url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nX-Health: healthy\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
        )
        .await?;

        let result = check_url_internal(
            &url,
            200,
            HttpAssertions {
                required_header_name: Some("x-health"),
                required_header_value: Some("healthy"),
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(result.is_success);

        let mismatch_url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nX-Health: degraded\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
        )
        .await?;

        let result = check_url_internal(
            &mismatch_url,
            200,
            HttpAssertions {
                required_header_name: Some("x-health"),
                required_header_value: Some("healthy"),
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(!result.is_success);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("assertion_failed:required_header.equals")
        );

        Ok(())
    }

    #[tokio::test]
    async fn check_url_supports_multiple_header_assertions() -> Result<()> {
        let url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nX-Health: healthy\r\nCache-Control: no-store, max-age=0\r\nX-Request-Id: abc123\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
        )
        .await?;
        let header_assertions = vec![
            HttpHeaderAssertion {
                name: "x-request-id".to_string(),
                equals: None,
                contains: None,
            },
            HttpHeaderAssertion {
                name: "x-health".to_string(),
                equals: Some("healthy".to_string()),
                contains: None,
            },
            HttpHeaderAssertion {
                name: "cache-control".to_string(),
                equals: None,
                contains: Some("no-store".to_string()),
            },
        ];

        let result = check_url_internal(
            &url,
            200,
            HttpAssertions {
                header_assertions: &header_assertions,
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(result.is_success);

        Ok(())
    }

    #[tokio::test]
    async fn check_url_rejects_header_when_required_substring_is_missing() -> Result<()> {
        let url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nCache-Control: public, max-age=60\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
        )
        .await?;
        let header_assertions = vec![HttpHeaderAssertion {
            name: "cache-control".to_string(),
            equals: None,
            contains: Some("no-store".to_string()),
        }];

        let result = check_url_internal(
            &url,
            200,
            HttpAssertions {
                header_assertions: &header_assertions,
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(!result.is_success);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("assertion_failed:header_assertions[0].contains")
        );

        Ok(())
    }

    #[test]
    fn evaluate_certificate_expiry_returns_none_when_healthy() {
        let metadata = CertificateMetadata {
            expires_at: Utc::now(),
            days_remaining: 30, // well above warning threshold
            issuer: "issuer".to_string(),
            subject: "subject".to_string(),
            domain: "example.com".to_string(),
        };
        assert!(
            evaluate_certificate_expiry(&metadata, DEFAULT_SSL_EXPIRY_WARNING_DAYS).is_none(),
            "healthy cert should not trigger any alert"
        );
    }

    #[test]
    fn evaluate_certificate_expiry_triggers_critical_for_expired_cert() {
        let metadata = CertificateMetadata {
            expires_at: Utc::now(),
            days_remaining: 0,
            issuer: "issuer".to_string(),
            subject: "subject".to_string(),
            domain: "example.com".to_string(),
        };
        let result = evaluate_certificate_expiry(&metadata, DEFAULT_SSL_EXPIRY_WARNING_DAYS)
            .expect("expired cert must trigger alert");
        assert_eq!(result.0, "ssl_certificate_expiring_critical");
    }

    #[test]
    fn evaluate_certificate_expiry_triggers_warning_between_thresholds() {
        // 10 days: above critical (7) but below warning (14) → warning
        let metadata = CertificateMetadata {
            expires_at: Utc::now(),
            days_remaining: 10,
            issuer: "issuer".to_string(),
            subject: "subject".to_string(),
            domain: "example.com".to_string(),
        };
        let result = evaluate_certificate_expiry(&metadata, DEFAULT_SSL_EXPIRY_WARNING_DAYS)
            .expect("should trigger warning");
        assert_eq!(result.0, "ssl_certificate_expiring_warning");
    }

    #[test]
    fn evaluate_certificate_expiry_clamps_warning_threshold_above_critical() {
        // warning_days=1 is clamped to critical+1=8; days_remaining=8 → warning
        let metadata = CertificateMetadata {
            expires_at: Utc::now(),
            days_remaining: 8,
            issuer: "issuer".to_string(),
            subject: "subject".to_string(),
            domain: "example.com".to_string(),
        };
        let result = evaluate_certificate_expiry(&metadata, 1)
            .expect("clamped warning_days should still alert at day 8");
        assert_eq!(result.0, "ssl_certificate_expiring_warning");
    }

    #[tokio::test]
    async fn check_url_succeeds_when_redirect_status_code_is_expected() -> Result<()> {
        let url = spawn_http_server(
            "HTTP/1.1 302 Found\r\nLocation: /login\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await?;

        let result = check_url_internal(
            &url,
            302,
            HttpAssertions::default(),
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(result.is_success);
        assert_eq!(result.status_code, Some(302));
        assert_eq!(result.failure_reason, None);

        Ok(())
    }

    #[tokio::test]
    async fn check_url_enforces_max_response_time_assertion() -> Result<()> {
        let url = spawn_delayed_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
            Duration::from_millis(20),
        )
        .await?;

        let result = check_url_internal(
            &url,
            200,
            HttpAssertions {
                max_response_time_ms: Some(1),
                ..HttpAssertions::default()
            },
            Duration::from_secs(10),
            HttpCheckConfig {
                max_response_body_bytes: 64 * 1024,
                validate_public_target: false,
            },
        )
        .await;

        assert!(!result.is_success);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("assertion_failed:max_response_time_ms")
        );

        Ok(())
    }

    async fn spawn_http_server(response: &'static str) -> Result<String> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept test connection");
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer).await;
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write test response");
        });

        Ok(format!("http://{}", address))
    }

    async fn spawn_hanging_http_server() -> Result<String> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;

        tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.expect("accept hanging connection");
            tokio::time::sleep(Duration::from_secs(1)).await;
        });

        Ok(format!("http://{}", address))
    }

    async fn spawn_delayed_http_server(response: &'static str, delay: Duration) -> Result<String> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept delayed connection");
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer).await;
            tokio::time::sleep(delay).await;
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write delayed response");
        });

        Ok(format!("http://{}", address))
    }

    async fn spawn_keep_alive_http_server() -> Result<(String, Arc<AtomicUsize>)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let accepted_connections = Arc::new(AtomicUsize::new(0));
        let accepted_connections_task = accepted_connections.clone();

        tokio::spawn(async move {
            let mut processed_requests = 0usize;

            while processed_requests < 2 {
                let (mut stream, _) = listener
                    .accept()
                    .await
                    .expect("accept keep-alive test connection");
                accepted_connections_task.fetch_add(1, Ordering::Relaxed);

                loop {
                    if read_http_headers(&mut stream).await.is_err() {
                        break;
                    }

                    processed_requests += 1;
                    let response = if processed_requests < 2 {
                        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\nOK"
                    } else {
                        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK"
                    };
                    stream
                        .write_all(response.as_bytes())
                        .await
                        .expect("write keep-alive test response");

                    if processed_requests >= 2 {
                        break;
                    }

                    let next_request = tokio::time::timeout(
                        Duration::from_millis(200),
                        read_http_headers(&mut stream),
                    )
                    .await;
                    match next_request {
                        Ok(Ok(())) => {
                            processed_requests += 1;
                            stream
                                .write_all(
                                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
                                )
                                .await
                                .expect("write second keep-alive test response");
                            break;
                        }
                        _ => break,
                    }
                }
            }
        });

        Ok((format!("http://{}", address), accepted_connections))
    }

    async fn spawn_tls_http_server(response: &'static str) -> Result<u16> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let acceptor = TlsAcceptor::from(build_test_tls_server_config()?);

        tokio::spawn(async move {
            for _ in 0..2 {
                let (stream, _) = listener.accept().await.expect("accept tls connection");
                let mut stream = acceptor
                    .accept(stream)
                    .await
                    .expect("complete tls handshake");
                let mut buffer = [0_u8; 1024];
                let _ = stream.read(&mut buffer).await;
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });

        Ok(port)
    }

    fn build_test_tls_target(port: u16) -> RuntimeRequestTarget {
        let url = reqwest::Url::parse(&format!("https://foobar.com:{port}/health"))
            .expect("valid https url");

        RuntimeRequestTarget {
            url,
            scheme: "https".to_string(),
            host: "foobar.com".to_string(),
            port,
            resolved_addrs: vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)],
        }
    }

    fn build_test_tls_server_config() -> Result<Arc<ServerConfig>> {
        super::ensure_rustls_crypto_provider_installed();
        let certificates = CertificateDer::pem_slice_iter(TEST_TLS_CHAIN_PEM.as_bytes())
            .collect::<Result<Vec<_>, _>>()?;
        let private_key = PrivateKeyDer::from_pem_slice(TEST_TLS_KEY_PEM.as_bytes())?.clone_key();

        Ok(Arc::new(
            ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certificates, private_key)?,
        ))
    }

    fn build_test_tls_client_config() -> Result<Arc<ClientConfig>> {
        super::ensure_rustls_crypto_provider_installed();
        let mut roots = RootCertStore::empty();
        let (added, ignored) = roots.add_parsable_certificates(
            CertificateDer::pem_slice_iter(TEST_TLS_ROOT_PEM.as_bytes()).flatten(),
        );
        assert_eq!(ignored, 0);
        assert_eq!(added, 1);

        Ok(Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        ))
    }

    async fn read_http_headers(stream: &mut tokio::net::TcpStream) -> Result<()> {
        let mut buffer = Vec::with_capacity(1024);
        let mut chunk = [0_u8; 256];

        loop {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                anyhow::bail!("connection closed before headers were fully read");
            }
            buffer.extend_from_slice(&chunk[..read]);
            if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                return Ok(());
            }
        }
    }
}
