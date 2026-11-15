//! Outgoing email, over SMTP (`RUPTA_SMTP_URL`) or the HTTP API of a
//! Camelmailer server (`RUPTA_CAMELMAILER_API_KEY`, `RUPTA_CAMELMAILER_URL`),
//! with the sender from `RUPTA_MAIL_FROM` (or `RUPTA_SMTP_FROM`).
//!
//! SMTP URL forms (lettre): `smtp://host:25` (plain, or STARTTLS with
//! `?tls=required`), `smtps://user:pass@host:465` (implicit TLS).

use camelmailer_rs::{Address, CamelMailer, SendEmailRequest};
use lettre::message::{Mailbox, MultiPart, SinglePart, header::ContentType};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

/// How messages leave rupta.
#[derive(Clone)]
enum Transport {
    Smtp(AsyncSmtpTransport<Tokio1Executor>),
    Camelmailer(CamelMailer),
}

#[derive(Clone)]
pub struct Mailer {
    transport: Transport,
    from: Mailbox,
}

impl std::fmt::Debug for Mailer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self.transport {
            Transport::Smtp(_) => "smtp",
            Transport::Camelmailer(_) => "camelmailer",
        };
        f.debug_struct("Mailer").field("transport", &kind).field("from", &self.from.to_string()).finish()
    }
}

fn parse_from(from: &str) -> Result<Mailbox, String> {
    from.parse().map_err(|e| format!("RUPTA_MAIL_FROM: {e}"))
}

