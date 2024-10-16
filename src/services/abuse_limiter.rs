use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use fred::{
    prelude::{KeysInterface, RedisPool, SortedSetsInterface},
    types::Expiration,
};
use serde_json::json;

// const ATTEMPT_LIMIT_6_MINUTES: usize = 3;
// const ATTEMPT_LIMIT_15_MINUTES: usize = 5;
// const BLOCK_DURATION_1_HOUR: usize = 3600; // in seconds
// const BLOCK_DURATION_24_HOURS: usize = 86400; // in seconds

pub struct AbuseLimiterConfig {
    key_prefix: String,
    temp_block_attempts: usize,
    temp_block_range: usize,
    temp_block_duration: usize,
    block_retry_limit: usize,
    block_range: usize,
    block_duration: usize,
}

pub async fn limiter(redis_pool: &RedisPool, config: AbuseLimiterConfig) -> Result<(), Response> {
    let block_key = format!("abuse_limiter:block:{}", config.key_prefix);
    let is_blocked: Option<String> = redis_pool.get(&block_key).await.unwrap();
    if let Some(blocked_until) = is_blocked {
        let blocked_until: usize = blocked_until.parse().unwrap();
        let current_time = chrono::Utc::now().timestamp() as usize;
        if current_time < blocked_until {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "error": "Too many attempts",
                    "message": "You have been temporarily blocked due to too many verification attempts. Please try again later",
                })),
            ).into_response());
        } else {
            // Unblock the user if the block duration has passed
            let _: () = redis_pool.del(&block_key).await.unwrap();
        }
    }

    // Track the number of attempts
    let attempt_key = format!("abuse_limiter:attempts:{}", config.key_prefix);
    let current_time = chrono::Utc::now().timestamp() as usize;
    let _: () = redis_pool
        .zadd(
            &attempt_key,
            None,
            None,
            false,
            false,
            (current_time as f64, current_time as f64),
        )
        .await
        .unwrap();
    let _: () = redis_pool
        .expire(&attempt_key, config.block_duration as i64)
        .await
        .unwrap();

    let temp_block_attempts: usize = redis_pool
        .zcount(
            &attempt_key,
            (current_time - config.temp_block_range) as f64,
            current_time as f64,
        )
        .await
        .unwrap();
    let block_attempts: usize = redis_pool
        .zcount(
            &attempt_key,
            (current_time - config.block_range) as f64,
            current_time as f64,
        )
        .await
        .unwrap();

    // Block the user if the limits are exceeded
    if temp_block_attempts > config.temp_block_attempts {
        let block_until = current_time + config.temp_block_duration;
        let _: () = redis_pool
            .set(
                &block_key,
                block_until as i64,
                Some(Expiration::EX(config.temp_block_duration as i64)),
                None,
                false,
            )
            .await
            .unwrap();
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "Too many attempts",
                "message": "You have been temporarily blocked due to too many verification attempts. Please try again in an hour.",
            })),
        ).into_response());
    }

    if block_attempts > config.block_retry_limit {
        let block_until = current_time + config.block_duration;
        let _: () = redis_pool
            .set(
                &block_key,
                block_until as i64,
                Some(Expiration::EX(config.block_duration as i64)),
                None,
                false,
            )
            .await
            .unwrap();
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "Too many attempts",
                "message": "You have been temporarily blocked due to too many verification attempts. Please try again in 24 hours.",
            })),
        ).into_response());
    }

    Ok(())
}
