use std::{
    net::IpAddr,
    str::FromStr,
    time::{Duration, Instant},
};

use hickory_resolver::{
    TokioResolver,
    config::{ConnectionConfig, NameServerConfig, ResolverConfig, ResolverOpts},
    net::runtime::TokioRuntimeProvider,
    proto::rr::{RData, RecordType},
};

use crate::{monitoring::http_checker::CheckResult, net};

pub struct DnsCheckParams<'a> {
    pub hostname: &'a str,
    pub record_type: &'a str,
    pub expected_value: Option<&'a str>,
    pub nameserver: Option<&'a str>,
}

pub async fn check_dns(
    p: &DnsCheckParams<'_>,
    query_timeout: Duration,
    validate_public: bool,
) -> CheckResult {
    let start = Instant::now();

    let record_type = match parse_record_type(p.record_type) {
        Ok(rt) => rt,
        Err(e) => return dns_failure("config_error", &e, start),
    };

    let resolver = match build_resolver(p.nameserver, query_timeout, validate_public) {
        Ok(r) => r,
        Err(e) => return dns_failure("config_error", &e, start),
    };

    let lookup = match resolver.lookup(p.hostname, record_type).await {
        Ok(l) => l,
        Err(e) => return dns_failure("lookup_error", &e.to_string(), start),
    };

    let records: Vec<String> = lookup
        .answers()
        .iter()
        .filter_map(|r| rdata_to_string(&r.data))
        .collect();

    if records.is_empty() {
        return dns_failure(
            "no_records",
            &format!("no {} records found for {}", p.record_type, p.hostname),
            start,
        );
    }

    if let Some(expected) = p.expected_value
        && !records.iter().any(|r| r == expected)
    {
        return dns_failure(
            "value_mismatch",
            &format!(
                "expected {} record '{}', got: {}",
                p.record_type,
                expected,
                records.join(", ")
            ),
            start,
        );
    }

    CheckResult {
        is_success: true,
        status_code: None,
        response_time_ms: Some(start.elapsed().as_millis() as i32),
        failure_reason: None,
        error_message: None,
        certificate_metadata: None,
    }
}

fn build_resolver(
    nameserver: Option<&str>,
    timeout: Duration,
    validate_public: bool,
) -> Result<TokioResolver, String> {
    let mut opts = ResolverOpts::default();
    opts.timeout = timeout;
    opts.attempts = 1;

    match nameserver {
        None => TokioResolver::builder_tokio()
            .map_err(|e| e.to_string())?
            .with_options(opts)
            .build()
            .map_err(|e| e.to_string()),
        Some(ns) => {
            let ip = parse_nameserver_ip(ns)?;
            if validate_public && !net::check_ip_is_public(ip) {
                return Err(format!("nameserver {ip} is not a public IP address"));
            }
            let config = ResolverConfig::from_parts(
                None,
                vec![],
                vec![NameServerConfig::new(
                    ip,
                    false,
                    vec![ConnectionConfig::udp()],
                )],
            );
            TokioResolver::builder_with_config(config, TokioRuntimeProvider::default())
                .with_options(opts)
                .build()
                .map_err(|e| e.to_string())
        }
    }
}

fn parse_nameserver_ip(nameserver: &str) -> Result<IpAddr, String> {
    let addr = nameserver.trim();
    IpAddr::from_str(addr).map_err(|_| format!("nameserver must be an IP address, got: {addr}"))
}

fn parse_record_type(s: &str) -> Result<RecordType, String> {
    match s.to_uppercase().as_str() {
        "A" => Ok(RecordType::A),
        "AAAA" => Ok(RecordType::AAAA),
        "CNAME" => Ok(RecordType::CNAME),
        "MX" => Ok(RecordType::MX),
        "TXT" => Ok(RecordType::TXT),
        "NS" => Ok(RecordType::NS),
        other => Err(format!(
            "unsupported record type '{other}'; supported: A, AAAA, CNAME, MX, TXT, NS"
        )),
    }
}

fn rdata_to_string(rdata: &RData) -> Option<String> {
    match rdata {
        RData::A(ip) => Some(ip.to_string()),
        RData::AAAA(ip) => Some(ip.to_string()),
        RData::CNAME(name) => Some(name.to_string().trim_end_matches('.').to_string()),
        RData::NS(name) => Some(name.to_string().trim_end_matches('.').to_string()),
        RData::MX(mx) => Some(format!(
            "{} {}",
            mx.preference,
            mx.exchange.to_string().trim_end_matches('.')
        )),
        RData::TXT(txt) => Some(
            txt.txt_data
                .iter()
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                .collect::<Vec<_>>()
                .join(""),
        ),
        _ => None,
    }
}

fn dns_failure(failure_reason: &str, error_message: &str, start: Instant) -> CheckResult {
    CheckResult {
        is_success: false,
        status_code: None,
        response_time_ms: Some(start.elapsed().as_millis() as i32),
        failure_reason: Some(failure_reason.to_string()),
        error_message: Some(error_message.to_string()),
        certificate_metadata: None,
    }
}