impl Mailer {
    /// A mailer that sends over SMTP.
    pub fn from_url(url: &str, from: &str) -> Result<Mailer, String> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
            .map_err(|e| format!("RUPTA_SMTP_URL: {e}"))?
            .timeout(Some(std::time::Duration::from_secs(20)))
            .build();
        Ok(Mailer { transport: Transport::Smtp(transport), from: parse_from(from)? })
    }

    /// A mailer that sends through the HTTP API of a Camelmailer server;
    /// `base_url` defaults to the hosted service.
    pub fn camelmailer(api_key: &str, base_url: Option<&str>, from: &str) -> Result<Mailer, String> {
        let mut builder = CamelMailer::builder(api_key).timeout(std::time::Duration::from_secs(20));
        if let Some(url) = base_url {
            builder = builder.base_url(url.trim_end_matches('/'));
        }
        let client = builder.build().map_err(|e| format!("RUPTA_CAMELMAILER_URL: {e}"))?;
        Ok(Mailer { transport: Transport::Camelmailer(client), from: parse_from(from)? })
    }

    /// `smtp` or `camelmailer`, for logs and the health page.
    pub fn transport_name(&self) -> &'static str {
        match self.transport {
            Transport::Smtp(_) => "smtp",
            Transport::Camelmailer(_) => "camelmailer",
        }
    }

    /// Sends one message to each recipient (recipients do not see each other).
    pub async fn send(&self, to: &[String], subject: &str, text: &str, html: Option<&str>) -> Result<(), String> {
        let mut errors = Vec::new();
        for addr in to {
            let mailbox: Mailbox = match addr.parse() {
                Ok(m) => m,
                Err(e) => {
                    errors.push(format!("{addr}: {e}"));
                    continue;
                }
            };
            let result = match &self.transport {
                Transport::Smtp(smtp) => self.send_smtp(smtp, mailbox, subject, text, html).await,
                Transport::Camelmailer(client) => self.send_camelmailer(client, &mailbox, subject, text, html).await,
            };
            if let Err(e) = result {
                errors.push(format!("{addr}: {e}"));
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }

    async fn send_smtp(
        &self,
        smtp: &AsyncSmtpTransport<Tokio1Executor>,
        to: Mailbox,
        subject: &str,
        text: &str,
        html: Option<&str>,
    ) -> Result<(), String> {
        let builder = Message::builder().from(self.from.clone()).to(to).subject(subject);
        let msg = match html {
            Some(html) => builder.multipart(
                MultiPart::alternative()
                    .singlepart(SinglePart::builder().header(ContentType::TEXT_PLAIN).body(text.to_string()))
                    .singlepart(SinglePart::builder().header(ContentType::TEXT_HTML).body(html.to_string())),
            ),
            None => builder.header(ContentType::TEXT_PLAIN).body(text.to_string()),
        }
        .map_err(|e| e.to_string())?;
        smtp.send(msg).await.map(|_| ()).map_err(|e| e.to_string())
    }

    async fn send_camelmailer(
        &self,
        client: &CamelMailer,
        to: &Mailbox,
        subject: &str,
        text: &str,
        html: Option<&str>,
    ) -> Result<(), String> {
        let mut request =
            SendEmailRequest::builder().from(address(&self.from)).to(address(to)).subject(subject).text_body(text);
        if let Some(html) = html {
            request = request.html_body(html);
        }
        client.emails().send(request.build()).await.map(|_| ()).map_err(|e| e.to_string())
    }
}

fn address(m: &Mailbox) -> Address {
    match &m.name {
        Some(name) => Address::with_name(m.email.to_string(), name.clone()),
        None => Address::from(m.email.to_string()),
    }
}

/// Escapes text for HTML bodies.
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::Router;
    use axum::http::HeaderMap;
    use axum::routing::post;

    use super::*;

    /// API key header and JSON body of each request the fake server saw.
    type Seen = Arc<Mutex<Vec<(Option<String>, serde_json::Value)>>>;

    #[tokio::test]
    async fn camelmailer_sends_one_message_per_recipient() {
        let seen: Seen = Arc::default();
        let record = seen.clone();
        let app = Router::new().route(
            "/api/v2/server/messages",
            post(move |headers: HeaderMap, body: axum::Json<serde_json::Value>| {
                let record = record.clone();
                async move {
                    let key = headers.get("x-server-api-key").and_then(|v| v.to_str().ok()).map(str::to_string);
                    record.lock().unwrap().push((key, body.0));
                    axum::Json(serde_json::json!({"status": "success", "time": 0.01, "data": {"message_id": 1, "recipients": []}}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let mailer = Mailer::camelmailer("cm_test", Some(&url), "rupta <rupta@example.com>").unwrap();
        assert_eq!(mailer.transport_name(), "camelmailer");
        mailer
            .send(&["ada@example.com".into(), "Grace <grace@example.com>".into()], "Alert", "text", Some("<p>html</p>"))
            .await
            .unwrap();

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "one request per recipient");
        let (key, body) = &seen[0];
        assert_eq!(key.as_deref(), Some("cm_test"));
        let raw = body.to_string();
        assert!(raw.contains("rupta@example.com") && raw.contains("ada@example.com"), "{raw}");
        assert!(raw.contains("Alert") && raw.contains("<p>html</p>") && raw.contains("text"), "{raw}");
        assert!(seen[1].1.to_string().contains("grace@example.com"));
    }

    #[tokio::test]
    async fn camelmailer_errors_are_reported_per_recipient() {
        let app = Router::new().route(
            "/api/v2/server/messages",
            post(|| async {
                (axum::http::StatusCode::UNAUTHORIZED, axum::Json(serde_json::json!({"status": "error", "time": 0.01, "error": {"code": "InvalidServerAPIKey", "message": "bad key"}})))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mailer = Mailer::camelmailer("cm_wrong", Some(&url), "rupta@example.com").unwrap();
        let err = mailer.send(&["ada@example.com".into()], "s", "t", None).await.unwrap_err();
        assert!(err.starts_with("ada@example.com: "), "{err}");
    }

    #[test]
    fn invalid_sender_is_refused() {
        let err = Mailer::camelmailer("cm", None, "not an address").unwrap_err();
        assert!(err.contains("RUPTA_MAIL_FROM"), "{err}");
    }
}
