use std::time::Duration;

use rand::RngCore;

use crate::{api::error::ApiError, net};

pub(super) mod dns;
pub(super) mod heartbeat;
pub(super) mod http;
pub(super) mod ssl;
pub(super) mod tcp;

pub(super) use heartbeat::HeartbeatSiteMonitorResponse;
pub(super) use http::HttpSiteMonitorResponse;
pub(super) use ssl::SslSiteMonitorResponse;

pub(super) async fn validate_monitor_target_url(
    target_url: &str,
    allow_private: bool,
) -> Result<(), ApiError> {
    let url = reqwest::Url::parse(target_url)
        .map_err(|_| ApiError::bad_request("target_url is not a valid URL"))?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(ApiError::bad_request(format!(
                "target_url scheme '{scheme}' is not allowed; use http or https"
            )));
        }
    }
    let host = url
        .host_str()
        .ok_or_else(|| ApiError::bad_request("target_url must have a host"))?;
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if !allow_private && !net::check_ip_is_public(ip) {
            return Err(ApiError::bad_request(
                "target_url must not resolve to a loopback, private, or link-local address",
            ));
        }
        return Ok(());
    }
    let port = url.port_or_known_default().unwrap_or(80);
    let resolved = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::lookup_host(format!("{host}:{port}")),
    )
    .await
    .map_err(|_| ApiError::bad_request("target_url hostname could not be resolved"))?
    .map_err(|_| ApiError::bad_request("target_url hostname could not be resolved"))?;
    for addr in resolved {
        if !allow_private && !net::check_ip_is_public(addr.ip()) {
            return Err(ApiError::bad_request(
                "target_url must not resolve to a loopback, private, or link-local address",
            ));
        }
    }
    Ok(())
}

pub(super) fn generate_heartbeat_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}
