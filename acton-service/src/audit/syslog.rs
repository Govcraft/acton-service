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

        let app_name = config.app_name.clone().unwrap_or_else(|| "acton".to_string());

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
        if let Some(ref ip) = event.source.ip {
            sd_params.push(format!("src_ip=\"{}\"", escape_sd_value(ip)));
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
        if let Some(ref hash) = event.hash {
            sd_params.push(format!("hash=\"{}\"", hash));
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
}
