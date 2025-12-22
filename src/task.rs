//! Task polling utilities
//!
//! Provides a generic helper for polling async tasks spawned on the tokio runtime.

use futures::FutureExt;
use tokio::task::JoinHandle;

/// Result of polling a task
pub enum PollResult<T> {
    /// No task to poll (task was None)
    NoTask,
    /// Task is still running
    Pending,
    /// Task completed with result (may be Ok or join error)
    Complete(Result<T, tokio::task::JoinError>),
}

/// Poll an optional task handle and return its result if finished.
///
/// This helper encapsulates the common pattern of:
/// 1. Checking if a task exists
/// 2. Checking if it's finished
/// 3. Taking ownership and extracting the result with `now_or_never()`
///
/// # Returns
/// - `PollResult::NoTask` if task is None
/// - `PollResult::Pending` if task is still running
/// - `PollResult::Complete(result)` if task is finished
///
/// # Example
/// ```ignore
/// match poll_task(&mut self.update_task) {
///     PollResult::Complete(Ok(Ok(()))) => { /* success */ }
///     PollResult::Complete(Ok(Err(e))) => { /* task returned error */ }
///     PollResult::Complete(Err(e)) => { /* task panicked */ }
///     PollResult::Pending => ctx.request_repaint(),
///     PollResult::NoTask => {}
/// }
/// ```
pub fn poll_task<T>(task: &mut Option<JoinHandle<T>>) -> PollResult<T> {
    let Some(handle) = task else {
        return PollResult::NoTask;
    };

    if !handle.is_finished() {
        return PollResult::Pending;
    }

    let handle = task.take().unwrap();
    match handle.now_or_never() {
        Some(result) => PollResult::Complete(result),
        None => {
            // Shouldn't happen since we checked is_finished()
            tracing::warn!("Task not ready despite is_finished()");
            PollResult::Pending
        }
    }
}
