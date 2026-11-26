//! macOS integration:
//! - `MacInserter`: accessibility-first text insertion with clipboard
//!   (save/restore) + synthesized ⌘V fallback via CGEvent.
//! - `MacContext`: front app identity and Accessibility selection queries.
//! - `MacHotkey`: CGEventTap driven by the shared gesture decoder;
//!   supports bare modifier keys (Right Option default, Fn via vendor HID).
//!
//! Note: this file compiles only on macOS; CI cross-checks it on that host.

use crate::platform::{HotkeyCallback, HotkeyHandle, HotkeyListener};
use ovt_core::hotkey::{Edge, GestureDecoder};
use ovt_core::Result;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use core_foundation::base::{CFRelease, CFStringRef, CFTypeRef, TCFType};
use core_foundation::runloop::CFRunLoopSource;
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop, CFRunLoopSourceRef};
use core_foundation::string::CFString;
use core_graphics::event::{
    CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, EventField,
};

pub use super::{ContextProvider, TargetInfo, TextInserter};

static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn set_session_active(active: bool) {
    SESSION_ACTIVE.store(active, std::sync::atomic::Ordering::SeqCst);
}

pub fn is_own_target(target: &TargetInfo) -> bool {
    target.app_name.as_deref() == Some("oh-my-voice-type")
}

pub fn foreground_work_area() -> Option<(i32, i32, i32, i32)> {
    use objc2_app_kit::NSScreen;
    unsafe {
        let screen = NSScreen::mainScreen()?;
        let frame = screen.visibleFrame();
        Some((
            frame.origin.x as i32,
            frame.origin.y as i32,
            frame.size.width as i32,
            frame.size.height as i32,
        ))
    }
}

// ---------------------------------------------------------------------------
// Permissions
// ---------------------------------------------------------------------------

extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

type AXUIElementRef = *mut std::ffi::c_void;

#[link(name = "ApplicationServices")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> i32;
}

pub fn accessibility_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

pub fn input_monitoring_trusted() -> bool {
    // CGEventTap creation succeeds only when an Input Monitoring-capable
    // process can install a tap; probe with an own-process session tap.
    let probe = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Defaults,
        vec![CGEventType::FlagChanged],
        |_proxy, _ty, _ev| core_foundation::runloop::CFRunLoopStatus::Keep,
    );
    match probe {
        Ok(mut tap) => match tap.mach_port() {
            Ok(_) => true,
            Err(_) => false,
        },
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Insertion
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct MacInserter {
    _private: (),
}

impl TextInserter for MacInserter {
    fn insert(&self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        // Accessibility AX insertion is the most robust macOS path; it needs
        // per-element work that lives in the follow-up context milestone.
        // v1 = clipboard + ⌘V (exactly what Typeless ships today).
        let previous = clipboard_get_text();
        clipboard_set_text(text)?;
        std::thread::sleep(Duration::from_millis(20));
        simulate_cmd_v()?;
        if previous.is_some() {
            let prev = previous;
            let inserted = text.to_string();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(700));
                if clipboard_get_text().as_deref() == Some(inserted.as_str()) {
                    if let Some(prev) = prev {
                        let _ = clipboard_set_text(&prev);
                    }
                }
            });
        }
        Ok(())
    }

    fn copy(&self, text: &str) -> Result<()> {
        clipboard_set_text(text)
    }
}

fn clipboard_set_text(text: &str) -> Result<()> {
    let mut cb = arboard::Clipboard::new()
        .map_err(|e| ovt_core::Error::Insert(format!("clipboard unavailable: {e}")))?;
    cb.set_text(text.to_string())
        .map_err(|e| ovt_core::Error::Insert(format!("clipboard write failed: {e}")))
}

fn clipboard_get_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

