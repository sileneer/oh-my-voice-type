//! Windows integration:
//! - `WindowsInserter`: clipboard write + synthesized paste (SendInput),
//!   clipboard restore, elevated-target detection.
//! - `WindowsContext`: foreground window app name / title.
//! - `WindowsHotkey`: low-level keyboard hook (`WH_KEYBOARD_LL`) feeding the
//!   shared gesture decoder.
//!
//! The hook does not suppress the activation key; chords therefore keep
//! working for the target app (the gesture decoder cancels takes on chords).

use crate::platform::{HotkeyCallback, HotkeyHandle, HotkeyListener, OutputMuteGuard};
use ovt_core::hotkey::{Edge, GestureDecoder, HotkeyEvent};
use ovt_core::Result;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eConsole, eRender, ERole, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL, VK_INSERT, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetClassNameW, GetForegroundWindow, GetMessageW,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

pub use super::{ContextProvider, TargetInfo, TextInserter};

static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn set_session_active(active: bool) {
    SESSION_ACTIVE.store(active, Ordering::SeqCst);
}

// ---------------------------------------------------------------------------
// Insertion
// ---------------------------------------------------------------------------

pub struct WindowsInserter;

impl Default for WindowsInserter {
    fn default() -> Self {
        Self
    }
}

impl TextInserter for WindowsInserter {
    fn insert(&self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        ensure_not_elevated()?;
        paste_with_restore(text)
    }

    fn copy(&self, text: &str) -> Result<()> {
        clipboard_set_text(text)
    }
}

fn paste_with_restore(text: &str) -> Result<()> {
    let previous = clipboard_get_text();
    clipboard_set_text(text)?;
    std::thread::sleep(Duration::from_millis(30));
    send_paste_keystrokes()?;
    let inserted = text.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(700));
        if clipboard_get_text().as_deref() == Some(inserted.as_str()) {
            if let Some(previous) = previous {
                let _ = clipboard_set_text(&previous);
            }
        }
    });
    Ok(())
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

fn send_paste_keystrokes() -> Result<()> {
    let (mods, key) = paste_shortcut_for_foreground();
    let mut inputs: Vec<INPUT> = Vec::new();
    for &m in &mods {
        inputs.push(key_input(m, false));
    }
    inputs.push(key_input(key, false));
    inputs.push(key_input(key, true));
    for &m in mods.iter().rev() {
        inputs.push(key_input(m, true));
    }
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent != inputs.len() as u32 {
        return Err(ovt_core::Error::Insert(
            "the focused window blocks simulated input (elevated app?) — text copied to the clipboard instead"
                .into(),
        ));
    }
    Ok(())
}

fn key_input(vk: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Ctrl+V is wrong in most terminals; route per app.
fn paste_shortcut_for(class_and_exe: &str) -> (Vec<u16>, u16) {
    let name = class_and_exe.to_lowercase();
    let is_terminalish = ["consolewindowclass", "conhost", "mintty", "windowsterminal"]
        .iter()
        .any(|t| name.contains(t));
    if is_terminalish {
        (vec![VK_SHIFT.0], VK_INSERT.0)
    } else {
        (vec![VK_CONTROL.0], VK_V.0)
    }
}

fn paste_shortcut_for_foreground() -> (Vec<u16>, u16) {
    let exe = foreground_exe();
    let class = foreground_class();
    paste_shortcut_for(&format!("{class} {exe}"))
}

fn foreground_class() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut buf = [0u16; 256];
        let n = GetClassNameW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..(n.max(0) as usize).min(256)])
    }
}

fn foreground_exe() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return String::new();
        }
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if ok {
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            path.rsplit(['\\', '/']).next().unwrap_or("").to_string()
        } else {
            String::new()
        }
    }
}

