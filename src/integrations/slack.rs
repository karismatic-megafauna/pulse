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
/// Uses conversations.list (im:read) to find DM channels, then
/// conversations.history (im:history) to get the latest message.
/// Does NOT require im:write scope.
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

    // Build a map of user_id -> DM channel_id using conversations.list
    let dm_map = list_dm_channels(&client, bot_token).await?;

    let mut messages = Vec::new();

    for user_id in important_users {
        let channel_id = dm_map.get(user_id.as_str());
        match channel_id {
            Some(ch) => {
                match fetch_latest_message(&client, bot_token, ch, user_id).await {
                    Ok(Some(msg)) => messages.push(msg),
                    Ok(None) => {}
                    Err(e) => {
                        messages.push(SlackMessage {
                            from_user: user_id.clone(),
                            text: format!("[error: {}]", e),
                            channel_id: String::new(),
                            timestamp: String::new(),
                        });
                    }
                }
            }
            None => {
                messages.push(SlackMessage {
                    from_user: user_id.clone(),
                    text: "[no DM channel found — bot may not have access]".to_string(),
                    channel_id: String::new(),
                    timestamp: String::new(),
                });
            }
        }
    }

    Ok(messages)
}

/// List all IM (DM) channels the bot can see, returning a map of user_id -> channel_id.
async fn list_dm_channels(
    client: &Client,
    bot_token: &str,
) -> Result<std::collections::HashMap<String, String>, String> {
    let mut map = std::collections::HashMap::new();
    let mut cursor = String::new();

    loop {
        let mut url = "https://slack.com/api/conversations.list?types=im&limit=200".to_string();
        if !cursor.is_empty() {
            url.push_str(&format!("&cursor={}", cursor));
        }

        let resp = client
            .get(&url)
            .bearer_auth(bot_token)
            .send()
            .await
            .map_err(|e| format!("Slack conversations.list: {}", e))?;

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Slack parse: {}", e))?;

        if !json["ok"].as_bool().unwrap_or(false) {
            let err = json["error"].as_str().unwrap_or("unknown");
            return Err(format!("Slack: {}", err));
        }

        if let Some(channels) = json["channels"].as_array() {
            for ch in channels {
                if let (Some(id), Some(user)) = (
                    ch["id"].as_str(),
                    ch["user"].as_str(),
                ) {
                    map.insert(user.to_string(), id.to_string());
                }
            }
        }

        // Paginate if needed
        let next = json["response_metadata"]["next_cursor"]
            .as_str()
            .unwrap_or("");
        if next.is_empty() {
            break;
        }
        cursor = next.to_string();
    }

    Ok(map)
}

async fn fetch_latest_message(
    client: &Client,
    bot_token: &str,
    channel_id: &str,
    user_id: &str,
) -> Result<Option<SlackMessage>, String> {

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
                channel_id: channel_id.to_string(),
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
