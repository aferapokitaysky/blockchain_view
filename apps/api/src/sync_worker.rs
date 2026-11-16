use std::time::Duration;

use chrono::{DateTime, Utc};
use redis::{
    AsyncCommands,
    streams::{StreamAutoClaimOptions, StreamReadOptions, StreamReadReply},
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tokio::{sync::watch, time};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{ApiState, transfers};

const STREAM_KEY: &str = "tracefold:wallet-sync:stream";
const GROUP_NAME: &str = "wallet-sync-workers";
const CURSOR_KEY: &str = "tracefold:wallet-sync:scan-cursor";
const DEDUPE_PREFIX: &str = "tracefold:wallet-sync:scheduled:";
const FAILURE_PREFIX: &str = "tracefold:wallet-sync:failures:";
const LOCK_PREFIX: &str = "tracefold:wallet-sync:lock:";
const DEFAULT_SCAN_INTERVAL: u64 = 30;
const DEFAULT_SYNC_INTERVAL: u64 = 300;
const SCAN_BATCH_SIZE: usize = 1_000;
const MAX_QUEUE_LENGTH: usize = 10_000;
const MAX_RECLAIM_IDLE: u64 = 600_000;
const MAX_JOB_RUNTIME: Duration = Duration::from_secs(480);
const DEDUPE_TTL_SECONDS: u64 = 86_400;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScanCursor {
    created_at: DateTime<Utc>,
    address_id: Uuid,
}

#[derive(Debug, FromRow)]
struct TrackedAddressCursor {
    id: Uuid,
    investigation_id: Uuid,
    created_at: DateTime<Utc>,
}

#[derive(Debug)]
struct SyncJob {
    stream_id: String,
    investigation_id: Uuid,
    address_id: Uuid,
}

pub async fn run(
    state: ApiState,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), redis::RedisError> {
    initialize_consumer_group(&state.redis).await?;
    let consumer_name = format!("sync-{}", Uuid::new_v4());
    let scheduler_state = state.clone();
    let scheduler_shutdown = shutdown.clone();
    let scheduler = tokio::spawn(async move {
        schedule_due_addresses(scheduler_state, scheduler_shutdown).await;
    });
    let consumer = tokio::spawn(consume_jobs(state, consumer_name, shutdown.clone()));
    if !*shutdown.borrow() {
        let _ = shutdown.changed().await;
    }
    let (scheduler_result, consumer_result) = tokio::join!(scheduler, consumer);
    if scheduler_result.is_err() || consumer_result.is_err() {
        error!("задача фоновой синхронизации завершилась аварийно");
    }
    Ok(())
}

async fn initialize_consumer_group(client: &redis::Client) -> Result<(), redis::RedisError> {
    let mut connection = client.get_multiplexed_async_connection().await?;
    let result: redis::RedisResult<()> = connection
        .xgroup_create_mkstream(STREAM_KEY, GROUP_NAME, "0")
        .await;
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.to_string().contains("BUSYGROUP") => Ok(()),
        Err(error) => Err(error),
    }
}

async fn schedule_due_addresses(state: ApiState, mut shutdown: watch::Receiver<bool>) {
    let mut interval = time::interval(Duration::from_secs(DEFAULT_SCAN_INTERVAL));
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            _ = interval.tick() => {
                if let Err(error) = enqueue_address_batch(&state).await {
                    warn!(error = %error, "не удалось поставить адреса в очередь синхронизации");
                }
            }
        }
    }
}

async fn enqueue_address_batch(
    state: &ApiState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut connection = state.redis.get_multiplexed_async_connection().await?;
    let cursor = connection
        .get::<_, Option<String>>(CURSOR_KEY)
        .await?
        .map(|value| serde_json::from_str::<ScanCursor>(&value))
        .transpose()?;
    let addresses = match &cursor {
        Some(cursor) => {
            sqlx::query_as::<_, TrackedAddressCursor>(
                "SELECT id, investigation_id, created_at FROM tracked_addresses \
             WHERE (created_at, id) > ($1, $2) ORDER BY created_at, id LIMIT $3",
            )
            .bind(cursor.created_at)
            .bind(cursor.address_id)
            .bind(SCAN_BATCH_SIZE as i64)
            .fetch_all(&state.database)
            .await?
        }
        None => {
            sqlx::query_as::<_, TrackedAddressCursor>(
                "SELECT id, investigation_id, created_at FROM tracked_addresses \
             ORDER BY created_at, id LIMIT $1",
            )
            .bind(SCAN_BATCH_SIZE as i64)
            .fetch_all(&state.database)
            .await?
        }
    };

    for address in &addresses {
        let key = format!("{DEDUPE_PREFIX}{}", address.id);
        enqueue_job(
            &mut connection,
            STREAM_KEY,
            &key,
            address.investigation_id,
            address.id,
            MAX_QUEUE_LENGTH,
        )
        .await?;
    }

    if addresses.len() < SCAN_BATCH_SIZE {
        let _: usize = connection.del(CURSOR_KEY).await?;
    } else if let Some(last) = addresses.last() {
        let next = ScanCursor {
            created_at: last.created_at,
            address_id: last.id,
        };
        let _: () = connection
            .set(CURSOR_KEY, serde_json::to_string(&next)?)
            .await?;
    }
    Ok(())
}

