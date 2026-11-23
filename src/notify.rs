//! Where alerts go. Email today; Slack, webhooks and the like can implement
//! the same trait.

use crate::config::{ConfigEmail, SmtpSecurity};
use anyhow::{Context, Result};
use lettre::message::{header::ContentType, Mailbox};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

pub trait Notifier {
    fn notify(&self, subject: &str, body: &str) -> Result<()>;
}

/// Used when no channel is configured: alerts only reach the log.
pub struct LogNotifier;

impl Notifier for LogNotifier {
    fn notify(&self, subject: &str, body: &str) -> Result<()> {
        log::warn!("No alert channel configured. {}\n{}", subject, body);
        Ok(())
    }
}

pub struct EmailNotifier {
    from: Mailbox,
    to: Vec<Mailbox>,
    transport: SmtpTransport,
}

impl EmailNotifier {
    /// Builds the SMTP transport once and checks every address up front, so a
    /// typo fails at startup instead of at the first alert.
    pub fn new(cfg: &ConfigEmail) -> Result<Self> {
        let from: Mailbox = cfg
            .from
            .parse()
            .with_context(|| format!("email.from '{}' is not a valid address", cfg.from))?;
        let to = cfg
            .to
            .iter()
            .map(|a| {
                a.parse()
                    .with_context(|| format!("email.to '{}' is not a valid address", a))
            })
            .collect::<Result<Vec<Mailbox>>>()?;

        let mut builder = match cfg.security() {
            SmtpSecurity::Starttls => SmtpTransport::starttls_relay(&cfg.smtp_host)
                .with_context(|| format!("cannot set up STARTTLS for {}", cfg.smtp_host))?,
            SmtpSecurity::Tls => SmtpTransport::relay(&cfg.smtp_host)
                .with_context(|| format!("cannot set up TLS for {}", cfg.smtp_host))?,
            SmtpSecurity::None => SmtpTransport::builder_dangerous(&cfg.smtp_host),
        }
        .port(cfg.smtp_port);

        if let (Some(user), Some(pass)) = (&cfg.username, &cfg.password) {
            builder = builder.credentials(Credentials::new(user.clone(), pass.clone()));
        }

        Ok(EmailNotifier {
            from,
            to,
            transport: builder.build(),
        })
    }
}

impl Notifier for EmailNotifier {
    fn notify(&self, subject: &str, body: &str) -> Result<()> {
        let mut msg = Message::builder()
            .from(self.from.clone())
            .subject(subject)
            .header(ContentType::TEXT_PLAIN);
        for to in &self.to {
            msg = msg.to(to.clone());
        }
        let msg = msg
            .body(body.to_string())
            .context("failed to build email")?;
        self.transport.send(&msg).context("SMTP send failed")?;
        Ok(())
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::cell::RefCell;

    /// Records alerts instead of sending them. Can be told to fail.
    #[derive(Default)]
    pub struct RecordingNotifier {
        pub sent: RefCell<Vec<(String, String)>>,
        pub fail: RefCell<bool>,
    }

    impl Notifier for RecordingNotifier {
        fn notify(&self, subject: &str, body: &str) -> Result<()> {
            if *self.fail.borrow() {
                anyhow::bail!("simulated send failure");
            }
            self.sent
                .borrow_mut()
                .push((subject.to_string(), body.to_string()));
            Ok(())
        }
    }
}
