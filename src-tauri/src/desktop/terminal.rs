use crate::terminal::{TerminalService, contracts::*};
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::State;

const MAX_BLOCKING_CALLS: usize = 32;
static BLOCKING_CALLS: AtomicUsize = AtomicUsize::new(0);
struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        BLOCKING_CALLS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Admission occurs before spawn_blocking, so the runtime's blocking queue cannot
/// accumulate unlimited terminal operations. Cancellation does not release a
/// permit until the actual operation finishes.
async fn blocking<T: Send + 'static>(
    service: TerminalService,
    operation: impl FnOnce(TerminalService) -> Result<T, TerminalError> + Send + 'static,
) -> Result<T, TerminalError> {
    BLOCKING_CALLS
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < MAX_BLOCKING_CALLS).then_some(count + 1)
        })
        .map_err(|_| TerminalError::new(ErrorCode::Busy))?;
    let permit = Permit;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        operation(service)
    })
    .await
    .map_err(|error| TerminalError::context(ErrorCode::InternalPty, error))?
}

#[tauri::command]
pub async fn create_terminal(
    service: State<'_, TerminalService>,
    request: CreateTerminal,
) -> Result<TerminalInfo, TerminalError> {
    blocking(service.inner().clone(), move |service| {
        service.create(request)
    })
    .await
}
#[tauri::command]
pub async fn terminal_write(
    service: State<'_, TerminalService>,
    request: WriteTerminal,
) -> Result<usize, TerminalError> {
    blocking(service.inner().clone(), move |service| {
        service.write(&request.terminal_id, request.data)
    })
    .await
}
#[tauri::command]
pub async fn terminal_resize(
    service: State<'_, TerminalService>,
    request: ResizeTerminal,
) -> Result<TerminalInfo, TerminalError> {
    blocking(service.inner().clone(), move |service| {
        service.resize(&request.terminal_id, request.size)
    })
    .await
}
#[tauri::command]
pub async fn close_terminal(
    service: State<'_, TerminalService>,
    request: TerminalTarget,
) -> Result<TerminalInfo, TerminalError> {
    blocking(service.inner().clone(), move |service| {
        service.close(&request.terminal_id)
    })
    .await
}
#[tauri::command]
pub fn terminal_read_output(
    service: State<'_, TerminalService>,
    request: ReadTerminal,
) -> Result<TerminalOutput, TerminalError> {
    service.read(&request.terminal_id, request.ack.as_ref())
}
#[tauri::command]
pub fn terminal_status(
    service: State<'_, TerminalService>,
    request: TerminalTarget,
) -> Result<TerminalInfo, TerminalError> {
    service.status(&request.terminal_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn blocking_operation_does_not_block_runtime_heartbeat() {
        let (entered, observed) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel(1);
        let operation =
            tauri::async_runtime::spawn(blocking(TerminalService::default(), move |_| {
                entered.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(3)).unwrap();
                Ok(())
            }));
        observed.recv_timeout(Duration::from_secs(3)).unwrap();
        let (heartbeat, received) = mpsc::sync_channel(1);
        tauri::async_runtime::spawn(async move {
            heartbeat.send(super::super::get_app_info()).unwrap();
        });
        let result = received.recv_timeout(Duration::from_secs(1));
        release.send(()).unwrap();
        tauri::async_runtime::block_on(operation).unwrap().unwrap();
        assert!(
            result.is_ok(),
            "runtime heartbeat stalled behind blocking operation"
        );
    }
}
