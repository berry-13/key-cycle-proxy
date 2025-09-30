use crate::config::ApiKeyInfo;
use dashmap::DashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

#[derive(Debug)]
pub struct KeyPool {
    keys: Vec<Arc<ApiKeyInfo>>,
    current_index: AtomicUsize,
    strategy: RotationStrategy,
    latency_cache: DashMap<usize, (Duration, Instant)>,
}

#[derive(Debug, Clone)]
pub enum RotationStrategy {
    RoundRobin,
    RoundRobinHealthWeighted,
    LeastLatency,
}

impl From<&str> for RotationStrategy {
    fn from(s: &str) -> Self {
        match s {
            "round_robin" => RotationStrategy::RoundRobin,
            "round_robin_health_weighted" => RotationStrategy::RoundRobinHealthWeighted,
            "least_latency" => RotationStrategy::LeastLatency,
            _ => RotationStrategy::RoundRobinHealthWeighted,
        }
    }
}

impl KeyPool {
    pub fn new(keys: Vec<ApiKeyInfo>, strategy: &str) -> Self {
        let keys: Vec<Arc<ApiKeyInfo>> = keys.into_iter().map(Arc::new).collect();
        Self {
            keys,
            current_index: AtomicUsize::new(0),
            strategy: RotationStrategy::from(strategy),
            latency_cache: DashMap::new(),
        }
    }

    /// Get the best available API key for the given model
    pub fn get_key_for_model(&self, model: &str) -> Option<Arc<ApiKeyInfo>> {
        let matching_keys: Vec<(usize, &Arc<ApiKeyInfo>)> = self
            .keys
            .iter()
            .enumerate()
            .filter(|(_, key)| key.supports_model(model))
            .collect();

        if matching_keys.is_empty() {
            return None;
        }

        match self.strategy {
            RotationStrategy::RoundRobin => self.round_robin_selection(&matching_keys),
            RotationStrategy::RoundRobinHealthWeighted => {
                self.health_weighted_selection(&matching_keys)
            }
            RotationStrategy::LeastLatency => self.least_latency_selection(&matching_keys),
        }
    }

    /// Get the next key in round-robin fashion
    pub fn get_next_key(&self) -> Option<Arc<ApiKeyInfo>> {
        if self.keys.is_empty() {
            return None;
        }

        let current = self.current_index.fetch_add(1, Ordering::SeqCst);
        let index = current % self.keys.len();
        Some(self.keys[index].clone())
    }

    /// Update latency measurement for a key
    pub fn update_latency(&self, key_index: usize, latency: Duration) {
        self.latency_cache.insert(key_index, (latency, Instant::now()));
        debug!("Updated latency for key {}: {:?}", key_index, latency);
    }

    /// Get all keys for health checking
    pub fn get_all_keys(&self) -> &[Arc<ApiKeyInfo>] {
        &self.keys
    }

    /// Measure latency for all keys by making HEAD requests
    pub async fn update_all_latencies(&self) {
        info!("Starting latency measurements for {} keys", self.keys.len());
        
        let client = reqwest::Client::new();
        let mut tasks = vec![];

        for (index, key_info) in self.keys.iter().enumerate() {
            let client = client.clone();
            let url = key_info.url.clone();
            
            let task = tokio::spawn(async move {
                let latency = measure_key_latency(&client, &url).await;
                (index, latency)
            });
            
            tasks.push(task);
        }

        // Wait for all measurements to complete
        for task in tasks {
            match task.await {
                Ok((index, latency)) => {
                    self.update_latency(index, latency);
                }
                Err(e) => {
                    error!("Failed to measure latency: {}", e);
                }
            }
        }

        self.log_latency_summary();
    }

    fn round_robin_selection(&self, keys: &[(usize, &Arc<ApiKeyInfo>)]) -> Option<Arc<ApiKeyInfo>> {
        if keys.is_empty() {
            return None;
        }
        
        let current = self.current_index.fetch_add(1, Ordering::SeqCst);
        let index = current % keys.len();
        Some(keys[index].1.clone())
    }

    fn health_weighted_selection(&self, keys: &[(usize, &Arc<ApiKeyInfo>)]) -> Option<Arc<ApiKeyInfo>> {
        if keys.is_empty() {
            return None;
        }

        // For now, use round-robin with simple health scoring
        // TODO: Implement proper health scoring based on recent errors
        self.round_robin_selection(keys)
    }

