//! Heartbeat keepalive for active Meshtastic connections.
//!
//! A background task that sends an empty [`ToRadio`] every 30 seconds to keep
//! the connection alive. Cancelled via a [`CancellationToken`].
//!
//! The Meshtastic serial and TCP wire protocol does not define a dedicated
//! heartbeat message in the vendored proto definitions; an empty `ToRadio`
//! (no payload variant) serves as the keepalive signal accepted by firmware.

use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use crate::Error;
use crate::connection::MeshConnection;
use crate::proto::ToRadio;

/// Interval between heartbeat transmissions.
pub(crate) const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// Run the heartbeat loop until the `token` is cancelled.
///
/// Sends an empty [`ToRadio`] on `conn` every [`HEARTBEAT_INTERVAL`].
/// Uses [`MissedTickBehavior::Skip`] so a slow `send()` call does not
/// cause burst transmissions after a long pause.
///
/// # Errors
///
/// Returns the first send error encountered.  The caller should cancel the
/// token and reconnect the underlying connection.
pub async fn run_heartbeat<C>(conn: &Mutex<C>, token: CancellationToken) -> Result<(), Error>
where
    C: MeshConnection,
{
    let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            () = token.cancelled() => {
                tracing::debug!("heartbeat task cancelled");
                return Ok(());
            }
            _ = interval.tick() => {
                // WHY: empty ToRadio (no payload_variant) is a no-op that keeps
                // the TCP/serial connection alive at the OS level.
                let heartbeat = ToRadio { payload_variant: None };
                conn.lock().await.send(heartbeat).await?;
                tracing::trace!("heartbeat sent");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::Mutex;

    use super::*;
    use crate::proto::FromRadio;

    /// Mock connection that counts how many times `send()` is called.
    struct CountingConn {
        send_count: usize,
    }

    impl MeshConnection for CountingConn {
        async fn send(&mut self, _: ToRadio) -> Result<(), Error> {
            self.send_count += 1;
            Ok(())
        }

        async fn recv(&mut self) -> Result<FromRadio, Error> {
            std::future::pending().await
        }

        fn is_connected(&self) -> bool {
            true
        }

        async fn reconnect(&mut self) -> Result<(), Error> {
            Ok(())
        }
    }

    #[tokio::test(start_paused = true)]
    async fn heartbeat_fires_at_30_second_interval() {
        let conn = Arc::new(Mutex::new(CountingConn { send_count: 0 }));
        let token = CancellationToken::new();
        let task_token = token.clone();
        let task_conn = Arc::clone(&conn);

        // Spawn the heartbeat task.
        let handle = tokio::spawn(async move { run_heartbeat(&*task_conn, task_token).await });

        // First tick fires at t=0 immediately; advance a tiny amount and yield
        // to let the task process it.
        tokio::time::advance(Duration::from_millis(1)).await;
        tokio::task::yield_now().await;

        let initial_count = conn.lock().await.send_count;
        assert!(
            initial_count >= 1,
            "expected ≥1 heartbeat at t=0, got {initial_count}"
        );

        // Second tick fires at t=30 s.
        tokio::time::advance(Duration::from_secs(30)).await;
        tokio::task::yield_now().await;

        let mid_count = conn.lock().await.send_count;
        assert!(
            mid_count >= 2,
            "expected ≥2 heartbeats after 30 s, got {mid_count}"
        );

        // Third tick fires at t=60 s.
        tokio::time::advance(Duration::from_secs(30)).await;
        tokio::task::yield_now().await;

        let final_count = conn.lock().await.send_count;
        assert!(
            final_count >= 3,
            "expected ≥3 heartbeats after 60 s, got {final_count}"
        );

        // Cancel and wait for the task to finish.
        token.cancel();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn heartbeat_cancels_cleanly() {
        let conn = Arc::new(Mutex::new(CountingConn { send_count: 0 }));
        let token = CancellationToken::new();
        let task_token = token.clone();
        let task_conn = Arc::clone(&conn);

        let handle = tokio::spawn(async move { run_heartbeat(&*task_conn, task_token).await });

        // Cancel immediately — task should exit without error.
        token.cancel();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }
}
