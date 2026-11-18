//! Trackpad gesture support for the desktop's swipe gestures (macOS only; no-ops elsewhere).
//!
//! - [`haptic`] plays one of AppKit's three trackpad haptic patterns through
//!   `NSHapticFeedbackManager`. It is only felt on a Force Touch trackpad, and only while a
//!   finger is on it (so not during momentum scrolling).
//! - [`ScrollPhaseMonitor`] installs an `NSEvent` local monitor on scroll-wheel events and reports
//!   their gesture phases. The web's `wheel` event carries no phase, so without it the renderer can
//!   only guess when the fingers lifted (an idle gap, decaying deltas). Only transitions are
//!   reported (not every `changed`), to keep the IPC quiet. The monitor never consumes or alters
//!   an event.
//!
//! - [`actuate`] drives the trackpad's actuator directly through the private MultitouchSupport
//!   framework (loaded at runtime; absent or changed, it reports unavailable and does nothing).
//!   Unlike AppKit's three patterns it takes an actuation ID and two floats, which reach lighter
//!   (and stronger) clicks. Experimental: Apple doesn't document it and may change it.
//!
//! All must be called on the main thread, which is where Electron's main-process JS runs.

use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::JsonCallback;

/// Whether this build can play trackpad haptics (macOS). Says nothing of the hardware: a Mac
/// without a Force Touch trackpad accepts the call and plays nothing.
#[napi(catch_unwind)]
pub fn haptics_available() -> bool {
    cfg!(target_os = "macos")
}

/// Play a trackpad haptic: `"generic"`, `"alignment"` or `"levelChange"`. Anything else, or a
/// platform without haptics, does nothing.
#[napi(catch_unwind)]
pub fn haptic(pattern: String) {
    imp::haptic(&pattern);
}

/// Whether the private trackpad actuator could be opened (macOS, MultitouchSupport present, a
/// Force Touch trackpad attached at the time of the first call).
#[napi(catch_unwind)]
pub fn actuator_available() -> bool {
    imp::actuator_available()
}

/// Fire the trackpad actuator: `MTActuatorActuate(actuator, actuation, 0, arg1, arg2)` on every
/// Force Touch trackpad. Returns whether one accepted it.
#[napi(catch_unwind)]
pub fn actuate(actuation: i32, arg1: f64, arg2: f64) -> bool {
    imp::actuate(actuation, arg1 as f32, arg2 as f32)
}

/// Reports scroll gesture phases as JSON (`ScrollPhase` in desktop/src/shared/bridge-types.ts)
/// until [`ScrollPhaseMonitor::stop`]. Without macOS, never reports anything.
#[napi]
pub struct ScrollPhaseMonitor {
    inner: Option<imp::Monitor>,
}

#[napi]
impl ScrollPhaseMonitor {
    #[napi(constructor, catch_unwind)]
    pub fn new(callback: JsonCallback) -> Result<Self> {
        Ok(Self {
            inner: imp::Monitor::start(callback).map_err(Error::from_reason)?,
        })
    }

    /// Remove the monitor. Idempotent.
    #[napi(catch_unwind)]
    pub fn stop(&mut self) {
        self.inner = None;
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use napi::threadsafe_function::ThreadsafeFunctionCallMode;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSEvent, NSEventMask, NSEventPhase, NSHapticFeedbackManager, NSHapticFeedbackPattern,
        NSHapticFeedbackPerformanceTime, NSHapticFeedbackPerformer,
    };

    use crate::JsonCallback;

    /// The MultitouchSupport entry points, and the opened actuators.
    mod mt {
        use std::ffi::{c_void, CStr};
        use std::sync::OnceLock;

        type CFArrayRef = *const c_void;
        type CFTypeRef = *const c_void;

        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            fn CFArrayGetCount(array: CFArrayRef) -> isize;
            fn CFArrayGetValueAtIndex(array: CFArrayRef, idx: isize) -> *const c_void;
        }

        type CreateList = unsafe extern "C" fn() -> CFArrayRef;
        type GetDeviceId = unsafe extern "C" fn(*const c_void, *mut u64) -> i32;
        type CreateFromId = unsafe extern "C" fn(u64) -> CFTypeRef;
        type Open = unsafe extern "C" fn(CFTypeRef) -> i32;
        type Actuate = unsafe extern "C" fn(CFTypeRef, i32, u32, f32, f32) -> i32;

        pub struct Actuators {
            actuate: Actuate,
            list: Vec<CFTypeRef>,
        }
        // SAFETY: only ever used from the main thread (the callers check MainThreadMarker); the
        // actuators live for the process.
        unsafe impl Send for Actuators {}
        unsafe impl Sync for Actuators {}

        static ACTUATORS: OnceLock<Option<Actuators>> = OnceLock::new();

        unsafe fn sym<T: Copy>(lib: *mut c_void, name: &CStr) -> Option<T> {
            let p = libc::dlsym(lib, name.as_ptr());
            // SAFETY: the caller names the symbol's real signature as T (a fn pointer).
            (!p.is_null()).then(|| std::mem::transmute_copy::<*mut c_void, T>(&p))
        }

