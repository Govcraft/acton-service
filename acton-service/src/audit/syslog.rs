//! Syslog RFC 5424 formatter and sender
//!
//! Sends audit events as structured syslog messages over UDP or TCP.
//! No external syslog crate is used — the RFC 5424 format is simple enough
//! to generate directly.

use std::net::SocketAddr;
use tokio::net::UdpSocket;

use super::config::SyslogConfig;
use super::event::AuditEvent;

/// Syslog sender for dispatching audit events
#[derive(Clone)]
pub struct SyslogSender {
    address: SocketAddr,
    facility: u8,
    app_name: String,
    transport: SyslogTransport,
}

#[derive(Clone, Debug)]
enum SyslogTransport {
    Udp,
    Tcp,
}

impl SyslogSender {
    /// Create a new syslog sender from configuration
    pub fn new(config: &SyslogConfig) -> Result<Self, std::io::Error> {
        let address: SocketAddr = config
            .address
            .parse()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

        let transport = match config.transport.as_str() {
            "tcp" => SyslogTransport::Tcp,
            _ => SyslogTransport::Udp,
        };

        let app_name = config
            .app_name
            .clone()
            .unwrap_or_else(|| "acton".to_string());

        Ok(Self {
            address,
            facility: config.facility,
            app_name,
            transport,
        })
    }