/// Synthesized ⌘V.
fn simulate_cmd_v() -> Result<()> {
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| ovt_core::Error::Insert("CGEventSource creation failed".into()))?;
    let cmd_down = CGEventFlags::CGEventFlagCommand;
    for down in [true, false] {
        let ev = core_graphics::event::CGEvent::new_keyboard_event(source.clone(), 9, down)
            .map_err(|_| ovt_core::Error::Insert("CGEvent creation failed".into()))?;
        ev.set_flags(cmd_down);
        ev.post(core_graphics::event::CGEventTapLocation::HID);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Foreground context
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct MacContext;

impl ContextProvider for MacContext {
    fn target(&self) -> TargetInfo {
        use objc2_app_kit::NSWorkspace;

        let workspace = unsafe { NSWorkspace::sharedWorkspace() };
        let mut info = TargetInfo::default();
        unsafe {
            if let Some(app) = workspace.frontmostApplication() {
                info.process_id = Some(app.processIdentifier());
                info.stable_target_id = Some(format!("{}:frontmost", info.process_id.unwrap_or(0)));
                if let Some(name) = app.localizedName() {
                    info.app_name = Some(name.to_string());
                }
            }
        }
        info.accessibility_ok = accessibility_trusted();
        info
    }

    fn capture_selection(&self) -> Result<TargetInfo> {
        if !accessibility_trusted() {
            return Err(ovt_core::Error::Unsupported(
                "grant Accessibility permission to edit a selection".into(),
            ));
        }
        let mut info = self.target();
        let pid = info.process_id.ok_or_else(|| {
            ovt_core::Error::Unsupported("the frontmost application is unavailable".into())
        })?;
        unsafe {
            let application = AXUIElementCreateApplication(pid);
            if application.is_null() {
                return Err(ovt_core::Error::Unsupported(
                    "Accessibility could not create the application element".into(),
                ));
            }
            let focused = focused_element(application)?;
            CFRelease(application as CFTypeRef);
            if focused.is_null() {
                return Err(ovt_core::Error::Unsupported(
                    "Accessibility could not read the focused element".into(),
                ));
            }
            let role = ax_string(focused, "AXRole").unwrap_or_default();
            let subrole = ax_string(focused, "AXSubrole").unwrap_or_default();
            let secure = role.contains("Secure") || subrole.contains("Secure");
            let selection = ax_string(focused, "AXSelectedText").filter(|text| !text.is_empty());
            CFRelease(focused as CFTypeRef);
            if secure {
                return Err(ovt_core::Error::Insert(
                    "the focused field is secure; selection editing is blocked".into(),
                ));
            }
            let selection = selection.ok_or_else(|| {
                ovt_core::Error::Insert("select non-empty text before editing".into())
            })?;
            info.selection_hash = Some(super::revision_hash(
                &info.stable_target_id,
                &Some(selection.clone()),
                &info.app_name,
            ));
            info.selection_revision = info.selection_hash.clone();
            info.selection = Some(selection);
            info.selection_supported = true;
            info.accessibility_ok = true;
        }
        Ok(info)
    }

    fn replace_selection(&self, expected: &TargetInfo, text: &str) -> Result<()> {
        let current = self.capture_selection()?;
        if current.stable_target_id != expected.stable_target_id
            || current.selection_revision != expected.selection_revision
            || current.selection_hash != expected.selection_hash
        {
            return Err(ovt_core::Error::Insert(
                "the target selection changed before replacement".into(),
            ));
        }
        let pid = current.process_id.ok_or_else(|| {
            ovt_core::Error::Unsupported("the frontmost application is unavailable".into())
        })?;
        unsafe {
            let application = AXUIElementCreateApplication(pid);
            if application.is_null() {
                return Err(ovt_core::Error::Unsupported(
                    "Accessibility could not create the application element".into(),
                ));
            }
            let focused = focused_element(application)?;
            CFRelease(application as CFTypeRef);
            if focused.is_null() {
                return Err(ovt_core::Error::Unsupported(
                    "Accessibility could not read the focused element".into(),
                ));
            }
            let attribute = CFString::new("AXSelectedText");
            let value = CFString::new(text);
            let result = AXUIElementSetAttributeValue(
                focused,
                attribute.as_concrete_TypeRef(),
                value.as_concrete_TypeRef() as CFTypeRef,
            );
            CFRelease(focused as CFTypeRef);
            if result != 0 {
                return Err(ovt_core::Error::Insert(format!(
                    "Accessibility selection replacement failed ({result})"
                )));
            }
        }
        Ok(())
    }
}

unsafe fn focused_element(application: AXUIElementRef) -> Result<AXUIElementRef> {
    let attribute = CFString::new("AXFocusedUIElement");
    let mut value: CFTypeRef = std::ptr::null();
    let status =
        AXUIElementCopyAttributeValue(application, attribute.as_concrete_TypeRef(), &mut value);
    if status != 0 || value.is_null() {
        return Err(ovt_core::Error::Unsupported(format!(
            "Accessibility focused-element query failed ({status})"
        )));
    }
    Ok(value as AXUIElementRef)
}

unsafe fn ax_string(element: AXUIElementRef, name: &str) -> Option<String> {
    let attribute = CFString::new(name);
    let mut value: CFTypeRef = std::ptr::null();
    let status =
        AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value);
    if status != 0 || value.is_null() {
        return None;
    }
    let string = CFString::wrap_under_get_rule(value as CFStringRef).to_string();
    CFRelease(value);
    Some(string)
}

