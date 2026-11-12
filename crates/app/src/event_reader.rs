//! Reads events from an engine or a background job into a view, so each screen does
//! not have to write the same receive loop.

use std::time::Duration;

use gpui::{AsyncApp, Context, Task, WeakEntity};
use smol::channel::Receiver;

/// Waits for events on `events` and hands them to `apply` on the UI thread, taking
/// everything already queued as one batch so the view redraws once per batch. With a
/// `throttle`, it then waits that long before taking the next batch, which keeps a fast
/// engine from redrawing the screen hundreds of times a second. Events keep their order.
///
/// The task ends when the channel closes or the view is dropped.
pub fn spawn_batched<T: 'static, E: 'static>(
    events: Receiver<E>,
    throttle: Option<Duration>,
    cx: &mut Context<T>,
    mut apply: impl FnMut(&mut T, Vec<E>, &mut Context<T>) + 'static,
) -> Task<()> {
    cx.spawn(async move |this: WeakEntity<T>, cx: &mut AsyncApp| {
        while let Ok(first) = events.recv().await {
            let mut batch = vec![first];
            while let Ok(event) = events.try_recv() {
                batch.push(event);
            }
            if this.update(cx, |view, cx| apply(view, batch, cx)).is_err() {
                return;
            }
            if let Some(throttle) = throttle {
                cx.background_executor().timer(throttle).await;
            }
        }
    })
}
