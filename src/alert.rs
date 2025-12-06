use anyhow::{Context, Result};
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

use crate::config::EmailConfig;

pub struct EmailAlerter {
    config: EmailConfig,
}

impl EmailAlerter {
    pub fn new(config: EmailConfig) -> Self {
        Self { config }
    }
    
    pub fn send_alert(&self, subject: &str, body: &str) -> Result<()> {
        log::info!("Sending email alert: {}", subject);
        
        let mut message_builder = Message::builder()
            .from(self.config.from.parse().context("Invalid 'from' email address")?)
            .subject(subject);
        
        // Add all recipients
        for to_addr in &self.config.to {
            message_builder = message_builder.to(to_addr.parse().context("Invalid 'to' email address")?);
        }
        
        let message = message_builder
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_string())
            .context("Failed to build email message")?;
        
        // Create SMTP transport
        let mut mailer_builder = SmtpTransport::relay(&self.config.smtp_host)
            .context("Failed to create SMTP transport")?;
        
        // Add credentials if provided
        if let (Some(username), Some(password)) = (&self.config.username, &self.config.password) {
            mailer_builder = mailer_builder.credentials(Credentials::new(
                username.to_string(),
                password.to_string(),
            ));
        }
        
        let mailer = mailer_builder.build();
        
        mailer
            .send(&message)
            .context("Failed to send email")?;
        
        log::info!("Email alert sent successfully");
        Ok(())
    }
}
