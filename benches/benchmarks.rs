use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use key_cycle_proxy::{
    config::{ApiKeyInfo, UpstreamConfig},
    proxy::{KeyPool, ProxyEngine, ProxyHandler, UpstreamClient},
    routes::create_router,
    types::OpenAIRequest,
};
use secrecy::SecretString;
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::runtime::Runtime;
use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

fn create_test_keys(count: usize) -> Vec<ApiKeyInfo> {
    (0..count)
        .map(|i| ApiKeyInfo {
            key: SecretString::new(format!("sk-bench-key-{}", i)),
            url: format!("https://api-{}.test.com", i),
            models: vec!["gpt-3.5-turbo".to_string(), "others".to_string()],
            latency: Some(Duration::from_millis(50 + i as u64 * 10)),
            health_score: 1.0 - (i as f64 * 0.1),
        })
        .collect()
}

fn bench_key_selection(c: &mut Criterion) {
    let mut group = c.benchmark_group("key_selection");
    
    for key_count in [5, 10, 25, 50, 100].iter() {
        let keys = create_test_keys(*key_count);
        let pool = KeyPool::new(keys, "round_robin");
        
        group.bench_with_input(
            BenchmarkId::new("round_robin", key_count),
            key_count,
            |b, _| {
                b.iter(|| {
                    black_box(pool.get_key_for_model("gpt-3.5-turbo"))
                })
            },
        );
        
        let keys = create_test_keys(*key_count);
        let pool = KeyPool::new(keys, "least_latency");
        
        group.bench_with_input(
            BenchmarkId::new("least_latency", key_count),
            key_count,
            |b, _| {
                b.iter(|| {
                    black_box(pool.get_key_for_model("gpt-3.5-turbo"))
                })
            },
        );
    }
    
    group.finish();
}

fn bench_json_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_parsing");
    
    let simple_request = json!({
        "model": "gpt-3.5-turbo",
        "messages": [
            {"role": "user", "content": "Hello!"}
        ]
    }).to_string();
    
    let complex_request = json!({
        "model": "gpt-4-turbo",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "Write a long story about artificial intelligence and its impact on society."},
            {"role": "assistant", "content": "Once upon a time in a world not so different from ours..."},
            {"role": "user", "content": "Continue the story and make it more detailed."}
        ],
        "temperature": 0.7,
        "max_tokens": 2048,
        "top_p": 0.9,
        "frequency_penalty": 0.1,
        "presence_penalty": 0.1,
        "stop": ["\n\n", "END"],
        "stream": false,
        "user": "benchmark-user-123"
    }).to_string();
    
    group.bench_function("simple_request", |b| {
        b.iter(|| {
            let request: OpenAIRequest = serde_json::from_str(black_box(&simple_request)).unwrap();
            black_box(request)
        })
    });
    
    group.bench_function("complex_request", |b| {
        b.iter(|| {
            let request: OpenAIRequest = serde_json::from_str(black_box(&complex_request)).unwrap();
            black_box(request)
        })
    });
    
    group.finish();
}

fn bench_concurrent_key_access(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("concurrent_access");
    group.sample_size(50); // Reduce sample size for concurrent tests
    
    let keys = create_test_keys(10);
    let pool = Arc::new(KeyPool::new(keys, "round_robin"));
    
    group.bench_function("sequential_access", |b| {
        b.iter(|| {
            for _ in 0..100 {
                black_box(pool.get_key_for_model("gpt-3.5-turbo"));
            }
        })
    });
    
    group.bench_function("concurrent_access", |b| {
        b.to_async(&rt).iter(|| async {
            let mut handles = vec![];
            
            for _ in 0..10 {
                let pool_clone = pool.clone();
                let handle = tokio::spawn(async move {
                    for _ in 0..10 {
                        black_box(pool_clone.get_key_for_model("gpt-3.5-turbo"));
                    }
                });
                handles.push(handle);
            }
            
            futures::future::join_all(handles).await
        })
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_key_selection,
    bench_json_parsing,
    bench_concurrent_key_access
);
criterion_main!(benches);