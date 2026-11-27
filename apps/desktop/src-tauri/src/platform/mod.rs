//! Platform integrations selected at compile time. Unsupported accessibility
//! operations return explicit errors rather than degrading edit mode to normal
//! dictation.

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

use ovt_core::Result;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Default)]
pub struct TargetInfo {
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub caret_text: Option<String>,
    pub selection: Option<String>,
    pub accessibility_ok: bool,
    pub stable_target_id: Option<String>,
    pub process_id: Option<u32>,
    pub selection_revision: Option<String>,
    pub selection_hash: Option<String>,
    pub secure_field: bool,
    pub selection_supported: bool,
}

pub fn revision_hash(
    target_id: &Option<String>,
    selection: &Option<String>,
    app_name: &Option<String>,
) -> String {
    let mut hasher = DefaultHasher::new();
    target_id.hash(&mut hasher);
    selection.hash(&mut hasher);
    app_name.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub trait TextInserter: Send + Sync + 'static {
    fn insert(&self, text: &str) -> Result<()>;
    fn copy(&self, text: &str) -> Result<()>;
}

pub trait ContextProvider: Send + Sync + 'static {
    fn target(&self) -> TargetInfo;
    fn capture(&self) -> Result<TargetInfo> {
        Ok(self.target())
    }
    fn capture_selection(&self) -> Result<TargetInfo> {
        Err(ovt_core::Error::Unsupported(
            "selection reading is not implemented on this platform".into(),
        ))
    }
    fn verify_target(&self, expected: &TargetInfo) -> Result<TargetInfo> {
        let current = self.capture()?;
        if expected.stable_target_id.is_some()
            && current.stable_target_id != expected.stable_target_id
        {
            return Err(ovt_core::Error::Insert(
                "the focused target changed before delivery".into(),
            ));
        }
        Ok(current)
    }
    fn replace_selection(&self, _expected: &TargetInfo, _text: &str) -> Result<()> {
        Err(ovt_core::Error::Unsupported(
            "selection replacement is not implemented on this platform".into(),
        ))
    }
}

pub trait OutputMuteGuard: Send {
    fn restore(&mut self) -> Result<()>;
}

pub struct OutputMuteGuardHandle(Box<dyn OutputMuteGuard>);

impl OutputMuteGuardHandle {
    pub fn restore(&mut self) -> Result<()> {
        self.0.restore()
    }
}

impl Drop for OutputMuteGuardHandle {
    fn drop(&mut self) {
        let _ = self.0.restore();
    }
}

pub type HotkeyCallback = Box<dyn Fn(ovt_core::hotkey::HotkeyEvent) + Send + 'static>;

pub trait HotkeyListener: Send + Sync + 'static {
    fn start(
        &self,
        key: &str,
        smart: bool,
        callback: HotkeyCallback,
    ) -> Result<Box<dyn HotkeyHandle + Send + Sync>>;
}

pub trait HotkeyHandle: Send + Sync {
    fn stop(&mut self);
}

pub fn inserter() -> Box<dyn TextInserter> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsInserter)
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacInserter::default())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        struct Null;
        impl TextInserter for Null {
            fn insert(&self, _text: &str) -> Result<()> {
                Err(ovt_core::Error::Unsupported("text insertion".into()))
            }
            fn copy(&self, _text: &str) -> Result<()> {
                Err(ovt_core::Error::Unsupported("clipboard".into()))
            }
        }
        Box::new(Null)
    }
}

pub fn context_provider() -> Box<dyn ContextProvider> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsContext)
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacContext)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        struct Null;
        impl ContextProvider for Null {
            fn target(&self) -> TargetInfo {
                TargetInfo::default()
            }
        }
        Box::new(Null)
    }
}

pub fn hotkey_listener() -> Box<dyn HotkeyListener> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsHotkey)
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacHotkey)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        struct Null;
        impl HotkeyListener for Null {
            fn start(
                &self,
                _key: &str,
                _smart: bool,
                _callback: HotkeyCallback,
            ) -> Result<Box<dyn HotkeyHandle + Send + Sync>> {
                Err(ovt_core::Error::Unsupported("hotkeys".into()))
            }
        }
        Box::new(Null)
    }
}