    fn least_latency_selection(&self, keys: &[(usize, &Arc<ApiKeyInfo>)]) -> Option<Arc<ApiKeyInfo>> {
        if keys.is_empty() {
            return None;
        }

        // Find the key with the lowest cached latency
        let mut best_key = keys[0].1.clone();
        let mut best_latency = Duration::from_secs(u64::MAX);

        for (index, key) in keys {
            if let Some(entry) = self.latency_cache.get(index) {
                let (latency, _) = entry.value();
                if *latency < best_latency {
                    best_latency = *latency;
                    best_key = (*key).clone();
                }
            }
        }

        Some(best_key)
    }

    fn log_latency_summary(&self) {
        let mut latencies = vec![];
        for (i, key) in self.keys.iter().enumerate() {
            if let Some(entry) = self.latency_cache.get(&i) {
                let (latency, _) = entry.value();
                latencies.push(format!("{}:{:?}", key.url, latency));
            }
        }
        if !latencies.is_empty() {
            info!("Updated proxy latencies: {}", latencies.join(", "));
        }
    }
}

async fn measure_key_latency(client: &reqwest::Client, url: &str) -> Duration {
    let start = Instant::now();
    
    // Make a HEAD request with timeout
    let request_timeout = Duration::from_secs(5);
    let result = timeout(request_timeout, client.head(url).send()).await;
    
    match result {
        Ok(Ok(_)) => {
            let duration = start.elapsed();
            debug!("Latency measurement for {}: {:?}", url, duration);
            duration
        }
        Ok(Err(e)) => {
            warn!("HTTP error measuring latency for {}: {}", url, e);
            Duration::from_secs(u64::MAX)
        }
        Err(_) => {
            warn!("Timeout measuring latency for {}", url);
            Duration::from_secs(u64::MAX)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::SecretString;

    fn create_test_key(id: &str, models: Vec<&str>) -> ApiKeyInfo {
        ApiKeyInfo {
            key: SecretString::new(format!("test-key-{}", id)),
            url: format!("https://api-{}.example.com", id),
            models: models.into_iter().map(String::from).collect(),
            latency: None,
            health_score: 1.0,
        }
    }

    #[test]
    fn test_round_robin_selection() {
        let keys = vec![
            create_test_key("1", vec!["gpt-3.5-turbo"]),
            create_test_key("2", vec!["gpt-3.5-turbo"]),
            create_test_key("3", vec!["gpt-4"]),
        ];

        let pool = KeyPool::new(keys, "round_robin");

        // Test round-robin for gpt-3.5-turbo (should cycle between keys 1 and 2)
        let key1 = pool.get_key_for_model("gpt-3.5-turbo").unwrap();
        let key2 = pool.get_key_for_model("gpt-3.5-turbo").unwrap();
        
        // Keys should be different (round-robin)
        assert_ne!(key1.url, key2.url);
    }

    #[test]
    fn test_model_matching() {
        let keys = vec![
            create_test_key("1", vec!["gpt-3.5-turbo"]),
            create_test_key("2", vec!["gpt-4"]),
            create_test_key("3", vec!["others"]),
        ];

        let pool = KeyPool::new(keys, "round_robin");

        // Test specific model matching
        let key_gpt35 = pool.get_key_for_model("gpt-3.5-turbo").unwrap();
        assert!(key_gpt35.url.contains("api-1") || key_gpt35.url.contains("api-3")); // key 1 or 3 (others)

        let key_gpt4 = pool.get_key_for_model("gpt-4").unwrap();
        assert!(key_gpt4.url.contains("api-2") || key_gpt4.url.contains("api-3")); // key 2 or 3 (others)

        // Test fallback to "others"
        let key_other = pool.get_key_for_model("some-random-model").unwrap();
        assert!(key_other.url.contains("api-3")); // should use key 3 (others)
    }

    #[test]
    fn test_no_matching_keys() {
        let keys = vec![
            create_test_key("1", vec!["gpt-3.5-turbo"]),
            create_test_key("2", vec!["gpt-4"]),
        ];

        let pool = KeyPool::new(keys, "round_robin");

        // Test with a model that has no matching keys
        let result = pool.get_key_for_model("claude-1");
        assert!(result.is_none());
    }
}