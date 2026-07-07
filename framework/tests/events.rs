//! EventBus wire-contract tests: batching, framing, keep-alive.
//!
//! The batch format is a MessagePack array of `[channel, value]` pairs — the
//! exact bytes `@elyra/runtime` decodes off the `/__events` long-poll.

use std::time::Duration;

use elyra::EventBus;

#[tokio::test]
async fn coalesces_emits_into_one_batch() {
    let bus = EventBus::new();

    // Three emits before anyone polls -> a single batch.
    bus.emit("tick", &1u32).unwrap();
    bus.emit("tick", &2u32).unwrap();
    bus.emit("tick", &3u32).unwrap();

    let buf = bus.next_batch().await;
    let batch: Vec<(String, u32)> = rmp_serde::from_slice(&buf).unwrap();

    assert_eq!(
        batch,
        vec![
            ("tick".to_string(), 1),
            ("tick".to_string(), 2),
            ("tick".to_string(), 3),
        ]
    );
}

#[tokio::test]
async fn poll_wakes_on_emit() {
    let bus = EventBus::new();

    // Poll first (empty), emit from a task, expect the batch to arrive.
    let emitter = bus.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        emitter.emit("late", &"hi").unwrap();
    });

    let buf = bus.next_batch().await;
    let batch: Vec<(String, String)> = rmp_serde::from_slice(&buf).unwrap();
    assert_eq!(batch, vec![("late".to_string(), "hi".to_string())]);
}

#[tokio::test]
async fn batch_window_still_flushes() {
    // A non-zero window must still deliver, just after a short delay.
    let bus = EventBus::with_batch_window(Duration::from_millis(5));
    bus.emit("x", &42u32).unwrap();

    let buf = bus.next_batch().await;
    let batch: Vec<(String, u32)> = rmp_serde::from_slice(&buf).unwrap();
    assert_eq!(batch, vec![("x".to_string(), 42)]);
}
