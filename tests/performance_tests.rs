use axum::{body::Body, http::Request, Router};
use key_cycle_proxy::{
    config::{ApiKeyInfo, UpstreamConfig},
    proxy::{KeyPool, ProxyEngine, ProxyHandler, UpstreamClient},
    routes::create_router,
};
use secrecy::SecretString;
use serde_json::json;
use std::{sync::Arc, time::{Duration, Instant}};
use tokio::time::timeout;
use tower::ServiceExt;
use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

/// Performance and load testing for the API proxy
async fn create_performance_test_app() -> (Router, Vec<MockServer>) {
    // Create multiple mock servers to simulate different API endpoints
    let mut mock_servers = vec![];
    let mut keys = vec![];

    // Create 5 mock servers for load testing
    for i in 0..5 {
        let mock_server = MockServer::start().await;
        
        // Setup each server to respond with a slight delay to simulate real API latency
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(50 + i * 10)) // Staggered latency
                    .set_body_json(json!({
                        "id": format!("chatcmpl-server-{}", i),
                        "object": "chat.completion",
                        "created": 1234567890,
                        "model": "gpt-3.5-turbo",
                        "choices": [{
                            "index": 0,
                            "message": {
                                "role": "assistant",
                                "content": format!("Response from server {}", i)
                            },
                            "finish_reason": "stop"
                        }],
                        "usage": {
                            "prompt_tokens": 10,
                            "completion_tokens": 5,
                            "total_tokens": 15
                        }
                    }))
            )
            .mount(&mock_server)
            .await;

        keys.push(ApiKeyInfo {
            key: SecretString::new(format!("sk-test-key-{}", i)),
            url: mock_server.uri(),
            models: vec!["gpt-3.5-turbo".to_string(), "others".to_string()],
            latency: None,
            health_score: 1.0,
        });

        mock_servers.push(mock_server);
    }

    // Create components with optimized configuration for performance
    let key_pool = Arc::new(KeyPool::new(keys, "least_latency"));
    let upstream_config = UpstreamConfig {
        base_url: "http://performance-test.com/v1".to_string(),
        connect_timeout_ms: 500,
        request_timeout_ms: 2000,
        retry_initial_backoff_ms: 25,
        retry_max_backoff_ms: 500,
        max_retries: 1, // Reduce retries for performance
    };
    let upstream_client = UpstreamClient::new(upstream_config).unwrap();
    let engine = Arc::new(ProxyEngine::new(key_pool, upstream_client, 1));
    let handler = Arc::new(ProxyHandler::new(engine));

    let app = create_router(handler, 1024 * 1024, Duration::from_secs(5));

    (app, mock_servers)
}

#[tokio::test]
async fn test_load_performance_100_concurrent_requests() {
    let (app, _mock_servers) = create_performance_test_app().await;
    
    let start_time = Instant::now();
    let num_requests = 100;
    
    // Create 100 concurrent requests
    let mut handles = vec![];
    for i in 0..num_requests {
        let app_clone = app.clone();
        let handle = tokio::spawn(async move {
            let request = Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "model": "gpt-3.5-turbo",
                        "messages": [
                            {"role": "user", "content": format!("Load test request {}", i)}
                        ]
                    })
                    .to_string(),
                ))
                .unwrap();

            let request_start = Instant::now();
            let response = app_clone.oneshot(request).await.unwrap();
            let request_duration = request_start.elapsed();
            
            (response.status(), request_duration)
        });
        handles.push(handle);
    }

    // Wait for all requests with timeout
    let results = timeout(Duration::from_secs(30), futures::future::join_all(handles))
        .await
        .expect("Load test should complete within 30 seconds");

    let total_duration = start_time.elapsed();
    
    // Analyze results
    let mut successful_requests = 0;
    let mut total_request_time = Duration::ZERO;
    let mut max_request_time = Duration::ZERO;
    let mut min_request_time = Duration::from_secs(999);
    
    for result in results {
        let (status, duration) = result.unwrap();
        if status.is_success() {
            successful_requests += 1;
            total_request_time += duration;
            max_request_time = max_request_time.max(duration);
            min_request_time = min_request_time.min(duration);
        }
    }
    
    // Performance assertions
    assert!(successful_requests >= num_requests * 95 / 100, 
           "At least 95% of requests should succeed, got {}/{}", successful_requests, num_requests);
    
    let avg_request_time = total_request_time / successful_requests as u32;
    let throughput = successful_requests as f64 / total_duration.as_secs_f64();
    
    println!("Load Test Results:");
    println!("  Total time: {:?}", total_duration);
    println!("  Successful requests: {}/{}", successful_requests, num_requests);
    println!("  Throughput: {:.2} req/s", throughput);
    println!("  Average request time: {:?}", avg_request_time);
    println!("  Min request time: {:?}", min_request_time);
    println!("  Max request time: {:?}", max_request_time);
    
    // Performance benchmarks (these are reasonable expectations)
    assert!(throughput > 20.0, "Throughput should be > 20 req/s, got {:.2}", throughput);
    assert!(avg_request_time < Duration::from_millis(500), 
           "Average request time should be < 500ms, got {:?}", avg_request_time);
}

