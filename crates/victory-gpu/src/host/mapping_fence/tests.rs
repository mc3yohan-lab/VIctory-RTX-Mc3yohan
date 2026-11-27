use std::cell::Cell;

use windows::Win32::Foundation::{CloseHandle, E_FAIL, E_OUTOFMEMORY, ERROR_INVALID_HANDLE};
use windows::Win32::System::Threading::CreateEventW;

use super::*;

#[test]
fn only_a_positive_millisecond_limit_bounds_the_wait() {
    for value in [
        None,
        Some(""),
        Some("0"),
        Some("-1"),
        Some("bad"),
        Some("4294967296"),
    ] {
        assert_eq!(parse_wait_ms(value), INFINITE);
    }
    assert_eq!(parse_wait_ms(Some(" 25 ")), 25);
    assert_eq!(parse_wait_ms(Some("4294967295")), INFINITE - 1);
}

#[test]
fn stale_event_wakes_require_the_requested_marker_to_complete() {
    let waits = Cell::new(0);
    let arms = Cell::new(0);
    wait_until_complete(
        || Ok(waits.get() == 2),
        || {
            arms.set(arms.get() + 1);
            Ok(())
        },
        || {
            waits.set(waits.get() + 1);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!((arms.get(), waits.get()), (1, 2));
}

#[test]
fn marker_completion_racing_a_timeout_is_confirmed() {
    let complete = Cell::new(false);
    wait_until_complete(
        || Ok(complete.get()),
        || Ok(()),
        || {
            complete.set(true);
            Err(Error::from_hresult(MAPPING_TIMEOUT))
        },
    )
    .unwrap();
}

#[test]
fn event_registration_and_native_wait_errors_are_reported() {
    let error = wait_until_complete(
        || Ok(false),
        || Err(Error::from_hresult(E_OUTOFMEMORY)),
        || panic!("the event was not armed"),
    )
    .unwrap_err();
    assert_eq!(error.code(), E_OUTOFMEMORY);
    assert_eq!(
        wait_event(HANDLE::default(), 0).unwrap_err().code(),
        HRESULT::from_win32(ERROR_INVALID_HANDLE.0),
    );
}

#[test]
fn an_unsignalled_native_event_reports_a_timeout() {
    // SAFETY: the test owns the event until the wait finishes.
    let event = unsafe { CreateEventW(None, false, false, None) }.unwrap();
    let result = wait_event(event, 1);
    // SAFETY: the event is owned by this test and the wait is over.
    unsafe { CloseHandle(event) }.unwrap();
    assert_eq!(result.unwrap_err().code(), MAPPING_TIMEOUT);
}

#[test]
fn the_device_removal_sentinel_surfaces_the_device_error() {
    assert!(!check_completed(4, 5, || unreachable!()).unwrap());
    assert!(check_completed(5, 5, || unreachable!()).unwrap());
    let removed = |status| check_completed(u64::MAX, 5, || status).unwrap_err().code();
    assert_eq!(removed(Ok(())), DXGI_ERROR_DEVICE_REMOVED);
    assert_eq!(removed(Err(Error::from_hresult(E_FAIL))), E_FAIL);
}

#[test]
fn a_failed_or_timed_out_marker_is_confirmed_before_resubmission() {
    for (code, retry) in [
        (E_FAIL, Attempt::Retry),
        (MAPPING_TIMEOUT, Attempt::AfterTimeout),
    ] {
        let mut progress = Progress::default();
        let submitted = Cell::new(0);
        let submit = || submitted.set(submitted.get() + 1);
        for attempt in [Attempt::Initial, retry, retry] {
            let failure = progress
                .run(submit, |value, how| {
                    assert_eq!((value, how, submitted.get()), (1, attempt, 1));
                    Err(Error::from_hresult(code))
                })
                .unwrap_err();
            assert_eq!(failure.code(), code);
        }
        let mut checked = Vec::new();
        progress
            .run(submit, |value, attempt| {
                checked.push((value, attempt, submitted.get()));
                Ok(())
            })
            .unwrap();
        assert_eq!(checked, [(1, retry, 1), (2, Attempt::Initial, 2)]);
    }
}

#[test]
fn a_native_mapping_fence_recovers_after_a_timeout() {
    let Some(host) = super::super::host() else {
        eprintln!("no D3D12 host available for the mapping marker test");
        return;
    };
    use windows::Win32::Graphics::Direct3D12::*;

    let device = &host.device;
    // SAFETY: all objects belong to this device and are only used by this test.
    let (queue, fence, gate, event) = unsafe {
        let queue: ID3D12CommandQueue = device
            .CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                Type: D3D12_COMMAND_LIST_TYPE_COPY,
                ..Default::default()
            })
            .unwrap();
        let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE).unwrap();
        let gate: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE).unwrap();
        let event = CreateEventW(None, false, false, None).unwrap();
        (queue, fence, gate, event)
    };
    // SAFETY: the test owns this queue and signals its gate again below.
    unsafe { queue.Wait(&gate, 1) }.unwrap();
    let first = confirm(device, &queue, &fence, event, 1, Attempt::Initial, 5);
    let retry = confirm(device, &queue, &fence, event, 1, Attempt::AfterTimeout, 5);
    // SAFETY: release the queue's wait before checking the timeout results.
    unsafe { gate.Signal(1) }.unwrap();
    confirm(device, &queue, &fence, event, 2, Attempt::Initial, 5000).unwrap();
    confirm(device, &queue, &fence, event, 1, Attempt::AfterTimeout, 5).unwrap();
    // SAFETY: the markers completed, so the test can close its event.
    unsafe { CloseHandle(event) }.unwrap();
    assert_eq!(first.unwrap_err().code(), MAPPING_TIMEOUT);
    assert_eq!(retry.unwrap_err().code(), MAPPING_TIMEOUT);
}