/// Refuse politely when the target app runs elevated: SendInput from a
/// non-elevated process cannot reach UIPI-elevated windows.
fn ensure_not_elevated() -> Result<()> {
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return Ok(());
        }
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => {
                let _ = CloseHandle(h);
                Ok(())
            }
            Err(_) => Err(ovt_core::Error::Insert(
                "the focused app runs elevated (as administrator); run this app elevated too, or the text is copied to the clipboard instead"
                    .into(),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Foreground context
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct WindowsContext;

impl WindowsContext {
    fn base_target(&self) -> TargetInfo {
        let hwnd = unsafe { GetForegroundWindow() };
        let pid = foreground_pid(hwnd);
        let title = window_title(hwnd);
        let class = foreground_class();
        let exe = foreground_exe();
        let app_name = if !exe.is_empty() {
            exe.trim_end_matches(".exe").to_string()
        } else if !class.is_empty() {
            class
        } else {
            "unknown".to_string()
        };
        let stable_target_id = Some(format!("{}:{}", pid.unwrap_or(0), hwnd.0 as isize));
        TargetInfo {
            app_name: Some(app_name),
            window_title: if title.is_empty() { None } else { Some(title) },
            process_id: pid,
            stable_target_id,
            accessibility_ok: automation().is_ok(),
            ..TargetInfo::default()
        }
    }
}

impl ContextProvider for WindowsContext {
    fn target(&self) -> TargetInfo {
        self.base_target()
    }

    fn capture(&self) -> Result<TargetInfo> {
        Ok(self.base_target())
    }

    fn capture_selection(&self) -> Result<TargetInfo> {
        let mut info = self.base_target();
        let automation = automation()?;
        let element = unsafe { automation.GetFocusedElement() }
            .map_err(|error| accessibility_error("focused element", error))?;
        let secure = unsafe { element.CurrentIsPassword() }
            .map(|value| value.as_bool())
            .unwrap_or(false);
        if secure {
            return Err(ovt_core::Error::Insert(
                "the focused field is a password field; selection editing is blocked".into(),
            ));
        }
        let pattern =
            unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
                .map_err(|_| {
                ovt_core::Error::Unsupported(
                    "the focused control does not expose a supported UI Automation text selection"
                        .into(),
                )
            })?;
        let ranges = unsafe { pattern.GetSelection() }
            .map_err(|error| accessibility_error("text selection", error))?;
        if unsafe { ranges.Length() }.unwrap_or(0) != 1 {
            return Err(ovt_core::Error::Insert(
                "select non-empty text in the target app before editing".into(),
            ));
        }
        let range = unsafe { ranges.GetElement(0) }
            .map_err(|error| accessibility_error("selected text range", error))?;
        let text = unsafe { range.GetText(-1) }
            .map_err(|error| accessibility_error("selected text", error))?;
        let selection = text.to_string();
        if selection.is_empty() {
            return Err(ovt_core::Error::Insert(
                "the selection is empty; select text before editing".into(),
            ));
        }
        let element_pid = unsafe { element.CurrentProcessId() }
            .ok()
            .map(|pid| pid as u32);
        let element_hwnd =
            unsafe { element.CurrentNativeWindowHandle() }.unwrap_or(HWND(std::ptr::null_mut()));
        if element_pid.is_some() {
            info.process_id = element_pid;
        }
        if !element_hwnd.0.is_null() {
            info.stable_target_id = Some(format!(
                "{}:{}",
                info.process_id.unwrap_or(0),
                element_hwnd.0 as isize
            ));
        }
        info.selection_hash = Some(super::revision_hash(
            &info.stable_target_id,
            &Some(selection.clone()),
            &info.app_name,
        ));
        info.selection_revision = info.selection_hash.clone();
        info.selection = Some(selection);
        info.selection_supported = true;
        info.accessibility_ok = true;
        Ok(info)
    }

    fn replace_selection(&self, expected: &TargetInfo, text: &str) -> Result<()> {
        let current = self.capture_selection()?;
        if current.secure_field {
            return Err(ovt_core::Error::Insert(
                "refusing to replace text in a secure field".into(),
            ));
        }
        if current.stable_target_id != expected.stable_target_id
            || current.selection_revision != expected.selection_revision
            || current.selection_hash != expected.selection_hash
        {
            return Err(ovt_core::Error::Insert(
                "the target or selection changed; text was copied for manual paste instead".into(),
            ));
        }
        let automation = automation()?;
        let element = unsafe { automation.GetFocusedElement() }
            .map_err(|error| accessibility_error("focused element", error))?;
        let pattern =
            unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
                .map_err(|_| {
                ovt_core::Error::Unsupported(
                    "this control does not support UI Automation selection replacement".into(),
                )
            })?;
        let ranges = unsafe { pattern.GetSelection() }
            .map_err(|error| accessibility_error("text selection", error))?;
        let range = unsafe { ranges.GetElement(0) }
            .map_err(|error| accessibility_error("selected text range", error))?;
        unsafe { range.Select() }
            .map_err(|error| accessibility_error("selection activation", error))?;
        ensure_not_elevated()?;
        paste_with_restore(text)
    }
}

fn automation() -> Result<IUIAutomation> {
    unsafe {
        // A session worker initializes its own MTA. If the caller already has
        // an STA, RPC_E_CHANGED_MODE is harmless and the existing COM apartment
        // continues to service the interface.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL).map_err(|error| {
            ovt_core::Error::Unsupported(format!("UI Automation is unavailable: {error}"))
        })
    }
}

