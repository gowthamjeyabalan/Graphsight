//! The Windows 11 window material (Mica, Mica Alt, Acrylic) behind a see-through window.
//!
//! The material is drawn by the system behind the window; Graphsight only has to leave the right pixels transparent
//! (the translucency sliders in Settings). On other systems, and on Windows 10, this does nothing.

use crate::theme::Backdrop;

/// Ask Windows to draw `backdrop` behind the window, in its dark or light variant. False when it cannot (Windows 10,
/// Windows 11 before 22H2), so the panels and the graph stay solid.
#[cfg(windows)]
pub fn apply(window: &impl raw_window_handle::HasWindowHandle, backdrop: Backdrop, dark: bool) -> bool {
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Graphics::Dwm::{
        DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMSBT_TABBEDWINDOW, DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
        DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
    };
    use windows_sys::Win32::UI::Controls::MARGINS;

    let Ok(handle) = window.window_handle() else { return false };
    let RawWindowHandle::Win32(h) = handle.as_raw() else { return false };
    let hwnd = h.hwnd.get() as *mut core::ffi::c_void;

    let kind: i32 = match backdrop {
        Backdrop::None => DWMSBT_NONE,
        Backdrop::Mica => DWMSBT_MAINWINDOW,
        Backdrop::MicaAlt => DWMSBT_TABBEDWINDOW,
        Backdrop::Acrylic => DWMSBT_TRANSIENTWINDOW,
    };
    let dark: i32 = dark as i32;
    // Windows 10 and older builds of Windows 11 do not know the backdrop setting: that call fails there.
    unsafe {
        DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE as u32, &dark as *const i32 as *const core::ffi::c_void, 4);
        let ok = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE as u32, &kind as *const i32 as *const core::ffi::c_void, 4) == 0;
        // let the material reach over the whole window, not just the title bar
        let margins = if backdrop == Backdrop::None || !ok { MARGINS::default() } else { MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 } };
        DwmExtendFrameIntoClientArea(hwnd, &margins);
        ok && backdrop != Backdrop::None
    }
}

#[cfg(not(windows))]
pub fn apply(_window: &impl raw_window_handle::HasWindowHandle, _backdrop: Backdrop, _dark: bool) -> bool {
    false
}
