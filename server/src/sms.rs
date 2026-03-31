use reqwest::Client;
use std::env;

pub struct TwilioConfig {
    account_sid: String,
    auth_token: String,
    from_number: String,
    client: Client,
}

impl TwilioConfig {
    pub fn from_env() -> Option<Self> {
        let account_sid = env::var("TWILIO_ACCOUNT_SID").ok()?;
        let auth_token = env::var("TWILIO_AUTH_TOKEN").ok()?;
        let from_number = env::var("TWILIO_FROM_NUMBER").ok()?;

        Some(Self {
            account_sid,
            auth_token,
            from_number,
            client: Client::new(),
        })
    }

    pub async fn send_pin_sms(&self, to: &str, pin: &str) -> Result<(), String> {
        let url = format!(
            "https://api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
            self.account_sid
        );

        let body = format!(
            "Your Simon locker PIN is: {}. It expires in 10 minutes.",
            pin
        );

        let res = self
            .client
            .post(&url)
            .basic_auth(&self.account_sid, Some(&self.auth_token))
            .form(&[
                ("To", to),
                ("From", &self.from_number),
                ("Body", &body),
            ])
            .send()
            .await
            .map_err(|e| format!("Twilio request failed: {}", e))?;

        if res.status().is_success() {
            tracing::info!("SMS sent to {}", to);
            Ok(())
        } else {
            let text = res.text().await.unwrap_or_default();
            tracing::error!("Twilio error: {}", text);
            Err(format!("Twilio error: {}", text))
        }
    }
}