        fn open() -> Option<Actuators> {
            // SAFETY: dlopen/dlsym of a system framework; the signatures are the ones the
            // framework has had since Force Touch shipped (as used by HapticKey and others).
            unsafe {
                let lib = libc::dlopen(c"/System/Library/PrivateFrameworks/MultitouchSupport.framework/MultitouchSupport".as_ptr(), libc::RTLD_NOW);
                if lib.is_null() {
                    return None;
                }
                let create_list: CreateList = sym(lib, c"MTDeviceCreateList")?;
                let get_id: GetDeviceId = sym(lib, c"MTDeviceGetDeviceID")?;
                let from_id: CreateFromId = sym(lib, c"MTActuatorCreateFromDeviceID")?;
                let open: Open = sym(lib, c"MTActuatorOpen")?;
                let actuate: Actuate = sym(lib, c"MTActuatorActuate")?;
                let devices = create_list();
                if devices.is_null() {
                    return None;
                }
                let mut list = Vec::new();
                for i in 0..CFArrayGetCount(devices) {
                    let device = CFArrayGetValueAtIndex(devices, i);
                    let mut id = 0u64;
                    if device.is_null() || get_id(device, &mut id) != 0 {
                        continue;
                    }
                    let actuator = from_id(id);
                    if !actuator.is_null() && open(actuator) == 0 {
                        list.push(actuator);
                    }
                }
                (!list.is_empty()).then_some(Actuators { actuate, list })
            }
        }

        pub fn get() -> Option<&'static Actuators> {
            ACTUATORS.get_or_init(open).as_ref()
        }

        impl Actuators {
            pub fn fire(&self, actuation: i32, a: f32, b: f32) -> bool {
                let mut ok = false;
                for &act in &self.list {
                    // SAFETY: an actuator opened in open(), never closed.
                    ok |= unsafe { (self.actuate)(act, actuation, 0, a, b) } == 0;
                }
                ok
            }
        }
    }

    pub fn actuator_available() -> bool {
        MainThreadMarker::new().is_some() && mt::get().is_some()
    }

    pub fn actuate(actuation: i32, a: f32, b: f32) -> bool {
        if MainThreadMarker::new().is_none() {
            return false;
        }
        mt::get().is_some_and(|m| m.fire(actuation, a, b))
    }

    pub fn haptic(pattern: &str) {
        let pattern = match pattern {
            "generic" => NSHapticFeedbackPattern::Generic,
            "alignment" => NSHapticFeedbackPattern::Alignment,
            "levelChange" => NSHapticFeedbackPattern::LevelChange,
            _ => return,
        };
        if MainThreadMarker::new().is_none() {
            return;
        }
        NSHapticFeedbackManager::defaultPerformer()
            .performFeedbackPattern_performanceTime(pattern, NSHapticFeedbackPerformanceTime::Now);
    }

    fn phase_name(phase: NSEventPhase) -> &'static str {
        if phase.contains(NSEventPhase::Began) {
            "began"
        } else if phase.contains(NSEventPhase::Ended) {
            "ended"
        } else if phase.contains(NSEventPhase::Cancelled) {
            "cancelled"
        } else if phase.contains(NSEventPhase::MayBegin) {
            "mayBegin"
        } else if phase.contains(NSEventPhase::Changed) {
            "changed"
        } else if phase.contains(NSEventPhase::Stationary) {
            "stationary"
        } else {
            "none"
        }
    }

    pub struct Monitor {
        token: Retained<AnyObject>,
    }

    impl Monitor {
        pub fn start(callback: JsonCallback) -> Result<Option<Self>, String> {
            if MainThreadMarker::new().is_none() {
                return Err("ScrollPhaseMonitor must be created on the main thread".into());
            }
            let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
                // SAFETY: AppKit hands the monitor a valid event for the duration of the call.
                let ev = unsafe { event.as_ref() };
                let phase = ev.phase();
                let momentum = ev.momentumPhase();
                // Every transition, but not the stream of `changed` events in between.
                let transition = !(phase.is_empty() || phase == NSEventPhase::Changed)
                    || !(momentum.is_empty() || momentum == NSEventPhase::Changed);
                if transition {
                    let json = format!(
                        r#"{{"phase":"{}","momentum":"{}","precise":{},"inverted":{},"t":{}}}"#,
                        phase_name(phase),
                        phase_name(momentum),
                        ev.hasPreciseScrollingDeltas(),
                        ev.isDirectionInvertedFromDevice(),
                        ev.timestamp() * 1000.0,
                    );
                    callback.call(json, ThreadsafeFunctionCallMode::NonBlocking);
                }
                event.as_ptr()
            });
            // SAFETY: the block returns the event it was given, a valid pointer.
            let token = unsafe {
                NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                    NSEventMask::ScrollWheel,
                    &block,
                )
            };
            Ok(token.map(|token| Self { token }))
        }
    }

    impl Drop for Monitor {
        fn drop(&mut self) {
            // SAFETY: the token is the one addLocalMonitor returned.
            unsafe { NSEvent::removeMonitor(&self.token) };
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use crate::JsonCallback;

    pub fn haptic(_pattern: &str) {}

    pub fn actuator_available() -> bool {
        false
    }

    pub fn actuate(_actuation: i32, _a: f32, _b: f32) -> bool {
        false
    }

    pub struct Monitor;

    impl Monitor {
        pub fn start(_callback: JsonCallback) -> Result<Option<Self>, String> {
            Ok(None)
        }
    }
}
