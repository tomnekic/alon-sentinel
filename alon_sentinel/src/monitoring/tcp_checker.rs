use std::time::{Duration, Instant};

use tokio::net::TcpStream;

use crate::{monitoring::http_checker::CheckResult, net};

pub async fn check_tcp(
    host: &str,
    port: u16,
    connect_timeout: Duration,
    validate_public: bool,
) -> CheckResult {
    let start = Instant::now();

    let addrs: Vec<_> = match tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::lookup_host(format!("{host}:{port}")),
    )
    .await
    {
        Ok(Ok(addrs)) => addrs.collect(),
        Ok(Err(_)) => {
            return connect_failure("dns_error", "hostname could not be resolved", start);
        }
        Err(_) => {
            return connect_failure("dns_error", "hostname DNS lookup timed out", start);
        }
    };

    if addrs.is_empty() {
        return connect_failure("dns_error", "hostname resolved to no addresses", start);
    }

    if validate_public {
        for addr in &addrs {
            if !net::check_ip_is_public(addr.ip()) {
                return connect_failure(
                    "ssrf_blocked",
                    &format!("target resolved to non-public IP {}", addr.ip()),
                    start,
                );
            }
        }
    }

    let addr = addrs[0];

    match tokio::time::timeout(connect_timeout, TcpStream::connect(addr)).await {
        Ok(Ok(_)) => CheckResult {
            is_success: true,
            status_code: None,
            response_time_ms: Some(start.elapsed().as_millis() as i32),
            failure_reason: None,
            error_message: None,
            certificate_metadata: None,
        },
        Ok(Err(e)) => connect_failure("connect_error", &e.to_string(), start),
        Err(_) => connect_failure(
            "connect_timeout",
            &format!(
                "connection timed out after {}ms",
                connect_timeout.as_millis()
            ),
            start,
        ),
    }
}

fn connect_failure(failure_reason: &str, error_message: &str, start: Instant) -> CheckResult {
    CheckResult {
        is_success: false,
        status_code: None,
        response_time_ms: Some(start.elapsed().as_millis() as i32),
        failure_reason: Some(failure_reason.to_string()),
        error_message: Some(error_message.to_string()),
        certificate_metadata: None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::net::TcpListener;

    use super::check_tcp;

    #[tokio::test]
    async fn check_tcp_succeeds_when_port_is_open() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let result = check_tcp("127.0.0.1", port, Duration::from_secs(5), false).await;
        assert!(result.is_success);
        assert!(result.response_time_ms.is_some());
    }

    #[tokio::test]
    async fn check_tcp_fails_when_connection_refused() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let result = check_tcp("127.0.0.1", port, Duration::from_secs(5), false).await;
        assert!(!result.is_success);
        assert_eq!(result.failure_reason.as_deref(), Some("connect_error"));
    }

    #[tokio::test]
    async fn check_tcp_blocks_private_ips() {
        let result = check_tcp("127.0.0.1", 80, Duration::from_secs(5), true).await;
        assert!(!result.is_success);
        assert_eq!(result.failure_reason.as_deref(), Some("ssrf_blocked"));
    }

    #[tokio::test]
    async fn check_tcp_fails_for_unresolvable_hostname() {
        let result = check_tcp(
            "this-host-does-not-exist-alon-sentinel.example",
            80,
            Duration::from_secs(5),
            false,
        )
        .await;
        assert!(!result.is_success);
        assert_eq!(result.failure_reason.as_deref(), Some("dns_error"));
        assert!(result.response_time_ms.is_some());
    }
}
