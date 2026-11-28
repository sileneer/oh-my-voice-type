//! macOS integration:
//! - `MacInserter`: AX direct set at the caret (strategy ladder level 1),
//!   with clipboard (save/restore) + synthesized ⌘V fallback via CGEvent.
//! - `MacContext`: front app identity and Accessibility selection queries.
//! - `MacHotkey`: CGEventTap driven by the shared gesture decoder for bare
//!   modifier keys (Right Option default); Fn/Globe goes through an
//!   IOHIDManager tap (vendor HID usage, needs Input Monitoring).
//!
//! Note: this file compiles only on macOS; CI cross-checks it on that host.

use crate::platform::{HotkeyCallback, HotkeyHandle, HotkeyListener};
use ovt_core::hotkey::{Edge, GestureDecoder};
use ovt_core::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::Mutex;
use std::time::Duration;

use core_foundation::base::{CFAllocatorRef, CFIndex, CFRange, CFRelease, CFTypeRef, TCFType};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::{CFNumber, CFNumberRef};
use core_foundation::runloop::{
    kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopRef,
};
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::event::{
    CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, EventField,
};

pub use super::{ContextProvider, TargetInfo, TextInserter};

static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn set_session_active(active: bool) {
    SESSION_ACTIVE.store(active, Ordering::SeqCst);
}

pub fn is_own_target(target: &TargetInfo) -> bool {
    target.app_name.as_deref() == Some("oh-my-voice-type")
}

pub fn foreground_work_area() -> Option<(i32, i32, i32, i32)> {
    use objc2_app_kit::NSScreen;
    use objc2_foundation::MainThreadMarker;

    // Main-thread only: callers run this inside `run_on_main_thread`.
    let mtm = MainThreadMarker::new()?;
    let screen = NSScreen::mainScreen(mtm)?;
    let full = screen.frame();
    let visible = screen.visibleFrame();
    // AppKit reports points with the origin at the bottom-left; Tauri's
    // `set_position` takes physical pixels from the top-left. Without this the
    // pill landed over the Dock (and at half the offset on Retina screens).
    let scale = screen.backingScaleFactor();
    let top = full.size.height - (visible.origin.y + visible.size.height);
    Some((
        (visible.origin.x * scale) as i32,
        (top * scale) as i32,
        (visible.size.width * scale) as i32,
        (visible.size.height * scale) as i32,
    ))
}

// ---------------------------------------------------------------------------
// Permissions
// ---------------------------------------------------------------------------

type AXUIElementRef = *mut std::ffi::c_void;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
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
    fn AXUIElementCopyParameterizedAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        parameter: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXValueGetValue(value: CFTypeRef, value_type: u32, out: *mut std::ffi::c_void) -> bool;
    fn AXValueCreate(value_type: u32, value: *const std::ffi::c_void) -> CFTypeRef;
}

/// `kAXValueCFRangeType`.
const AX_VALUE_CF_RANGE: u32 = 4;

pub fn accessibility_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

pub fn input_monitoring_trusted() -> bool {
    // CGEventTap creation succeeds only when an Input Monitoring-capable
    // process can install a tap; probe with a passive tap.
    CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![CGEventType::FlagsChanged],
        |_proxy, _ty, _ev| None,
    )
    .is_ok()
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
        // Strategy ladder level 1: set AXSelectedText on the focused element —
        // an empty selection inserts at the caret, a selection is replaced.
        // Apps without AX text support (and every failure mode) fall through
        // to the clipboard + ⌘V ladder below, which was the v1 path.
        if accessibility_trusted() && unsafe { try_ax_insert(text) } {
            return Ok(());
        }
        let previous = clipboard_get_text();
        clipboard_set_text(text)?;
        std::thread::sleep(Duration::from_millis(20));
        simulate_cmd_v()?;
        if let Some(previous) = previous {
            let inserted = text.to_string();
            std::thread::spawn(move || {
                // 700 ms let slow targets paste the restored old content.
                std::thread::sleep(Duration::from_millis(1500));
                if clipboard_get_text().as_deref() == Some(inserted.as_str()) {
                    let _ = clipboard_set_text(&previous);
                }
            });
        }
        Ok(())
    }

    fn copy(&self, text: &str) -> Result<()> {
        clipboard_set_text(text)
    }
}