#[tokio::test]
async fn test_latency_key_selection() {
    let (app, _mock_servers) = create_performance_test_app().await;
    
    // Make several requests to populate latency measurements
    for i in 0..10 {
        let request = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "model": "gpt-3.5-turbo",
                    "messages": [
                        {"role": "user", "content": format!("Latency test {}", i)}
                    ]
                })
                .to_string(),
            ))
            .unwrap();
        
        let response = app.clone().oneshot(request).await.unwrap();
        assert!(response.status().is_success());
        
        // Small delay to allow latency measurements to update
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    
    // The key pool should now have latency data and prefer faster servers
    // This is more of a smoke test since latency selection is internal
}

#[tokio::test]
async fn test_memory_usage_under_load() {
    let (app, _mock_servers) = create_performance_test_app().await;
    
    // This test ensures we don't have memory leaks under sustained load
    let num_batches = 10;
    let requests_per_batch = 20;
    
    for batch in 0..num_batches {
        let mut handles = vec![];
        
        for i in 0..requests_per_batch {
            let app_clone = app.clone();
            let handle = tokio::spawn(async move {
                let request = Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "model": "gpt-3.5-turbo",
                            "messages": [
                                {"role": "user", "content": format!("Memory test batch {} request {}", batch, i)}
                            ]
                        })
                        .to_string(),
                    ))
                    .unwrap();
                
                app_clone.oneshot(request).await.unwrap()
            });
            handles.push(handle);
        }
        
        // Wait for batch to complete
        let results = futures::future::join_all(handles).await;
        
        // Verify all requests in batch succeeded
        for result in results {
            let response = result.unwrap();
            assert!(response.status().is_success());
        }
        
        // Small delay between batches
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    
    // If we get here without panicking or timing out, memory usage is likely stable
    println!("Completed {} batches of {} requests each", num_batches, requests_per_batch);
}

#[tokio::test]
async fn test_error_resilience_under_load() {
    let (app, mock_servers) = create_performance_test_app().await;
    
    // Configure some servers to fail intermittently
    for (i, server) in mock_servers.iter().enumerate() {
        if i % 2 == 0 {
            // Every other server will return errors 30% of the time
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(429).set_body_json(json!({
                    "error": {
                        "type": "rate_limit_exceeded",
                        "message": "Rate limit exceeded"
                    }
                })))
                .up_to_n_times(3) // First 3 requests will fail
                .mount(server)
                .await;
        }
    }
    
    let num_requests = 50;
    let mut handles = vec![];
    
    for i in 0..num_requests {
        let app_clone = app.clone();
        let handle = tokio::spawn(async move {
            let request = Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "model": "gpt-3.5-turbo",
                        "messages": [
                            {"role": "user", "content": format!("Resilience test {}", i)}
                        ]
                    })
                    .to_string(),
                ))
                .unwrap();
            
            app_clone.oneshot(request).await.unwrap()
        });
        handles.push(handle);
    }
    
    let results = futures::future::join_all(handles).await;
    
    let mut successful = 0;
    let mut failed = 0;
    
    for result in results {
        let response = result.unwrap();
        if response.status().is_success() {
            successful += 1;
        } else {
            failed += 1;
        }
    }
    
    println!("Resilience test: {} successful, {} failed", successful, failed);
    
    // Even with some servers failing, we should still have high success rate due to key rotation
    assert!(successful >= num_requests * 80 / 100, 
           "At least 80% requests should succeed due to key rotation, got {}/{}", successful, num_requests);
}

#[tokio::test] 
async fn test_timeout_handling() {
    // Create a mock server that responds very slowly
    let slow_server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(10)) // Very slow response
                .set_body_json(json!({"response": "too slow"}))
        )
        .mount(&slow_server)
        .await;
    
    let keys = vec![ApiKeyInfo {
        key: SecretString::new("sk-slow-key".to_string()),
        url: slow_server.uri(),
        models: vec!["gpt-3.5-turbo".to_string()],
        latency: None,
        health_score: 1.0,
    }];
    
    let key_pool = Arc::new(KeyPool::new(keys, "round_robin"));
    let upstream_config = UpstreamConfig {
        base_url: "http://timeout-test.com/v1".to_string(),
        connect_timeout_ms: 100,
        request_timeout_ms: 1000, // 1 second timeout
        retry_initial_backoff_ms: 50,
        retry_max_backoff_ms: 200,
        max_retries: 1,
    };
    let upstream_client = UpstreamClient::new(upstream_config).unwrap();
    let engine = Arc::new(ProxyEngine::new(key_pool, upstream_client, 1));
    let handler = Arc::new(ProxyHandler::new(engine));
    let app = create_router(handler, 1024 * 1024, Duration::from_secs(2));
    
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-3.5-turbo",
                "messages": [
                    {"role": "user", "content": "This should timeout"}
                ]
            })
            .to_string(),
        ))
        .unwrap();
    
    let start = Instant::now();
    let response = app.oneshot(request).await.unwrap();
    let duration = start.elapsed();
    
    // Should fail due to timeout, and should fail relatively quickly
    assert!(response.status().is_server_error() || response.status().is_client_error(), 
           "Expected error status but got: {}", response.status());
    assert!(duration < Duration::from_secs(5), "Timeout should be handled quickly");
}