fn accessibility_error(operation: &str, error: windows::core::Error) -> ovt_core::Error {
    ovt_core::Error::Unsupported(format!("UI Automation {operation} failed: {error}"))
}

fn foreground_pid(hwnd: HWND) -> Option<u32> {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

fn window_title(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; len as usize + 1];
        let count = GetWindowTextW(hwnd, &mut buffer);
        String::from_utf16_lossy(&buffer[..count.max(0) as usize])
    }
}

pub fn is_own_target(target: &TargetInfo) -> bool {
    target.process_id == Some(unsafe { GetCurrentProcessId() })
}

pub fn foreground_work_area() -> Option<(i32, i32, i32, i32)> {
    unsafe {
        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..MONITORINFO::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let area = info.rcWork;
        Some((
            area.left,
            area.top,
            area.right - area.left,
            area.bottom - area.top,
        ))
    }
}

enum MuteCommand {
    Restore(Sender<Result<()>>),
}

struct WindowsMuteGuard {
    commands: Sender<MuteCommand>,
    restored: bool,
}

impl OutputMuteGuard for WindowsMuteGuard {
    fn restore(&mut self) -> Result<()> {
        if self.restored {
            return Ok(());
        }
        let (reply, receiver) = channel();
        self.commands
            .send(MuteCommand::Restore(reply))
            .map_err(|_| ovt_core::Error::Audio("output mute worker stopped".into()))?;
        let result = receiver
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| ovt_core::Error::Audio("output mute restore timed out".into()))?;
        result?;
        self.restored = true;
        Ok(())
    }
}

pub fn mute_output() -> Result<Box<dyn OutputMuteGuard>> {
    let (command_tx, command_rx) = channel::<MuteCommand>();
    let (ready_tx, ready_rx) = channel::<Result<()>>();
    std::thread::Builder::new()
        .name("ovt-output-mute".into())
        .spawn(move || {
            let setup = (|| -> Result<()> {
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                    let enumerator: IMMDeviceEnumerator =
                        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(
                            |error| ovt_core::Error::Audio(format!("audio endpoint: {error}")),
                        )?;
                    let device = enumerator
                        .GetDefaultAudioEndpoint(eRender, ERole(eConsole.0))
                        .map_err(|error| {
                            ovt_core::Error::Audio(format!("default output endpoint: {error}"))
                        })?;
                    let volume: IAudioEndpointVolume =
                        device.Activate(CLSCTX_ALL, None).map_err(|error| {
                            ovt_core::Error::Audio(format!("endpoint volume: {error}"))
                        })?;
                    let was_muted =
                        volume
                            .GetMute()
                            .map(|muted| muted.as_bool())
                            .map_err(|error| {
                                ovt_core::Error::Audio(format!("read output mute: {error}"))
                            })?;
                    volume.SetMute(true, std::ptr::null()).map_err(|error| {
                        ovt_core::Error::Audio(format!("mute system output: {error}"))
                    })?;
                    ready_tx
                        .send(Ok(()))
                        .map_err(|_| ovt_core::Error::Audio("mute caller disconnected".into()))?;
                    loop {
                        match command_rx.recv() {
                            Ok(MuteCommand::Restore(reply)) => {
                                let restored =
                                    volume
                                        .SetMute(was_muted, std::ptr::null())
                                        .map_err(|error| {
                                            ovt_core::Error::Audio(format!(
                                                "restore system output mute: {error}"
                                            ))
                                        });
                                let done = restored.is_ok();
                                let _ = reply.send(restored);
                                if done {
                                    break;
                                }
                            }
                            Err(_) => {
                                let _ = volume.SetMute(was_muted, std::ptr::null());
                                break;
                            }
                        }
                    }
                }
                Ok(())
            })();
            if let Err(error) = setup {
                let _ = ready_tx.send(Err(error));
            }
        })
        .map_err(|error| ovt_core::Error::Audio(format!("start output mute worker: {error}")))?;
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| ovt_core::Error::Audio("output mute startup timed out".into()))??;
    Ok(Box::new(WindowsMuteGuard {
        commands: command_tx,
        restored: false,
    }))
}