/// Strategy ladder level 1: set `AXSelectedText` on the focused element.
/// `false` = the element does not accept AX text edits (or the AX query
/// failed); the caller falls back to the clipboard + ⌘V ladder.
unsafe fn try_ax_insert(text: &str) -> bool {
    let systemwide = AXUIElementCreateSystemWide();
    if systemwide.is_null() {
        return false;
    }
    let focused = match focused_element(systemwide) {
        Ok(focused) => focused,
        Err(_) => {
            CFRelease(systemwide as CFTypeRef);
            return false;
        }
    };
    CFRelease(systemwide as CFTypeRef);
    if focused.is_null() {
        return false;
    }
    let role = ax_string(focused, "AXRole").unwrap_or_default();
    let subrole = ax_string(focused, "AXSubrole").unwrap_or_default();
    if role.contains("Secure") || subrole.contains("Secure") {
        CFRelease(focused as CFTypeRef);
        return false;
    }
    // Several hosts (Chromium/Electron web content, some Qt and Java apps)
    // report success for an AXSelectedText write they ignore, so success is
    // only trusted once the element visibly changed. Without a readable
    // baseline there is nothing to verify against: use the clipboard ladder.
    let (Some(count_before), Some(range_before)) =
        (ax_character_count(focused), ax_selected_range(focused))
    else {
        CFRelease(focused as CFTypeRef);
        return false;
    };
    let attribute = CFString::new("AXSelectedText");
    let value = CFString::new(text);
    let result = AXUIElementSetAttributeValue(
        focused,
        attribute.as_concrete_TypeRef(),
        value.as_concrete_TypeRef() as CFTypeRef,
    );
    let inserted = result == 0 && ax_edit_observed(focused, count_before, range_before);
    CFRelease(focused as CFTypeRef);
    inserted
}

/// Wait briefly for the AX write to show up: Chromium applies it
/// asynchronously in the renderer. Any change to the character count or the
/// selection counts as applied, so a slow but successful write is never
/// pasted a second time; no change at all means the write was ignored.
unsafe fn ax_edit_observed(
    element: AXUIElementRef,
    count_before: CFIndex,
    range_before: CFRange,
) -> bool {
    for attempt in 0..12 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(25));
        }
        let count = ax_character_count(element);
        let range = ax_selected_range(element);
        let count_changed = count.is_some_and(|count| count != count_before);
        let range_changed = range.is_some_and(|range| {
            range.location != range_before.location || range.length != range_before.length
        });
        if count_changed || range_changed {
            return true;
        }
    }
    tracing::info!("AXSelectedText write had no visible effect; falling back to paste");
    false
}

unsafe fn ax_character_count(element: AXUIElementRef) -> Option<CFIndex> {
    let attribute = CFString::new("AXNumberOfCharacters");
    let mut value: CFTypeRef = std::ptr::null();
    if AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value) != 0
        || value.is_null()
    {
        return None;
    }
    let count = CFNumber::wrap_under_create_rule(value as CFNumberRef).to_i64();
    count.map(|count| count as CFIndex)
}

