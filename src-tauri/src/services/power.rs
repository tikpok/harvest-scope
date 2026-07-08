use std::{
    ffi::c_void,
    io,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use tauri::AppHandle;
use tokio::sync::mpsc::{self, UnboundedSender};
use windows_sys::Win32::{
    Foundation::HANDLE,
    System::Power::{
        PowerRegisterSuspendResumeNotification, PowerUnregisterSuspendResumeNotification,
        DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY,
    },
    UI::WindowsAndMessaging::{
        DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMECRITICAL,
        PBT_APMRESUMESTANDBY, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND,
    },
};

use crate::state::AppState;

use super::monitor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PowerTransition {
    Suspend,
    Resume,
}

fn power_transition(event_type: u32) -> Option<PowerTransition> {
    match event_type {
        PBT_APMSUSPEND => Some(PowerTransition::Suspend),
        PBT_APMRESUMEAUTOMATIC
        | PBT_APMRESUMECRITICAL
        | PBT_APMRESUMESTANDBY
        | PBT_APMRESUMESUSPEND => Some(PowerTransition::Resume),
        _ => None,
    }
}

struct CallbackContext {
    state: AppState,
    generation: Arc<AtomicU64>,
    resume_tx: UnboundedSender<(u32, u64)>,
}

unsafe extern "system" fn power_callback(
    context: *const c_void,
    event_type: u32,
    _setting: *const c_void,
) -> u32 {
    if context.is_null() {
        return 0;
    }

    // SAFETY: `context` points to the boxed CallbackContext retained by
    // PowerResumeListener. Drop unregisters the callback before freeing it.
    let context = unsafe { &*(context.cast::<CallbackContext>()) };
    match power_transition(event_type) {
        Some(PowerTransition::Suspend) => {
            context.generation.fetch_add(1, Ordering::AcqRel);
            // This is intentionally synchronous. On Modern Standby the process
            // may keep running after the network is suspended.
            context.state.set_system_suspended(true);
            context.state.set_all_nodes_rechecking(false);
            tracing::info!("Windows suspend notification received; monitoring is paused");
        }
        Some(PowerTransition::Resume) => {
            let generation = context
                .generation
                .fetch_add(1, Ordering::AcqRel)
                .wrapping_add(1);
            // Keep monitoring paused until the async reset has completed.
            context.state.set_system_suspended(true);
            context.state.set_all_nodes_rechecking(true);
            let _ = context.resume_tx.send((event_type, generation));
        }
        None => {}
    }
    0
}

pub struct PowerResumeListener {
    registration: HPOWERNOTIFY,
    _context: Box<CallbackContext>,
}

// The registration handle is only used for unregistration during Drop, and
// CallbackContext consists entirely of Send + Sync values.
unsafe impl Send for PowerResumeListener {}
unsafe impl Sync for PowerResumeListener {}

impl Drop for PowerResumeListener {
    fn drop(&mut self) {
        // SAFETY: registration was returned by the matching registration API.
        let result = unsafe { PowerUnregisterSuspendResumeNotification(self.registration) };
        if result != 0 {
            tracing::warn!(
                error_code = result,
                "failed to unregister Windows power notifications"
            );
        }
    }
}

pub fn register(app: AppHandle, state: AppState) -> io::Result<PowerResumeListener> {
    let (resume_tx, mut resume_rx) = mpsc::unbounded_channel();
    let generation = Arc::new(AtomicU64::new(0));
    let context = Box::new(CallbackContext {
        state: state.clone(),
        generation: generation.clone(),
        resume_tx,
    });
    let parameters = DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
        Callback: Some(power_callback),
        Context: (&*context as *const CallbackContext).cast_mut().cast(),
    };
    let mut registration = std::ptr::null_mut();

    // SAFETY: parameters and the callback context are valid for the call. The
    // boxed context remains at a stable address until after unregistration.
    let result = unsafe {
        PowerRegisterSuspendResumeNotification(
            DEVICE_NOTIFY_CALLBACK,
            (&parameters as *const DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS) as HANDLE,
            &mut registration,
        )
    };
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result as i32));
    }

    tauri::async_runtime::spawn(async move {
        while let Some((event_type, resume_generation)) = resume_rx.recv().await {
            monitor::handle_system_resume(&state, "Windows power resume notification").await;
            // A new suspend can arrive while the async runtime reset is in
            // progress. Never let an older resume event undo that suspension.
            let is_latest_transition = generation.load(Ordering::Acquire) == resume_generation;
            if is_latest_transition {
                state.set_system_suspended(false);
                if let Err(error) = monitor::refresh_all_nodes(app.clone(), state.clone()).await {
                    tracing::warn!(%error, "post-resume node refresh failed");
                }
            }
            tracing::info!(
                event_type,
                is_latest_transition,
                "Windows resume notification handled"
            );
        }
    });

    Ok(PowerResumeListener {
        registration: registration as HPOWERNOTIFY,
        _context: context,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_suspend_and_all_resume_notifications() {
        assert_eq!(
            power_transition(PBT_APMSUSPEND),
            Some(PowerTransition::Suspend)
        );
        for event_type in [
            PBT_APMRESUMEAUTOMATIC,
            PBT_APMRESUMECRITICAL,
            PBT_APMRESUMESTANDBY,
            PBT_APMRESUMESUSPEND,
        ] {
            assert_eq!(power_transition(event_type), Some(PowerTransition::Resume));
        }
        assert_eq!(power_transition(0), None);
    }
}
