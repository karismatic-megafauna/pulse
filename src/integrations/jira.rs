use reqwest::Client;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(5 * 60); // 5 minutes

#[derive(Debug, Clone)]
pub struct JiraIssue {
    pub key: String,
    pub summary: String,
    pub status: String,
    pub priority: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub enum JiraState {
    Idle,
    Loading,
    Ready(Vec<JiraIssue>),
    Error(String),
}

pub struct JiraCache {
    pub state: JiraState,
    last_fetched: Option<Instant>,
}

impl JiraCache {
    pub fn new() -> Self {
        Self {
            state: JiraState::Idle,
            last_fetched: None,
        }
    }

    pub fn needs_refresh(&self) -> bool {
        match &self.state {
            JiraState::Loading => false,
            JiraState::Idle | JiraState::Error(_) => true,
            JiraState::Ready(_) => self
                .last_fetched
                .map(|t| t.elapsed() >= CACHE_TTL)
                .unwrap_or(true),
        }
    }

    pub fn set_loading(&mut self) {
        self.state = JiraState::Loading;
    }

    pub fn set_result(&mut self, result: Result<Vec<JiraIssue>, String>) {
        self.last_fetched = Some(Instant::now());
        self.state = match result {
            Ok(issues) => JiraState::Ready(issues),
            Err(e) => JiraState::Error(e),
        };
    }
}

pub async fn fetch(base_url: &str, email: &str, api_token: &str) -> Result<Vec<JiraIssue>, String> {
    let jql = "assignee = currentUser() AND resolution = Unresolved ORDER BY priority DESC, updated DESC";
    let url = format!(
        "{}/rest/api/3/search/jql?jql={}&maxResults=15&fields=summary,status,priority",
        base_url.trim_end_matches('/'),
        urlencoding::encode(jql)
    );

    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .get(&url)
        .basic_auth(email, Some(api_token))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Jira request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Jira HTTP {}: {}", status, &body[..body.len().min(200)]));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Jira parse error: {}", e))?;

    let issues = json["issues"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|issue| {
            let key = issue["key"].as_str()?.to_string();
            let fields = &issue["fields"];
            Some(JiraIssue {
                url: format!(
                    "{}/browse/{}",
                    base_url.trim_end_matches('/'),
                    key
                ),
                key,
                summary: fields["summary"].as_str().unwrap_or("").to_string(),
                status: fields["status"]["name"].as_str().unwrap_or("").to_string(),
                priority: fields["priority"]["name"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect();

    Ok(issues)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_needs_refresh_when_idle() {
        let cache = JiraCache::new();
        assert!(cache.needs_refresh());
    }

    #[test]
    fn test_cache_no_refresh_while_loading() {
        let mut cache = JiraCache::new();
        cache.set_loading();
        assert!(!cache.needs_refresh());
    }

    #[test]
    fn test_cache_no_refresh_when_fresh() {
        let mut cache = JiraCache::new();
        cache.set_result(Ok(vec![]));
        assert!(!cache.needs_refresh());
    }

    fn sample_jira_json() -> serde_json::Value {
        serde_json::json!({
            "issues": [
                {
                    "key": "SG-123",
                    "fields": {
                        "summary": "Fix auth flow",
                        "status": { "name": "In Progress" },
                        "priority": { "name": "High" }
                    }
                },
                {
                    "key": "SG-456",
                    "fields": {
                        "summary": "Update docs",
                        "status": { "name": "To Do" },
                        "priority": { "name": "Medium" }
                    }
                }
            ]
        })
    }

    #[test]
    fn test_parse_jira_issues() {
        let json = sample_jira_json();
        let issues: Vec<JiraIssue> = json["issues"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|issue| {
                let key = issue["key"].as_str()?.to_string();
                let fields = &issue["fields"];
                Some(JiraIssue {
                    url: format!("https://example.atlassian.net/browse/{}", key),
                    key,
                    summary: fields["summary"].as_str().unwrap_or("").to_string(),
                    status: fields["status"]["name"].as_str().unwrap_or("").to_string(),
                    priority: fields["priority"]["name"].as_str().unwrap_or("").to_string(),
                })
            })
            .collect();

        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].key, "SG-123");
        assert_eq!(issues[0].status, "In Progress");
        assert_eq!(issues[1].summary, "Update docs");
    }
}