async fn enqueue_job(
    connection: &mut redis::aio::MultiplexedConnection,
    stream_key: &str,
    dedupe_key: &str,
    investigation_id: Uuid,
    address_id: Uuid,
    queue_limit: usize,
) -> Result<bool, redis::RedisError> {
    let script = redis::Script::new(
        "if redis.call('XLEN', KEYS[2]) >= tonumber(ARGV[1]) then return 0 end; \
         if redis.call('SET', KEYS[1], '1', 'NX', 'EX', ARGV[2]) then \
           redis.call('XADD', KEYS[2], '*', 'investigation_id', ARGV[3], 'address_id', ARGV[4]); return 1 \
         end; return 0",
    );
    let queued: i32 = script
        .key(dedupe_key)
        .key(stream_key)
        .arg(queue_limit)
        .arg(DEDUPE_TTL_SECONDS)
        .arg(investigation_id.to_string())
        .arg(address_id.to_string())
        .invoke_async(connection)
        .await?;
    Ok(queued == 1)
}

async fn consume_jobs(state: ApiState, consumer: String, mut shutdown: watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow() {
            break;
        }
        let next = tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
                continue;
            }
            result = next_job(&state.redis, &consumer) => result,
        };
        match next {
            Ok(Some(job)) => process_job(&state, job).await,
            Ok(None) => {}
            Err(error) => {
                warn!(error = %error, "ошибка очереди синхронизации; повтор подключения через 2 секунды");
                tokio::select! {
                    _ = time::sleep(Duration::from_secs(2)) => {},
                    changed = shutdown.changed() => if changed.is_err() || *shutdown.borrow() { break; },
                }
            }
        }
    }
}

async fn next_job(
    client: &redis::Client,
    consumer: &str,
) -> Result<Option<SyncJob>, redis::RedisError> {
    let mut connection = client.get_multiplexed_async_connection().await?;
    let claimed: redis::streams::StreamAutoClaimReply = connection
        .xautoclaim_options(
            STREAM_KEY,
            GROUP_NAME,
            consumer,
            MAX_RECLAIM_IDLE,
            "0-0",
            StreamAutoClaimOptions::default().count(1),
        )
        .await?;
    if let Some(job) = claimed.claimed.into_iter().next() {
        return match parse_job(job.clone()) {
            Ok(job) => Ok(Some(job)),
            Err(error) => {
                discard_job(&mut connection, &job.id).await?;
                Err(error)
            }
        };
    }

    let options = StreamReadOptions::default()
        .group(GROUP_NAME, consumer)
        .count(1)
        .block(1_000);
    let reply: StreamReadReply = connection
        .xread_options(&[STREAM_KEY], &[">"], &options)
        .await?;
    let Some(id) = reply.keys.into_iter().flat_map(|key| key.ids).next() else {
        return Ok(None);
    };
    match parse_job(id.clone()) {
        Ok(job) => Ok(Some(job)),
        Err(error) => {
            discard_job(&mut connection, &id.id).await?;
            Err(error)
        }
    }
}

async fn discard_job(
    connection: &mut redis::aio::MultiplexedConnection,
    stream_id: &str,
) -> Result<(), redis::RedisError> {
    acknowledge_and_delete(connection, STREAM_KEY, GROUP_NAME, stream_id).await?;
    warn!(
        stream_id,
        "повреждённая задача синхронизации удалена из очереди"
    );
    Ok(())
}

fn parse_job(id: redis::streams::StreamId) -> Result<SyncJob, redis::RedisError> {
    let field = |name: &str| -> Result<String, redis::RedisError> {
        let value = id.map.get(name).ok_or_else(|| {
            redis::RedisError::from((redis::ErrorKind::TypeError, "missing sync job field"))
        })?;
        redis::from_redis_value(value)
    };
    let parse_uuid = |name: &str| -> Result<Uuid, redis::RedisError> {
        field(name)?.parse().map_err(|_| {
            redis::RedisError::from((redis::ErrorKind::TypeError, "invalid sync job identifier"))
        })
    };
    Ok(SyncJob {
        stream_id: id.id,
        investigation_id: parse_uuid("investigation_id")?,
        address_id: parse_uuid("address_id")?,
    })
}

