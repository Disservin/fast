//! Reads events from an engine or a background job into a view, so each screen does
//! not have to write the same receive loop.

use std::time::Duration;

use gpui::{AppContext as _, AsyncApp, Context, Task, WeakEntity};
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

/// Runs `job` on a background thread, then hands its result to `done` on the UI thread
/// and redraws the view. Dropping the task stops listening; the job runs to its end.
pub fn spawn_job<T: 'static, R: Send + 'static>(
    cx: &mut Context<T>,
    job: impl FnOnce() -> R + Send + 'static,
    done: impl FnOnce(&mut T, R, &mut Context<T>) + 'static,
) -> Task<()> {
    let worker = cx.background_spawn(async move { job() });
    cx.spawn(async move |this: WeakEntity<T>, cx: &mut AsyncApp| {
        let result = worker.await;
        let _ = this.update(cx, |view, cx| {
            done(view, result, cx);
            cx.notify();
        });
    })
}

/// Runs `job` on a background thread and shows each progress message it reports through
/// `progress`, then hands its result to `done` on the UI thread. Dropping the task stops
/// listening at once; the job itself runs until it notices its own cancel flag.
pub fn spawn_with_progress<T: 'static, R: Send + 'static>(
    cx: &mut Context<T>,
    job: impl FnOnce(&(dyn Fn(String) + Sync)) -> R + Send + 'static,
    progress: impl Fn(&mut T, String, &mut Context<T>) + 'static,
    done: impl FnOnce(&mut T, R, &mut Context<T>) + 'static,
) -> Task<()> {
    let (sender, receiver) = smol::channel::unbounded::<String>();
    let worker = cx.background_spawn(async move {
        job(&move |message| {
            let _ = sender.try_send(message);
        })
    });
    cx.spawn(async move |this: WeakEntity<T>, cx: &mut AsyncApp| {
        while let Ok(message) = receiver.recv().await {
            if this
                .update(cx, |view, cx| progress(view, message, cx))
                .is_err()
            {
                return;
            }
        }
        let result = worker.await;
        let _ = this.update(cx, |view, cx| done(view, result, cx));
    })
}