unsafe fn ax_selected_range(element: AXUIElementRef) -> Option<CFRange> {
    let attribute = CFString::new("AXSelectedTextRange");
    let mut value: CFTypeRef = std::ptr::null();
    if AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value) != 0
        || value.is_null()
    {
        return None;
    }
    let mut range = CFRange {
        location: 0,
        length: 0,
    };
    let read = AXValueGetValue(
        value,
        AX_VALUE_CF_RANGE,
        &mut range as *mut CFRange as *mut std::ffi::c_void,
    );
    CFRelease(value);
    read.then_some(range)
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

        let workspace = NSWorkspace::sharedWorkspace();
        let mut info = TargetInfo::default();
        if let Some(app) = workspace.frontmostApplication() {
            if let Ok(pid) = u32::try_from(app.processIdentifier()) {
                info.process_id = Some(pid);
                info.stable_target_id = Some(format!("{pid}:frontmost"));
            }
            if let Some(name) = app.localizedName() {
                info.app_name = Some(name.to_string());
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
        let pid = info
            .process_id
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| {
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

    fn text_before_caret(&self, max_chars: usize) -> Option<String> {
        if max_chars == 0 || !accessibility_trusted() {
            return None;
        }
        let pid = i32::try_from(self.target().process_id?).ok()?;
        unsafe {
            let application = AXUIElementCreateApplication(pid);
            if application.is_null() {
                return None;
            }
            let focused = focused_element(application);
            CFRelease(application as CFTypeRef);
            let focused = focused.ok()?;
            if focused.is_null() {
                return None;
            }
            let text = caret_prefix(focused, max_chars);
            CFRelease(focused as CFTypeRef);
            text
        }
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
        let pid = current
            .process_id
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| {
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

/// Up to `max_chars` characters before the caret of `element`, via
/// `AXSelectedTextRange` + `AXStringForRange`; `None` for secure fields.
unsafe fn caret_prefix(element: AXUIElementRef, max_chars: usize) -> Option<String> {
    let role = ax_string(element, "AXRole").unwrap_or_default();
    let subrole = ax_string(element, "AXSubrole").unwrap_or_default();
    if role.contains("Secure") || subrole.contains("Secure") {
        return None;
    }
    let attribute = CFString::new("AXSelectedTextRange");
    let mut value: CFTypeRef = std::ptr::null();
    if AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value) != 0
        || value.is_null()
    {
        return None;
    }
    let mut selected = CFRange {
        location: 0,
        length: 0,
    };
    let read = AXValueGetValue(
        value,
        AX_VALUE_CF_RANGE,
        &mut selected as *mut CFRange as *mut std::ffi::c_void,
    );
    CFRelease(value);
    if !read || selected.location <= 0 {
        return None;
    }
    let count = (max_chars as CFIndex).min(selected.location);
    let wanted = CFRange {
        location: selected.location - count,
        length: count,
    };
    let parameter = AXValueCreate(
        AX_VALUE_CF_RANGE,
        &wanted as *const CFRange as *const std::ffi::c_void,
    );
    if parameter.is_null() {
        return None;
    }
    let name = CFString::new("AXStringForRange");
    let mut text: CFTypeRef = std::ptr::null();
    let status = AXUIElementCopyParameterizedAttributeValue(
        element,
        name.as_concrete_TypeRef(),
        parameter,
        &mut text,
    );
    CFRelease(parameter);
    if status != 0 || text.is_null() {
        return None;
    }
    let string = CFString::wrap_under_get_rule(text as CFStringRef).to_string();
    CFRelease(text);
    Some(string)
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
}

impl HotkeyHandle for MacHookHandle {
    fn stop(&mut self) {
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
        if key.trim().eq_ignore_ascii_case("fn") {
            // The Fn/Globe key is a vendor HID usage, not a keycode: CGEventTap
            // only reports a bare SecondaryFn flag flip without key codes, so
            // its tap lives one layer lower, at IOHIDManager.
            return self.start_fn_tap(smart, callback);
        }
        let mask = flags_mask_for(key)?;
        let (tx, rx) = sync_channel::<Result<CFRunLoop>>(1);

        // The tap and its CFRunLoop must live on the same dedicated thread so
        // the tap stays alive for the duration of the run loop.
        std::thread::spawn(move || {
            let decoder = Mutex::new((
                GestureDecoder::new(Duration::from_millis(350), smart),
                false,
            ));

            let tap = match CGEventTap::new(
                CGEventTapLocation::HID,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                vec![
                    CGEventType::FlagsChanged,
                    CGEventType::KeyDown,
                    CGEventType::KeyUp,
                ],
                move |_proxy, kind, event| {
                    let flags = event.get_flags();
                    let mut state = decoder.lock().unwrap();
                    let (ref mut d, ref mut was_down) = *state;
                    if matches!(kind, CGEventType::KeyDown)
                        && event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) == 53
                        && SESSION_ACTIVE.load(Ordering::SeqCst)
                    {
                        d.reset();
                        *was_down = false;
                        callback(ovt_core::hotkey::HotkeyEvent::Cancel);
                        return None;
                    }
                    match kind {
                        CGEventType::FlagsChanged => {
                            let down = flags.contains(mask);
                            let edge = match (down, *was_down) {
                                (true, false) => Some(Edge::Down),
                                (false, true) => Some(Edge::Up),
                                _ => None,
                            };
                            *was_down = down;
                            if edge == Some(Edge::Down) {
                                d.sync_session(SESSION_ACTIVE.load(Ordering::SeqCst));
                            }
                            if let Some(edge) = edge {
                                if let Some(ev) = d.on_edge(edge, std::time::Instant::now()) {
                                    callback(ev);
                                }
                            }
                        }
                        _ => {
                            if flags.contains(mask) {
                                // other key pressed while activation modifier held
                                if let Some(ev) = d.note_other_key() {
                                    callback(ev);
                                }
                            }
                        }
                    }
                    // Do not swallow: chords must keep reaching the target app.
                    None
                },
            ) {
                Ok(tap) => tap,
                Err(()) => {
                    let _ = tx.send(Err(ovt_core::Error::Unsupported(
                        "CGEventTap failed: grant Input Monitoring/Accessibility in System Settings"
                            .into(),
                    )));
                    return;
                }
            };

            let source = match tap.mach_port.create_runloop_source(0) {
                Ok(source) => source,
                Err(()) => {
                    let _ = tx.send(Err(ovt_core::Error::Unsupported(
                        "CGEventTap run loop source failed".into(),
                    )));
                    return;
                }
            };
            let run_loop = CFRunLoop::get_current();
            run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
            tap.enable();
            let _ = tx.send(Ok(run_loop));
            CFRunLoop::run_current();
            // Keep `tap` and `source` alive until the run loop stops.
            drop(source);
            drop(tap);
        });

        let run_loop = rx
            .recv()
            .map_err(|_| ovt_core::Error::Unsupported("CGEventTap thread exited early".into()))??;
        Ok(Box::new(MacHookHandle { run_loop }))
    }
}

impl MacHotkey {
    /// IOHIDManager tap for the Fn/Globe key; see the FFI block below for why
    /// CGEventTap cannot serve it. Needs Input Monitoring.
    fn start_fn_tap(
        &self,
        smart: bool,
        callback: HotkeyCallback,
    ) -> Result<Box<dyn HotkeyHandle + Send + Sync>> {
        let (tx, rx) = sync_channel::<Result<CFRunLoop>>(1);
        std::thread::spawn(move || {
            // Owned by this thread; the HID callbacks only fire while the run
            // loop below is running, so the raw pointer stays valid.
            let context = Box::new(FnTapContext {
                decoder: Mutex::new((
                    GestureDecoder::new(Duration::from_millis(350), smart),
                    false,
                )),
                callback,
            });
            let manager = unsafe { IOHIDManagerCreate(std::ptr::null(), 0) };
            if manager.is_null() {
                let _ = tx.send(Err(ovt_core::Error::Unsupported(
                    "IOHIDManagerCreate failed".into(),
                )));
                return;
            }
            let matching = CFDictionary::from_CFType_pairs(&[
                (CFString::new("DeviceUsagePage"), CFNumber::from(1)), // GenericDesktop
                (CFString::new("DeviceUsage"), CFNumber::from(6)),     // Keyboard
            ]);
            unsafe {
                IOHIDManagerSetDeviceMatching(manager, matching.as_concrete_TypeRef());
                IOHIDManagerScheduleWithRunLoop(
                    manager,
                    CFRunLoop::get_current().as_concrete_TypeRef(),
                    kCFRunLoopDefaultMode,
                );
                IOHIDManagerRegisterInputValueCallback(
                    manager,
                    fn_tap_value_callback,
                    &*context as *const FnTapContext as *mut std::ffi::c_void,
                );
            }
            let opened = unsafe { IOHIDManagerOpen(manager, 0) };
            if opened != 0 {
                unsafe { CFRelease(manager as CFTypeRef) };
                let _ = tx.send(Err(ovt_core::Error::Unsupported(
                    "IOHIDManagerOpen failed: grant Input Monitoring in System Settings".into(),
                )));
                return;
            }
            let run_loop = CFRunLoop::get_current();
            let _ = tx.send(Ok(run_loop.clone()));
            CFRunLoop::run_current();
            unsafe {
                IOHIDManagerClose(manager, 0);
                CFRelease(manager as CFTypeRef);
            }
        });
        let run_loop = rx.recv().map_err(|_| {
            ovt_core::Error::Unsupported("IOHIDManager thread exited early".into())
        })??;
        Ok(Box::new(MacHookHandle { run_loop }))
    }
}

// ---------------------------------------------------------------------------
// Fn/Globe hotkey via IOHIDManager
// ---------------------------------------------------------------------------

type IOHIDManagerRef = *mut std::ffi::c_void;
type IOHIDValueRef = *mut std::ffi::c_void;
type IOHIDElementRef = *mut std::ffi::c_void;

/// `kHIDPage_KeyboardOrKeypad`.
const HID_PAGE_KEYBOARD: u32 = 0x07;
/// `kHIDPage_AppleVendorTopCase`: Fn on built-in MacBook keyboards.
const HID_PAGE_APPLE_VENDOR_TOP_CASE: u32 = 0xFF;
/// `kHIDPage_AppleVendorKeyboard`: Fn/Globe on Magic Keyboards.
const HID_PAGE_APPLE_VENDOR_KEYBOARD: u32 = 0xFF01;
/// `kHIDUsage_AV_TopCase_KeyboardFn` and `kHIDUsage_AppleVendorKeyboard_Function`
/// (both 0x03 on their pages). Not 0x3F: that is the virtual keycode
/// `kVK_Function`, and on the keyboard page 0x3F is F6.
const HID_USAGE_APPLE_FN: u32 = 0x03;
/// `kHIDUsage_KeyboardEscape`.
const HID_USAGE_KEYBOARD_ESCAPE: u32 = 0x29;
/// Real key usages on the keyboard page (`a` through right GUI); below are the
/// error/rollover codes, above are array sentinels.
const HID_KEYBOARD_KEYS: std::ops::RangeInclusive<u32> = 0x04..=0xE7;

fn is_apple_fn(page: u32, usage: u32) -> bool {
    matches!(
        page,
        HID_PAGE_APPLE_VENDOR_TOP_CASE | HID_PAGE_APPLE_VENDOR_KEYBOARD
    ) && usage == HID_USAGE_APPLE_FN
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDManagerCreate(allocator: CFAllocatorRef, options: u32) -> IOHIDManagerRef;
    fn IOHIDManagerSetDeviceMatching(manager: IOHIDManagerRef, matching: CFDictionaryRef);
    fn IOHIDManagerScheduleWithRunLoop(
        manager: IOHIDManagerRef,
        run_loop: CFRunLoopRef,
        mode: CFStringRef,
    );
    fn IOHIDManagerOpen(manager: IOHIDManagerRef, options: u32) -> i32;
    fn IOHIDManagerClose(manager: IOHIDManagerRef, options: u32) -> i32;
    fn IOHIDManagerRegisterInputValueCallback(
        manager: IOHIDManagerRef,
        callback: IOHIDValueCallback,
        context: *mut std::ffi::c_void,
    );
    fn IOHIDValueGetElement(value: IOHIDValueRef) -> IOHIDElementRef;
    fn IOHIDValueGetIntegerValue(value: IOHIDValueRef) -> CFIndex;
    fn IOHIDElementGetUsagePage(element: IOHIDElementRef) -> u32;
    fn IOHIDElementGetUsage(element: IOHIDElementRef) -> u32;
}

type IOHIDValueCallback = unsafe extern "C" fn(
    context: *mut std::ffi::c_void,
    result: i32,
    sender: *mut std::ffi::c_void,
    value: IOHIDValueRef,
);

struct FnTapContext {
    decoder: Mutex<(GestureDecoder, bool)>,
    callback: HotkeyCallback,
}

unsafe fn handle_fn_tap_value(context: *mut std::ffi::c_void, value: IOHIDValueRef) {
    let Some(context) = (context as *const FnTapContext).as_ref() else {
        return;
    };
    if value.is_null() {
        return;
    }
    let element = IOHIDValueGetElement(value);
    if element.is_null() {
        return;
    }
    let page = IOHIDElementGetUsagePage(element);
    let usage = IOHIDElementGetUsage(element);
    let fn_key = is_apple_fn(page, usage);
    if !fn_key && (page != HID_PAGE_KEYBOARD || !HID_KEYBOARD_KEYS.contains(&usage)) {
        return;
    }
    let down = IOHIDValueGetIntegerValue(value) != 0;
    let mut state = context
        .decoder
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (ref mut decoder, ref mut was_down) = *state;
    match usage {
        _ if fn_key => {
            let edge = match (down, *was_down) {
                (true, false) => Some(Edge::Down),
                (false, true) => Some(Edge::Up),
                _ => None,
            };
            *was_down = down;
            if edge == Some(Edge::Down) {
                decoder.sync_session(SESSION_ACTIVE.load(Ordering::SeqCst));
            }
            if let Some(edge) = edge {
                if let Some(event) = decoder.on_edge(edge, std::time::Instant::now()) {
                    (context.callback)(event);
                }
            }
        }
        HID_USAGE_KEYBOARD_ESCAPE if down && SESSION_ACTIVE.load(Ordering::SeqCst) => {
            decoder.reset();
            *was_down = false;
            (context.callback)(ovt_core::hotkey::HotkeyEvent::Cancel);
        }
        _ if *was_down => {
            // Another key arrives while Fn is held: a chord, not a tap.
            if let Some(event) = decoder.note_other_key() {
                (context.callback)(event);
            }
        }
        _ => {}
    }
}

unsafe extern "C" fn fn_tap_value_callback(
    context: *mut std::ffi::c_void,
    _result: i32,
    _sender: *mut std::ffi::c_void,
    value: IOHIDValueRef,
) {
    handle_fn_tap_value(context, value);
}

fn flags_mask_for(key: &str) -> Result<CGEventFlags> {
    Ok(match key.trim().to_lowercase().as_str() {
        "right-opt" | "right-option" | "ropt" => CGEventFlags::CGEventFlagAlternate,
        "right-cmd" | "right-command" | "rcmd" => CGEventFlags::CGEventFlagCommand,
        "right-ctrl" | "right-control" => CGEventFlags::CGEventFlagControl,
        "right-shift" => CGEventFlags::CGEventFlagShift,
        // "fn" never reaches here: start() routes it to the IOHIDManager tap.
        other => {
            return Err(ovt_core::Error::Config(format!(
                "unsupported macOS hotkey {other:?} (right-opt, right-cmd, right-ctrl, right-shift, fn)"
            )))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fn_key_matches_apple_vendor_usages_only() {
        assert!(is_apple_fn(HID_PAGE_APPLE_VENDOR_TOP_CASE, 0x03));
        assert!(is_apple_fn(HID_PAGE_APPLE_VENDOR_KEYBOARD, 0x03));
        // 0x3F on the keyboard page is F6, not Fn.
        assert!(!is_apple_fn(HID_PAGE_KEYBOARD, 0x3F));
        assert!(!is_apple_fn(HID_PAGE_KEYBOARD, 0x03));
    }
}