async fn process_job(state: &ApiState, job: SyncJob) {
    let lock_key = format!("{LOCK_PREFIX}{}", job.address_id);
    let dedupe_key = format!("{DEDUPE_PREFIX}{}", job.address_id);
    let failure_key = format!("{FAILURE_PREFIX}{}", job.address_id);
    let worker_id = Uuid::new_v4().to_string();
    let result = process_locked_job(state, &job, &lock_key, &worker_id).await;
    let mut connection = match state.redis.get_multiplexed_async_connection().await {
        Ok(connection) => connection,
        Err(error) => {
            error!(error = %error, "не удалось подтвердить обработанную задачу синхронизации");
            return;
        }
    };

    match result {
        Ok(true) => {
            if let Err(error) = connection.del::<_, usize>(&failure_key).await {
                warn!(error = %error, address_id = %job.address_id, "не удалось сбросить счётчик повторов");
            }
            if let Err(error) = connection
                .expire::<_, bool>(&dedupe_key, DEFAULT_SYNC_INTERVAL as i64)
                .await
            {
                warn!(error = %error, address_id = %job.address_id, "не удалось обновить срок дедупликации");
            }
            info!(address_id = %job.address_id, "история кошелька синхронизирована");
        }
        Ok(false) => {
            info!(address_id = %job.address_id, "повторная задача пропущена: кошелёк уже синхронизируется");
        }
        Err(error) => {
            let attempts: u32 = match connection.incr(&failure_key, 1).await {
                Ok(attempts) => attempts,
                Err(redis_error) => {
                    warn!(error = %redis_error, "не удалось сохранить число повторов синхронизации");
                    1
                }
            };
            if let Err(redis_error) = connection
                .expire::<_, bool>(&failure_key, 30 * 24 * 60 * 60)
                .await
            {
                warn!(error = %redis_error, "не удалось ограничить срок хранения счётчика повторов");
            }
            let retry_seconds = retry_delay_seconds(attempts);
            if let Err(redis_error) = connection
                .expire::<_, bool>(&dedupe_key, retry_seconds as i64)
                .await
            {
                warn!(error = %redis_error, address_id = %job.address_id, "не удалось установить задержку повтора");
            }
            warn!(address_id = %job.address_id, attempt = attempts, retry_seconds, error = %error, "синхронизация кошелька завершилась ошибкой");
        }
    }
    match acknowledge_and_delete(&mut connection, STREAM_KEY, GROUP_NAME, &job.stream_id).await {
        Ok(acknowledged) if acknowledged > 0 => {
            info!(stream_id = %job.stream_id, "задача синхронизации подтверждена и удалена");
        }
        Ok(_) => {}
        Err(error) => {
            warn!(error = %error, stream_id = %job.stream_id, "не удалось подтвердить задачу синхронизации")
        }
    }
    release_lock(&mut connection, &lock_key, &worker_id).await;
}

async fn acknowledge_and_delete(
    connection: &mut redis::aio::MultiplexedConnection,
    stream_key: &str,
    group: &str,
    stream_id: &str,
) -> Result<usize, redis::RedisError> {
    let script = redis::Script::new(
        "local acked = redis.call('XACK', KEYS[1], ARGV[1], ARGV[2]); \
         if acked > 0 then redis.call('XDEL', KEYS[1], ARGV[2]); end; return acked",
    );
    script
        .key(stream_key)
        .arg(group)
        .arg(stream_id)
        .invoke_async(connection)
        .await
}

async fn process_locked_job(
    state: &ApiState,
    job: &SyncJob,
    lock_key: &str,
    lock_value: &str,
) -> Result<bool, String> {
    let mut connection = state
        .redis
        .get_multiplexed_async_connection()
        .await
        .map_err(|error| error.to_string())?;
    let acquired: Option<String> = redis::cmd("SET")
        .arg(lock_key)
        .arg(lock_value)
        .arg("NX")
        .arg("EX")
        .arg(MAX_JOB_RUNTIME.as_secs() as usize + 60)
        .query_async(&mut connection)
        .await
        .map_err(|error: redis::RedisError| error.to_string())?;
    if acquired.is_none() {
        return Ok(false);
    }

    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM tracked_addresses WHERE id = $1 AND investigation_id = $2)",
    )
    .bind(job.address_id)
    .bind(job.investigation_id)
    .fetch_one(&state.database)
    .await
    .map_err(|error| error.to_string())?;
    if exists {
        tokio::time::timeout(
            MAX_JOB_RUNTIME,
            transfers::sync_tracked_address(state, job.investigation_id, job.address_id),
        )
        .await
        .map_err(|_| "sync job timed out".to_owned())?
        .map_err(|error| error.to_string())?;
    }
    Ok(true)
}