// ---------------------------------------------------------------------------
// Global hotkey (low-level keyboard hook)
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct WindowsHotkey;

impl HotkeyListener for WindowsHotkey {
    fn start(
        &self,
        key: &str,
        smart: bool,
        callback: HotkeyCallback,
    ) -> Result<Box<dyn HotkeyHandle + Send + Sync>> {
        let Some(vk) = parse_hotkey_key(key) else {
            return Err(ovt_core::Error::Config(format!(
                "unsupported hotkey {key:?} (use right-alt, left-alt, right-ctrl, f6..f12)"
            )));
        };
        let (tx, rx) = std::sync::mpsc::channel::<Result<u32>>();
        std::thread::Builder::new()
            .name("ovt-hotkey-hook".into())
            .spawn(move || run_hotkey_thread(vk, smart, Duration::from_millis(350), callback, tx))
            .map_err(|e| ovt_core::Error::Unsupported(format!("cannot start hook thread: {e}")))?;
        let thread_id = rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| ovt_core::Error::Unsupported("hotkey hook did not start".into()))??;
        Ok(Box::new(WinHookHandle {
            thread_id: Some(thread_id),
        }))
    }
}

fn parse_hotkey_key(key: &str) -> Option<u16> {
    Some(match key.trim().to_lowercase().as_str() {
        "right-alt" | "rightalt" | "ralt" => 0xA5, // VK_RMENU
        "left-alt" | "leftalt" | "lalt" => 0xA4,   // VK_LMENU
        "right-ctrl" | "rightctrl" | "rctrl" => 0xA3,
        "left-ctrl" | "leftctrl" | "lctrl" => 0xA2,
        "right-shift" | "rightshift" | "rshift" => 0xA1,
        "f6" => 0x75,
        "f7" => 0x76,
        "f8" => 0x77,
        "f9" => 0x78,
        "f10" => 0x79,
        "f11" => 0x7A,
        "f12" => 0x7B,
        "pause" => 0x13,
        _ => return None,
    })
}

struct HookCtx {
    vk: u16,
    decoder: GestureDecoder,
    callback: HotkeyCallback,
    /// The activation key is physically down (used to skip auto-repeat).
    key_down: bool,
}

/// `VK_LMENU` / `VK_RMENU`. A lone Alt press and release activates the menu bar
/// of the focused app, which then swallows the synthesized paste.
fn is_alt_vk(vk: u16) -> bool {
    matches!(vk, 0xA4 | 0xA5)
}

/// Unassigned virtual key (the same "menu mask" key AutoHotkey uses). Tapping
/// it while Alt is down makes Windows treat the Alt press as a chord, so no
/// menu activation happens on release.
const VK_MENU_MASK: u16 = 0xE8;

/// AltGr layouts synthesize an `LCtrl` edge with this scan code around every
/// Right Alt edge; it is not a real chord key.
const ALTGR_FAKE_LCTRL_SCAN: u32 = 0x21D;

fn is_ignored_hook_event(kb: &KBDLLHOOKSTRUCT) -> bool {
    // Our own SendInput (paste keystrokes, the menu mask) must not reach the
    // gesture decoder, where it would look like a chord and cancel the take.
    let injected = kb.flags.0 & LLKHF_INJECTED.0 != 0;
    let altgr_ctrl = kb.vkCode == 0xA2 && kb.scanCode == ALTGR_FAKE_LCTRL_SCAN;
    injected || altgr_ctrl
}

fn send_menu_mask() {
    let inputs = [
        key_input(VK_MENU_MASK, false),
        key_input(VK_MENU_MASK, true),
    ];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

fn run_hotkey_thread(
    vk: u16,
    smart: bool,
    threshold: Duration,
    callback: HotkeyCallback,
    tx: Sender<Result<u32>>,
) {
    let ctx = Arc::new(Mutex::new(HookCtx {
        vk,
        decoder: GestureDecoder::new(threshold, smart),
        callback,
        key_down: false,
    }));
    CTX.with(|slot| {
        *slot.borrow_mut() = Some(ctx.clone());
    });
    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) };
    let Ok(hook) = hook else {
        let _ = tx.send(Err(ovt_core::Error::Unsupported(
            "global keyboard hook could not be installed (Win32 error)".into(),
        )));
        return;
    };
    let _ = tx.send(Ok(unsafe { GetCurrentThreadId() }));
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = UnhookWindowsHookEx(hook);
    }
}