    /// Send an audit event as an RFC 5424 syslog message
    pub async fn send(&self, event: &AuditEvent) -> Result<(), std::io::Error> {
        let message = self.format_rfc5424(event);

        match self.transport {
            SyslogTransport::Udp => {
                let socket = UdpSocket::bind("0.0.0.0:0").await?;
                socket.send_to(message.as_bytes(), self.address).await?;
            }
            SyslogTransport::Tcp => {
                use tokio::io::AsyncWriteExt;
                use tokio::net::TcpStream;

                match TcpStream::connect(self.address).await {
                    Ok(mut stream) => {
                        // RFC 5425: TCP syslog uses newline framing
                        let framed = format!("{}\n", message);
                        stream.write_all(framed.as_bytes()).await?;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to connect to syslog TCP endpoint: {}", e);
                        return Err(e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Format an audit event as RFC 5424 syslog message
    ///
    /// Format: `<PRI>VERSION TIMESTAMP HOSTNAME APP-NAME PROCID MSGID [SD-ID SD-PARAM...] MSG`
    ///
    /// The structured data carries **every input of the chain hash** — with
    /// syslog as the only durable store, the exported stream must be enough
    /// to recompute and verify the chain on its own. `ts` is the exact
    /// RFC 3339 string the hash consumes (the header timestamp uses the
    /// RFC 5424 format and is not byte-identical), `metadata` is the same
    /// canonical key-sorted JSON the hash consumes, and `prev_hash` makes any
    /// contiguous slice of exported lines verifiable without the lines before
    /// it.
    fn format_rfc5424(&self, event: &AuditEvent) -> String {
        // PRI = facility * 8 + severity
        let pri = (self.facility as u16) * 8 + event.severity.as_syslog_severity() as u16;

        // RFC 5424 timestamp format
        let timestamp = event.timestamp.format("%Y-%m-%dT%H:%M:%S%.6fZ");

        // Hostname: use service name
        let hostname = &event.service_name;

        // MSGID: event kind
        let msgid = event.kind.to_string();

        // Structured data
        let mut sd_params = Vec::new();
        sd_params.push(format!("event_id=\"{}\"", event.id));
        sd_params.push(format!(
            "ts=\"{}\"",
            escape_sd_value(&event.timestamp.to_rfc3339())
        ));
        sd_params.push(format!(
            "kind=\"{}\"",
            escape_sd_value(&event.kind.to_string())
        ));
        sd_params.push(format!(
            "severity=\"{}\"",
            event.severity.as_syslog_severity()
        ));
        sd_params.push(format!(
            "service=\"{}\"",
            escape_sd_value(&event.service_name)
        ));
        if let Some(ref ip) = event.source.ip {
            sd_params.push(format!("src_ip=\"{}\"", escape_sd_value(ip)));
        }
        if let Some(ref user_agent) = event.source.user_agent {
            sd_params.push(format!("ua=\"{}\"", escape_sd_value(user_agent)));
        }
        if let Some(ref subject) = event.source.subject {
            sd_params.push(format!("subject=\"{}\"", escape_sd_value(subject)));
        }
        if let Some(ref request_id) = event.source.request_id {
            sd_params.push(format!("request_id=\"{}\"", escape_sd_value(request_id)));
        }
        if let Some(ref method) = event.method {
            sd_params.push(format!("method=\"{}\"", escape_sd_value(method)));
        }
        if let Some(ref path) = event.path {
            sd_params.push(format!("path=\"{}\"", escape_sd_value(path)));
        }
        if let Some(code) = event.status_code {
            sd_params.push(format!("status=\"{}\"", code));
        }
        if let Some(ms) = event.duration_ms {
            sd_params.push(format!("duration_ms=\"{}\"", ms));
        }
        if let Some(ref metadata) = event.metadata {
            sd_params.push(format!(
                "metadata=\"{}\"",
                escape_sd_value(&crate::audit::chain::canonical_json(metadata))
            ));
        }
        if let Some(ref prev) = event.previous_hash {
            sd_params.push(format!("prev_hash=\"{}\"", escape_sd_value(prev)));
        }
        if let Some(ref hash) = event.hash {
            sd_params.push(format!("hash=\"{}\"", escape_sd_value(hash)));
        }
        sd_params.push(format!("seq=\"{}\"", event.sequence));

        let structured_data = if sd_params.is_empty() {
            "-".to_string()
        } else {
            format!("[audit@49610 {}]", sd_params.join(" "))
        };

        // Message body
        let msg = format!("{} seq={}", event.kind, event.sequence);

        format!(
            "<{}>{} {} {} {} - {} {} {}",
            pri, 1, timestamp, hostname, self.app_name, msgid, structured_data, msg
        )
    }
}

/// Escape special characters in structured data values per RFC 5424
fn escape_sd_value(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(']', "\\]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::event::{AuditEventKind, AuditSeverity};

    #[test]
    fn test_syslog_format_rfc5424() {
        let sender = SyslogSender {
            address: "127.0.0.1:514".parse().unwrap(),
            facility: 13,
            app_name: "test-app".to_string(),
            transport: SyslogTransport::Udp,
        };

        let event = AuditEvent::new(
            AuditEventKind::AuthLoginSuccess,
            AuditSeverity::Informational,
            "test-service".to_string(),
        );

        let message = sender.format_rfc5424(&event);

        // PRI = 13*8 + 6 = 110
        assert!(message.starts_with("<110>1"));
        assert!(message.contains("test-service"));
        assert!(message.contains("test-app"));
        assert!(message.contains("auth.login.success"));
    }

    #[test]
    fn test_escape_sd_value() {
        assert_eq!(escape_sd_value("hello"), "hello");
        assert_eq!(escape_sd_value("he\"llo"), "he\\\"llo");
        assert_eq!(escape_sd_value("he\\llo"), "he\\\\llo");
        assert_eq!(escape_sd_value("he]llo"), "he\\]llo");
    }

    /// Escape-aware parse of the `[audit@49610 …]` structured-data element
    /// into key/value pairs — the verifier side of the export format.
    fn parse_sd_params(message: &str) -> std::collections::HashMap<String, String> {
        let start = message
            .find("[audit@49610 ")
            .expect("audit structured data present")
            + "[audit@49610 ".len();
        let mut params = std::collections::HashMap::new();
        let mut chars = message[start..].chars().peekable();

        loop {
            match chars.peek() {
                Some(']') => break,
                Some(' ') => {
                    chars.next();
                }
                Some(_) => {
                    let mut key = String::new();
                    for c in chars.by_ref() {
                        if c == '=' {
                            break;
                        }
                        key.push(c);
                    }
                    assert_eq!(chars.next(), Some('"'), "param value must be quoted");
                    let mut value = String::new();
                    loop {
                        match chars.next().expect("unterminated param value") {
                            '\\' => value.push(chars.next().expect("dangling escape")),
                            '"' => break,
                            c => value.push(c),
                        }
                    }
                    params.insert(key, value);
                }
                None => panic!("unterminated structured data"),
            }
        }
        params
    }

    fn severity_from_byte(byte: u8) -> AuditSeverity {
        match byte {
            0 => AuditSeverity::Emergency,
            1 => AuditSeverity::Alert,
            2 => AuditSeverity::Critical,
            3 => AuditSeverity::Error,
            4 => AuditSeverity::Warning,
            5 => AuditSeverity::Notice,
            6 => AuditSeverity::Informational,
            _ => AuditSeverity::Debug,
        }
    }

    /// Rebuild an event purely from an exported line's structured data. The
    /// kind maps to `Custom(raw)` — only the kind's string enters the hash,
    /// so a verifier needs no knowledge of the kind vocabulary.
    fn event_from_sd(params: &std::collections::HashMap<String, String>) -> AuditEvent {
        AuditEvent {
            id: params["event_id"].parse().expect("uuid parses"),
            timestamp: chrono::DateTime::parse_from_rfc3339(&params["ts"])
                .expect("rfc3339 parses")
                .with_timezone(&chrono::Utc),
            kind: AuditEventKind::from_wire(&params["kind"]).expect("kind is known"),
            severity: severity_from_byte(params["severity"].parse().expect("severity parses")),
            source: crate::audit::event::AuditSource {
                ip: params.get("src_ip").cloned(),
                user_agent: params.get("ua").cloned(),
                subject: params.get("subject").cloned(),
                request_id: params.get("request_id").cloned(),
            },
            method: params.get("method").cloned(),
            path: params.get("path").cloned(),
            status_code: params.get("status").map(|s| s.parse().expect("status parses")),
            duration_ms: params
                .get("duration_ms")
                .map(|s| s.parse().expect("duration parses")),
            service_name: params["service"].clone(),
            metadata: params
                .get("metadata")
                .map(|s| serde_json::from_str(s).expect("metadata parses")),
            hash: params.get("hash").cloned(),
            previous_hash: params.get("prev_hash").cloned(),
            sequence: params["seq"].parse().expect("seq parses"),
        }
    }

    /// The load-bearing property of syslog-only durability: a chain rebuilt
    /// from nothing but exported lines recomputes and verifies, and a
    /// tampered export does not.
    #[test]
    fn exported_chain_recomputes_and_verifies_from_lines_alone() {
        let sender = SyslogSender {
            address: "127.0.0.1:514".parse().unwrap(),
            facility: 13,
            app_name: "test-app".to_string(),
            transport: SyslogTransport::Udp,
        };

        let mut chain = crate::audit::chain::AuditChain::new("test-service".to_string());
        let mut lines = Vec::new();
        for index in 0..3u16 {
            let mut event = AuditEvent::new(
                AuditEventKind::Custom(format!("admin.action{index}")),
                AuditSeverity::Warning,
                "test-service".to_string(),
            )
            .with_source(crate::audit::event::AuditSource {
                ip: Some("198.51.100.42".to_string()),
                user_agent: Some("curl/8.0 \"quoted\" [bracketed]".to_string()),
                subject: Some("operator-1".to_string()),
                request_id: Some(format!("req_{index}")),
            })
            .with_http("POST".to_string(), "/admin/x".to_string(), Some(200), Some(12));
            event.metadata =
                Some(serde_json::json!({"roles": ["auditor"], "decision": "permit"}));
            let sealed = chain.seal(event);
            lines.push(sender.format_rfc5424(&sealed));
        }

        let rebuilt: Vec<AuditEvent> = lines
            .iter()
            .map(|line| event_from_sd(&parse_sd_params(line)))
            .collect();

        crate::audit::chain::verify_chain(&rebuilt)
            .expect("chain rebuilt from exported lines alone must verify");

        // A tampered export breaks: flip one metadata byte in the middle line.
        let mut tampered = rebuilt;
        tampered[1].metadata = Some(serde_json::json!({"roles": ["provisioner"]}));
        assert!(crate::audit::chain::verify_chain(&tampered).is_err());
    }
}