pub fn mute_output() -> Result<OutputMuteGuardHandle> {
    #[cfg(target_os = "windows")]
    {
        Ok(OutputMuteGuardHandle(windows::mute_output()?))
    }
    #[cfg(target_os = "macos")]
    {
        Err(ovt_core::Error::Unsupported(
            "system output muting is not available through the current macOS integration".into(),
        ))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err(ovt_core::Error::Unsupported("system output muting".into()))
    }
}

pub fn set_session_active(active: bool) {
    #[cfg(target_os = "windows")]
    {
        windows::set_session_active(active);
    }
    #[cfg(target_os = "macos")]
    {
        macos::set_session_active(active);
    }
}

pub fn is_own_target(target: &TargetInfo) -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::is_own_target(target)
    }
    #[cfg(target_os = "macos")]
    {
        macos::is_own_target(target)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = target;
        false
    }
}

/// Physical work area `(x, y, width, height)` of the target's display.
pub fn target_work_area() -> Option<(i32, i32, i32, i32)> {
    #[cfg(target_os = "windows")]
    {
        windows::foreground_work_area()
    }
    #[cfg(target_os = "macos")]
    {
        macos::foreground_work_area()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
use ::windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use ::windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE};

/// Hide a window shown with [`show_inactive`]; must run on the main thread.
///
/// On Windows `show_inactive` bypasses tao, whose cached visibility flag stays
/// "hidden"; tao's `hide()` then sees no change and does nothing, which left
/// the pill on screen forever. Hide it with the same Win32 call instead.
pub fn hide_window(window: &tauri::WebviewWindow) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        let tauri_hwnd = window.hwnd().map_err(|error| {
            ovt_core::Error::Unsupported(format!("window handle unavailable: {error}"))
        })?;
        unsafe {
            let _ = ShowWindow(HWND(tauri_hwnd.0), SW_HIDE);
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        window
            .hide()
            .map_err(|error| ovt_core::Error::Unsupported(format!("hide window: {error}")))
    }
}

/// Show without activating; must run on the platform main thread.
pub fn show_inactive(window: &tauri::WebviewWindow) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        let tauri_hwnd = window.hwnd().map_err(|error| {
            ovt_core::Error::Unsupported(format!("window handle unavailable: {error}"))
        })?;
        unsafe {
            let _ = ShowWindow(HWND(tauri_hwnd.0), SW_SHOWNOACTIVATE);
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let pointer = window.ns_window().map_err(|error| {
            ovt_core::Error::Unsupported(format!("window handle unavailable: {error}"))
        })? as *mut objc2_app_kit::NSWindow;
        if pointer.is_null() {
            return Err(ovt_core::Error::Unsupported("NSWindow unavailable".into()));
        }
        unsafe {
            (*pointer).orderFrontRegardless();
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = window;
        Err(ovt_core::Error::Unsupported(
            "inactive window presentation".into(),
        ))
    }
}

pub struct Inserters {
    pub inserter: Box<dyn TextInserter>,
    pub context: Box<dyn ContextProvider>,
    pub audio: crate::audio::AudioHandle,
}

impl Default for Inserters {
    fn default() -> Self {
        Self::new()
    }
}

impl Inserters {
    pub fn new() -> Self {
        Self {
            inserter: inserter(),
            context: context_provider(),
            audio: crate::audio::AudioHandle::new(),
        }
    }
}

pub fn permission_status() -> serde_json::Value {
    #[cfg(target_os = "windows")]
    {
        serde_json::json!({
            "microphone": "granted_at_first_capture",
            "accessibility": "not_required",
        })
    }
    #[cfg(target_os = "macos")]
    {
        serde_json::json!({
            "microphone": "av_capture_auto_prompts",
            "accessibility": if macos::accessibility_trusted() { "granted" } else { "missing" },
            "input_monitoring": if macos::input_monitoring_trusted() { "granted" } else { "missing" },
        })
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        serde_json::json!({ "platform": "unsupported" })
    }
}
