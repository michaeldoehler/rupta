//! Outgoing email over SMTP (`RUPTA_SMTP_URL`, `RUPTA_SMTP_FROM`).
//!
//! URL forms (lettre): `smtp://host:25` (plain, or STARTTLS with
//! `?tls=required`), `smtps://user:pass@host:465` (implicit TLS).

use lettre::message::{Mailbox, MultiPart, SinglePart, header::ContentType};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

#[derive(Clone)]
pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl std::fmt::Debug for Mailer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mailer").field("from", &self.from.to_string()).finish_non_exhaustive()
    }
}

impl Mailer {
    pub fn from_url(url: &str, from: &str) -> Result<Mailer, String> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
            .map_err(|e| format!("RUPTA_SMTP_URL: {e}"))?
            .timeout(Some(std::time::Duration::from_secs(20)))
            .build();
        let from: Mailbox = from.parse().map_err(|e| format!("RUPTA_SMTP_FROM: {e}"))?;
        Ok(Mailer { transport, from })
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
            let builder = Message::builder().from(self.from.clone()).to(mailbox).subject(subject);
            let msg = match html {
                Some(html) => builder.multipart(
                    MultiPart::alternative()
                        .singlepart(SinglePart::builder().header(ContentType::TEXT_PLAIN).body(text.to_string()))
                        .singlepart(SinglePart::builder().header(ContentType::TEXT_HTML).body(html.to_string())),
                ),
                None => builder.header(ContentType::TEXT_PLAIN).body(text.to_string()),
            };
            match msg {
                Ok(m) => {
                    if let Err(e) = self.transport.send(m).await {
                        errors.push(format!("{addr}: {e}"));
                    }
                }
                Err(e) => errors.push(format!("{addr}: {e}")),
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }
}

/// Escapes text for HTML bodies.
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}