pub fn is_valid_record_type(s: &str) -> bool {
    parse_record_type(s).is_ok()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{DnsCheckParams, check_dns, is_valid_record_type};

    #[test]
    fn valid_record_types_are_accepted() {
        for t in ["A", "AAAA", "CNAME", "MX", "TXT", "NS"] {
            assert!(is_valid_record_type(t), "{t} should be valid");
        }
    }

    #[test]
    fn invalid_record_type_is_rejected() {
        assert!(!is_valid_record_type("PTR"));
        assert!(!is_valid_record_type("SOA"));
        assert!(!is_valid_record_type(""));
    }

    #[tokio::test]
    async fn check_dns_blocks_private_nameserver() {
        let result = check_dns(
            &DnsCheckParams {
                hostname: "example.com",
                record_type: "A",
                expected_value: None,
                nameserver: Some("127.0.0.1"),
            },
            Duration::from_secs(5),
            true,
        )
        .await;
        assert!(!result.is_success);
        assert_eq!(result.failure_reason.as_deref(), Some("config_error"));
    }

    #[tokio::test]
    async fn check_dns_resolves_real_domain() {
        let result = check_dns(
            &DnsCheckParams {
                hostname: "one.one.one.one",
                record_type: "A",
                expected_value: None,
                nameserver: None,
            },
            Duration::from_secs(10),
            false,
        )
        .await;
        assert!(
            result.is_success,
            "expected success, got: {:?}",
            result.error_message
        );
    }

    #[tokio::test]
    async fn check_dns_fails_value_mismatch() {
        let result = check_dns(
            &DnsCheckParams {
                hostname: "one.one.one.one",
                record_type: "A",
                expected_value: Some("192.0.2.1"),
                nameserver: None,
            },
            Duration::from_secs(10),
            false,
        )
        .await;
        assert!(!result.is_success);
        assert_eq!(result.failure_reason.as_deref(), Some("value_mismatch"));
    }

    #[test]
    fn record_type_parsing_is_case_insensitive() {
        for t in ["a", "aaaa", "cname", "mx", "txt", "ns"] {
            assert!(is_valid_record_type(t), "{t} should be valid in lowercase");
        }
    }

    #[tokio::test]
    async fn check_dns_rejects_hostname_as_nameserver() {
        let result = check_dns(
            &DnsCheckParams {
                hostname: "example.com",
                record_type: "A",
                expected_value: None,
                nameserver: Some("ns1.example.com"),
            },
            Duration::from_secs(5),
            false,
        )
        .await;
        assert!(!result.is_success);
        assert_eq!(result.failure_reason.as_deref(), Some("config_error"));
        let msg = result.error_message.unwrap();
        assert!(
            msg.contains("IP address"),
            "expected error about IP address, got: {msg}"
        );
    }

    #[tokio::test]
    async fn check_dns_returns_config_error_for_unsupported_record_type() {
        let result = check_dns(
            &DnsCheckParams {
                hostname: "example.com",
                record_type: "PTR",
                expected_value: None,
                nameserver: None,
            },
            Duration::from_secs(5),
            false,
        )
        .await;
        assert!(!result.is_success);
        assert_eq!(result.failure_reason.as_deref(), Some("config_error"));
    }

    #[tokio::test]
    async fn check_dns_succeeds_when_expected_value_matches() {
        let result = check_dns(
            &DnsCheckParams {
                hostname: "one.one.one.one",
                record_type: "A",
                expected_value: Some("1.1.1.1"),
                nameserver: None,
            },
            Duration::from_secs(10),
            false,
        )
        .await;
        assert!(
            result.is_success,
            "expected success, got: {:?}",
            result.error_message
        );
    }

    #[tokio::test]
    async fn check_dns_fails_for_nonexistent_domain() {
        let result = check_dns(
            &DnsCheckParams {
                hostname: "this-domain-does-not-exist-alon-sentinel.example",
                record_type: "A",
                expected_value: None,
                nameserver: None,
            },
            Duration::from_secs(5),
            false,
        )
        .await;
        assert!(!result.is_success);
        assert_eq!(result.failure_reason.as_deref(), Some("lookup_error"));
    }

    #[tokio::test]
    async fn check_dns_value_mismatch_error_includes_expected_and_actual() {
        let result = check_dns(
            &DnsCheckParams {
                hostname: "one.one.one.one",
                record_type: "A",
                expected_value: Some("192.0.2.1"),
                nameserver: None,
            },
            Duration::from_secs(10),
            false,
        )
        .await;
        assert!(!result.is_success);
        let msg = result.error_message.unwrap();
        assert!(
            msg.contains("192.0.2.1"),
            "message should include expected value: {msg}"
        );
        assert!(
            msg.contains("1.1.1.1"),
            "message should include actual value: {msg}"
        );
    }
}
