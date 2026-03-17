use reqwest::Client;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone)]
pub struct SlackMessage {
    pub from_user: String,
    pub text: String,
    pub channel_id: String,
    pub timestamp: String,
}

#[derive(Debug, Clone)]
pub enum SlackState {
    Idle,
    Loading,
    Ready(Vec<SlackMessage>),
    Error(String),
}

pub struct SlackCache {
    pub state: SlackState,
    last_fetched: Option<Instant>,
}

impl SlackCache {
    pub fn new() -> Self {
        Self {
            state: SlackState::Idle,
            last_fetched: None,
        }
    }

    pub fn needs_refresh(&self) -> bool {
        match &self.state {
            SlackState::Loading => false,
            SlackState::Idle | SlackState::Error(_) => true,
            SlackState::Ready(_) => self
                .last_fetched
                .map(|t| t.elapsed() >= CACHE_TTL)
                .unwrap_or(true),
        }
    }

    pub fn set_loading(&mut self) {
        self.state = SlackState::Loading;
    }

    pub fn set_result(&mut self, result: Result<Vec<SlackMessage>, String>) {
        self.last_fetched = Some(Instant::now());
        self.state = match result {
            Ok(msgs) => SlackState::Ready(msgs),
            Err(e) => SlackState::Error(e),
        };
    }
}

/// Fetch recent DMs from the configured "important" users.
/// For each user, opens the DM conversation and fetches the latest message.
pub async fn fetch(
    bot_token: &str,
    important_users: &[String],
) -> Result<Vec<SlackMessage>, String> {
    if important_users.is_empty() {
        return Ok(vec![]);
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let mut messages = Vec::new();

    for user_id in important_users {
        match fetch_dm_for_user(&client, bot_token, user_id).await {
            Ok(Some(msg)) => messages.push(msg),
            Ok(None) => {} // no messages from this user
            Err(e) => {
                // Log but don't fail the whole batch
                messages.push(SlackMessage {
                    from_user: user_id.clone(),
                    text: format!("[error: {}]", e),
                    channel_id: String::new(),
                    timestamp: String::new(),
                });
            }
        }
    }

    Ok(messages)
}

async fn fetch_dm_for_user(
    client: &Client,
    bot_token: &str,
    user_id: &str,
) -> Result<Option<SlackMessage>, String> {
    // Open/find the DM conversation with this user
    let open_resp = client
        .post("https://slack.com/api/conversations.open")
        .bearer_auth(bot_token)
        .json(&serde_json::json!({ "users": user_id }))
        .send()
        .await
        .map_err(|e| format!("Slack conversations.open: {}", e))?;

    let open_json: serde_json::Value = open_resp
        .json()
        .await
        .map_err(|e| format!("Slack parse: {}", e))?;

    if !open_json["ok"].as_bool().unwrap_or(false) {
        let err = open_json["error"].as_str().unwrap_or("unknown");
        return Err(format!("Slack: {}", err));
    }

    let channel_id = open_json["channel"]["id"]
        .as_str()
        .ok_or("No channel ID")?
        .to_string();

    // Get the latest message from this DM
    let history_url = format!(
        "https://slack.com/api/conversations.history?channel={}&limit=1",
        channel_id
    );

    let hist_resp = client
        .get(&history_url)
        .bearer_auth(bot_token)
        .send()
        .await
        .map_err(|e| format!("Slack history: {}", e))?;

    let hist_json: serde_json::Value = hist_resp
        .json()
        .await
        .map_err(|e| format!("Slack parse: {}", e))?;

    if !hist_json["ok"].as_bool().unwrap_or(false) {
        return Ok(None);
    }

    let msg = hist_json["messages"]
        .as_array()
        .and_then(|msgs| msgs.first())
        .and_then(|m| {
            Some(SlackMessage {
                from_user: m["user"].as_str().unwrap_or(user_id).to_string(),
                text: m["text"].as_str().unwrap_or("").to_string(),
                channel_id: channel_id.clone(),
                timestamp: m["ts"].as_str().unwrap_or("").to_string(),
            })
        });

    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_lifecycle() {
        let mut cache = SlackCache::new();
        assert!(cache.needs_refresh());

        cache.set_loading();
        assert!(!cache.needs_refresh());

        cache.set_result(Ok(vec![SlackMessage {
            from_user: "U123".into(),
            text: "hello".into(),
            channel_id: "C456".into(),
            timestamp: "12345.6789".into(),
        }]));
        assert!(!cache.needs_refresh());
    }
}