thread_local! {
    static CTX: RefCell<Option<Arc<Mutex<HookCtx>>>> = const { RefCell::new(None) };
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if is_ignored_hook_event(kb) {
            return unsafe { CallNextHookEx(None, code, wparam, lparam) };
        }
        if kb.vkCode == 0x1B && SESSION_ACTIVE.load(Ordering::SeqCst) {
            let ctx_opt = CTX.with(|slot| slot.borrow().clone());
            if let Some(ctx_arc) = ctx_opt {
                if let Ok(mut ctx) = ctx_arc.lock() {
                    ctx.decoder.reset();
                    (ctx.callback)(HotkeyEvent::Cancel);
                }
            }
            return LRESULT(1);
        }
        let edge = match wparam.0 as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => Some(Edge::Down),
            WM_KEYUP | WM_SYSKEYUP => Some(Edge::Up),
            _ => None,
        };
        if let Some(edge) = edge {
            let ctx_opt = CTX.with(|slot| slot.borrow().clone());
            if let Some(ctx_arc) = ctx_opt {
                if let Ok(mut ctx) = ctx_arc.lock() {
                    let is_target = kb.vkCode == u32::from(ctx.vk);
                    if is_target && edge == Edge::Down && !ctx.key_down && is_alt_vk(ctx.vk) {
                        send_menu_mask();
                    }
                    if is_target {
                        ctx.key_down = edge == Edge::Down;
                    }
                    let event = if is_target {
                        ctx.decoder.on_edge(edge, Instant::now())
                    } else if edge == Edge::Down {
                        ctx.decoder.note_other_key()
                    } else {
                        None
                    };
                    if let Some(ev) = event {
                        (ctx.callback)(ev);
                    }
                }
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

struct WinHookHandle {
    thread_id: Option<u32>,
}

impl HotkeyHandle for WinHookHandle {
    fn stop(&mut self) {
        if let Some(id) = self.thread_id.take() {
            unsafe {
                let _ = PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
    }
}

impl Drop for WinHookHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkey_names_parse() {
        assert_eq!(parse_hotkey_key("right-alt"), Some(0xA5));
        assert_eq!(parse_hotkey_key("right-ctrl"), Some(0xA3));
        assert_eq!(parse_hotkey_key("F8"), Some(0x77));
        assert_eq!(parse_hotkey_key("nope"), None);
    }

    #[test]
    fn injected_and_altgr_events_are_ignored() {
        let event = |vk: u32, scan: u32, flags: u32| KBDLLHOOKSTRUCT {
            vkCode: vk,
            scanCode: scan,
            flags: windows::Win32::UI::WindowsAndMessaging::KBDLLHOOKSTRUCT_FLAGS(flags),
            ..KBDLLHOOKSTRUCT::default()
        };
        // Our own Ctrl+V and menu-mask keystrokes.
        assert!(is_ignored_hook_event(&event(0x56, 0x2F, LLKHF_INJECTED.0)));
        assert!(is_ignored_hook_event(&event(
            u32::from(VK_MENU_MASK),
            0,
            LLKHF_INJECTED.0
        )));
        // AltGr's synthesized left Ctrl.
        assert!(is_ignored_hook_event(&event(
            0xA2,
            ALTGR_FAKE_LCTRL_SCAN,
            0
        )));
        // A real left Ctrl or letter is a genuine chord key.
        assert!(!is_ignored_hook_event(&event(0xA2, 0x1D, 0)));
        assert!(!is_ignored_hook_event(&event(0x41, 0x1E, 0)));
        assert!(is_alt_vk(0xA5) && is_alt_vk(0xA4) && !is_alt_vk(0xA3));
    }

    #[test]
    fn terminal_shortcuts_route() {
        let (mods, key) = paste_shortcut_for("ConsoleWindowClass conhost.exe");
        assert_eq!(mods, vec![VK_SHIFT.0]);
        assert_eq!(key, VK_INSERT.0);
        let (mods, key) = paste_shortcut_for("Chrome_WidgetWin_1 chrome.exe");
        assert_eq!(mods, vec![VK_CONTROL.0]);
        assert_eq!(key, VK_V.0);
    }
}
