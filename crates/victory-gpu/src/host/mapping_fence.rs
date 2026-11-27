//! Confirmation of the copy-queue marker that orders new GPU mappings.

use std::sync::OnceLock;
use std::time::Instant;

use windows::Win32::Foundation::{
    E_UNEXPECTED, ERROR_TIMEOUT, HANDLE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Graphics::Dxgi::DXGI_ERROR_DEVICE_REMOVED;
use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};
use windows::core::{Error, HRESULT, Result};

use super::{ID3D12CommandQueue, ID3D12Device, ID3D12Fence};

const MAPPING_TIMEOUT: HRESULT = HRESULT::from_win32(ERROR_TIMEOUT.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Attempt {
    Initial,
    Retry,
    AfterTimeout,
}

#[derive(Default)]
pub(super) struct Progress {
    value: u64,
    pending: Option<Attempt>,
}

impl Progress {
    pub(super) fn run(
        &mut self,
        submit: impl FnOnce(),
        mut confirm: impl FnMut(u64, Attempt) -> Result<()>,
    ) -> Result<()> {
        if let Some(attempt) = self.pending {
            self.check(attempt, &mut confirm)?;
        }
        // D3D12 reserves u64::MAX for device removal.
        self.value = self
            .value
            .checked_add(1)
            .filter(|&value| value != u64::MAX)
            .ok_or_else(|| Error::from_hresult(E_UNEXPECTED))?;
        self.pending = Some(Attempt::Retry);
        submit();
        self.check(Attempt::Initial, &mut confirm)
    }

    fn check(
        &mut self,
        attempt: Attempt,
        confirm: &mut impl FnMut(u64, Attempt) -> Result<()>,
    ) -> Result<()> {
        let result = confirm(self.value, attempt);
        self.pending = match &result {
            Ok(()) => None,
            Err(error) if attempt == Attempt::AfterTimeout || error.code() == MAPPING_TIMEOUT => {
                Some(Attempt::AfterTimeout)
            }
            Err(_) => Some(Attempt::Retry),
        };
        result
    }
}

fn parse_wait_ms(value: Option<&str>) -> u32 {
    value
        .and_then(|s| s.trim().parse().ok())
        .filter(|&ms| ms != 0)
        .map_or(INFINITE, |ms: u32| ms.min(INFINITE - 1))
}

pub(super) fn wait_ms() -> u32 {
    static LIMIT: OnceLock<u32> = OnceLock::new();
    *LIMIT.get_or_init(|| parse_wait_ms(std::env::var("VICTORY_MAPPING_WAIT_MS").ok().as_deref()))
}

fn check_completed(
    completed: u64,
    value: u64,
    device_status: impl FnOnce() -> Result<()>,
) -> Result<bool> {
    if completed == u64::MAX {
        device_status()?;
        return Err(Error::from_hresult(DXGI_ERROR_DEVICE_REMOVED));
    }
    Ok(completed >= value)
}

fn wait_until_complete(
    mut complete: impl FnMut() -> Result<bool>,
    arm: impl FnOnce() -> Result<()>,
    mut wait: impl FnMut() -> Result<()>,
) -> Result<()> {
    if complete()? {
        return Ok(());
    }
    arm()?;
    loop {
        let result = wait();
        // A stale event (or a timeout racing completion) still needs a fence read.
        if complete()? {
            return Ok(());
        }
        result?;
    }
}

fn wait_event(event: HANDLE, millis: u32) -> Result<()> {
    // SAFETY: the caller keeps the event alive throughout the wait.
    match unsafe { WaitForSingleObject(event, millis) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(Error::from_hresult(MAPPING_TIMEOUT)),
        WAIT_FAILED => Err(Error::from_thread()),
        _ => Err(Error::from_hresult(E_UNEXPECTED)),
    }
}

pub(super) fn confirm(
    device: &ID3D12Device,
    queue: &ID3D12CommandQueue,
    fence: &ID3D12Fence,
    event: HANDLE,
    value: u64,
    attempt: Attempt,
    limit: u32,
) -> Result<()> {
    // SAFETY: the mapping queue is serialized, and the caller owns the fence
    // and event for the lifetime of the command list, including during recovery.
    unsafe {
        let complete = || {
            check_completed(fence.GetCompletedValue(), value, || {
                device.GetDeviceRemovedReason()
            })
        };
        if attempt != Attempt::Initial && complete()? {
            return Ok(());
        }
        if attempt == Attempt::AfterTimeout {
            // The marker is already queued and armed. Poll it on this call.
            device.GetDeviceRemovedReason()?;
            return Err(Error::from_hresult(MAPPING_TIMEOUT));
        }
        queue.Signal(fence, value)?;
        let start = (limit != INFINITE).then(Instant::now);
        let result = wait_until_complete(
            complete,
            || fence.SetEventOnCompletion(value, event),
            || {
                let remaining = start.map_or(INFINITE, |start| {
                    limit.saturating_sub(
                        u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX),
                    )
                });
                wait_event(event, remaining)
            },
        );
        if result
            .as_ref()
            .is_err_and(|error| error.code() == MAPPING_TIMEOUT)
        {
            device.GetDeviceRemovedReason()?;
            tracing::warn!(value, limit, "mapping fence wait timed out");
        }
        result
    }
}

#[cfg(test)]
mod tests;