// ---------------------------------------------------------------------------
// Global hotkey via CGEventTap
// ---------------------------------------------------------------------------

pub struct MacHookHandle {
    run_loop: CFRunLoop,
    stop: Arc<Mutex<bool>>,
}

impl HotkeyHandle for MacHookHandle {
    fn stop(&mut self) {
        *self.stop.lock().unwrap() = true;
        self.run_loop.stop();
    }
}

impl Drop for MacHookHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone, Default)]
pub struct MacHotkey;

impl HotkeyListener for MacHotkey {
    fn start(
        &self,
        key: &str,
        smart: bool,
        callback: HotkeyCallback,
    ) -> Result<Box<dyn HotkeyHandle + Send + Sync>> {
        let mask = flags_mask_for(key)?;
        let smart_decoder = Arc::new(Mutex::new(GestureDecoder::new(
            Duration::from_millis(350),
            smart,
        )));
        let callback = Arc::new(callback);
        let decoder = smart_decoder.clone();
        let cb = callback.clone();

        let tap = CGEventTap::new(
            CGEventTapLocation::HID,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Defaults,
            vec![
                CGEventType::FlagsChanged,
                CGEventType::KeyDown,
                CGEventType::KeyUp,
            ],
            move |_proxy, kind, event| {
                use core_foundation::runloop::CFRunLoopStatus;
                let flags = event.get_flags();
                let mut d = decoder.lock().unwrap();
                if kind == CGEventType::KeyDown
                    && event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) == 53
                    && SESSION_ACTIVE.load(std::sync::atomic::Ordering::SeqCst)
                {
                    d.reset();
                    cb(ovt_core::hotkey::HotkeyEvent::Cancel);
                    return core_foundation::runloop::CFRunLoopStatus::Keep;
                }
                match kind {
                    CGEventType::FlagsChanged => {
                        let down = flags.contains(mask);
                        // Detect edge from decoder-tracked state (host OS gives
                        // no per-key edge for modifiers).
                        let edge = match &mut *d {
                            dec => dec.edge_from_flag(down),
                        };
                        if let Some(edge) = edge {
                            if let Some(ev) = d.on_edge(edge, std::time::Instant::now()) {
                                cb(ev);
                            }
                        }
                    }
                    _ => {
                        if flags.contains(mask) {
                            // other key pressed while activation modifier held
                            if let Some(ev) = d.note_other_key() {
                                cb(ev);
                            }
                        }
                    }
                }
                // Do not swallow: chords must keep reaching the target app.
                CFRunLoopStatus::Keep
            },
        )
        .map_err(|_| {
            ovt_core::Error::Unsupported(
                "CGEventTap failed: grant Input Monitoring/Accessibility in System Settings".into(),
            )
        })?;

        let source = tap
            .mach_port()
            .map_err(|_| ovt_core::Error::Unsupported("CGEventTap port failed".into()))?;
        let source = CFRunLoopSource::from_binary(source);
        let run_loop = CFRunLoop::get_current();
        run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
        let stop = Arc::new(Mutex::new(false));
        let run_loop_clone = run_loop.clone();
        // The tap must be pumped on a dedicated thread run loop.
        std::thread::spawn(move || {
            let _ = run_loop_clone.run();
        });
        Ok(Box::new(MacHookHandle { run_loop, stop }))
    }
}

fn flags_mask_for(key: &str) -> Result<CGEventFlags> {
    Ok(match key.trim().to_lowercase().as_str() {
        "right-opt" | "right-option" | "ropt" => CGEventFlags::CGEventFlagAlternate,
        "right-cmd" | "right-command" | "rcmd" => CGEventFlags::CGEventFlagCommand,
        "right-ctrl" | "right-control" => CGEventFlags::CGEventFlagControl,
        "right-shift" => CGEventFlags::CGEventFlagShift,
        // Fn/Globe is a vendor HID key on macOS and needs an IOHIDManager tap;
        // for v1 default to Right Option (documented; matches Typeless's
        // external-keyboard guidance).
        "fn" => CGEventFlags::CGEventFlagSecondaryFn,
        other => {
            return Err(ovt_core::Error::Config(format!(
                "unsupported macOS hotkey {other:?} (right-opt, right-cmd, right-ctrl, right-shift)"
            )))
        }
    })
}

// Minimal helper to expose flag-edge state on the decoder for modifier keys.
impl GestureDecoder {
    fn edge_from_flag(&mut self, down: bool) -> Option<Edge> {
        let was_down = !self.is_idle();
        match (down, was_down) {
            (true, false) => Some(Edge::Down),
            (false, true) => Some(Edge::Up),
            _ => None,
        }
    }
}