async fn release_lock(connection: &mut redis::aio::MultiplexedConnection, key: &str, value: &str) {
    let script = redis::Script::new(
        "if redis.call('GET', KEYS[1]) == ARGV[1] then return redis.call('DEL', KEYS[1]) end; return 0",
    );
    let result: Result<i32, redis::RedisError> =
        script.key(key).arg(value).invoke_async(connection).await;
    if let Err(error) = result {
        warn!(error = %error, "не удалось освободить блокировку кошелька");
    }
}

fn retry_delay_seconds(attempt: u32) -> u64 {
    let exponent = attempt.saturating_sub(1).min(8);
    30_u64.saturating_mul(2_u64.pow(exponent)).min(3_600)
}

#[cfg(test)]
mod tests {
    use redis::{
        AsyncCommands,
        streams::{StreamAutoClaimOptions, StreamReadOptions, StreamReadReply},
    };
    use uuid::Uuid;

    use super::{
        MAX_QUEUE_LENGTH, SCAN_BATCH_SIZE, acknowledge_and_delete, enqueue_job, retry_delay_seconds,
    };

    #[test]
    fn retry_backoff_is_exponential_and_capped_at_one_hour() {
        assert_eq!(retry_delay_seconds(1), 30);
        assert_eq!(retry_delay_seconds(2), 60);
        assert_eq!(retry_delay_seconds(6), 960);
        assert_eq!(retry_delay_seconds(20), 3_600);
    }

    #[test]
    fn scheduler_and_queue_have_explicit_work_limits() {
        assert_eq!(SCAN_BATCH_SIZE, 1_000);
        assert_eq!(MAX_QUEUE_LENGTH, 10_000);
    }

    #[tokio::test]
    async fn enqueue_is_atomic_deduplicated_and_queue_is_bounded() {
        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:56379".into());
        let client = redis::Client::open(redis_url).expect("Redis URL must be valid");
        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .expect("Redis must be available for the queue integration test");
        let suffix = Uuid::new_v4();
        let stream = format!("tracefold:test:sync:{suffix}");
        let dedupe = format!("tracefold:test:dedupe:{suffix}");
        let second_dedupe = format!("{dedupe}:second");
        let investigation_id = Uuid::new_v4();
        let first_address_id = Uuid::new_v4();

        assert!(
            enqueue_job(
                &mut connection,
                &stream,
                &dedupe,
                investigation_id,
                first_address_id,
                1,
            )
            .await
            .expect("the first job should be enqueued")
        );
        assert!(
            !enqueue_job(
                &mut connection,
                &stream,
                &dedupe,
                investigation_id,
                first_address_id,
                1,
            )
            .await
            .expect("duplicate jobs should be ignored")
        );
        assert!(
            !enqueue_job(
                &mut connection,
                &stream,
                &second_dedupe,
                investigation_id,
                Uuid::new_v4(),
                1,
            )
            .await
            .expect("the queue limit should be enforced")
        );
        let queued: usize = connection
            .xlen(&stream)
            .await
            .expect("stream length should be readable");
        assert_eq!(queued, 1);
        let group = format!("workers:{suffix}");
        let _: () = connection
            .xgroup_create_mkstream(&stream, &group, "0")
            .await
            .expect("consumer group should be created");
        let read: StreamReadReply = connection
            .xread_options(
                &[stream.as_str()],
                &[">"],
                &StreamReadOptions::default()
                    .group(&group, "worker-a")
                    .count(1),
            )
            .await
            .expect("the first consumer should receive the queued job");
        let pending_id = read.keys[0].ids[0].id.clone();
        let reclaimed: redis::streams::StreamAutoClaimReply = connection
            .xautoclaim_options(
                &stream,
                &group,
                "worker-b",
                0,
                "0-0",
                StreamAutoClaimOptions::default().count(1),
            )
            .await
            .expect("another consumer should reclaim the pending job");
        let claimed = reclaimed
            .claimed
            .into_iter()
            .next()
            .expect("pending job should be claimed");
        let parsed = super::parse_job(claimed).expect("reclaimed job should remain parseable");
        assert_eq!(parsed.address_id, first_address_id);
        let acked = acknowledge_and_delete(&mut connection, &stream, &group, &pending_id)
            .await
            .expect("the reclaimed job should be acknowledged");
        assert_eq!(acked, 1);
        let remaining: usize = connection
            .xlen(&stream)
            .await
            .expect("the acknowledged job should be removed");
        assert_eq!(remaining, 0);
        let _: usize = connection
            .xgroup_destroy(&stream, &group)
            .await
            .expect("the test consumer group should be removed");
        for key in [&stream, &dedupe, &second_dedupe] {
            let _: usize = connection
                .del(key)
                .await
                .expect("test keys should be removed");
        }
    }
}
