use std::error::Error;
use std::io;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{ScreenToClient, UpdateWindow};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRect, ClipCursor, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect,
    GetCursorPos, GetForegroundWindow, LoadCursorW, PeekMessageW, RegisterClassW, SetCursorPos,
    ShowCursor, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, IDC_ARROW,
    MSG, PM_REMOVE, SW_SHOW, WM_DESTROY, WM_QUIT, WM_SIZE, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

static CLIENT_WIDTH: AtomicU32 = AtomicU32::new(1280);
static CLIENT_HEIGHT: AtomicU32 = AtomicU32::new(720);

pub struct Window {
    hwnd: HWND,
    mouse_captured: bool,
    cursor_hide_count: u32,
    mouse_clip_size: (u32, u32),
}

impl Window {
    pub fn new(width: i32, height: i32) -> Result<Self, Box<dyn Error>> {
        let instance = unsafe { GetModuleHandleW(PCWSTR::null())? };
        let class_name = w!("EclipseD3D11Window");
        let cursor = unsafe { LoadCursorW(None, IDC_ARROW)? };
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: HINSTANCE(instance.0),
            hCursor: cursor,
            lpszClassName: class_name,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&window_class) } == 0 {
            return Err(io::Error::last_os_error().into());
        }

        let style = WS_OVERLAPPEDWINDOW;
        let mut outer_rect = windows::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        unsafe { AdjustWindowRect(&mut outer_rect, style, false)? };

        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                class_name,
                w!("ECLIPSE - Direct3D 11"),
                style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                outer_rect.right - outer_rect.left,
                outer_rect.bottom - outer_rect.top,
                None,
                None,
                Some(HINSTANCE(instance.0)),
                None,
            )?
        };
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = UpdateWindow(hwnd);
        }

        let mut client_rect = windows::Win32::Foundation::RECT::default();
        unsafe { GetClientRect(hwnd, &mut client_rect)? };
        CLIENT_WIDTH.store(
            (client_rect.right - client_rect.left).max(0) as u32,
            Ordering::Relaxed,
        );
        CLIENT_HEIGHT.store(
            (client_rect.bottom - client_rect.top).max(0) as u32,
            Ordering::Relaxed,
        );
        Ok(Self {
            hwnd,
            mouse_captured: false,
            cursor_hide_count: 0,
            mouse_clip_size: (0, 0),
        })
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn client_size(&self) -> (u32, u32) {
        (
            CLIENT_WIDTH.load(Ordering::Relaxed),
            CLIENT_HEIGHT.load(Ordering::Relaxed),
        )
    }

    pub fn is_focused(&self) -> bool {
        unsafe { GetForegroundWindow() == self.hwnd }
    }

    pub fn cursor_position_client(&self) -> Result<[f32; 2], Box<dyn Error>> {
        let mut cursor = POINT::default();
        unsafe { GetCursorPos(&mut cursor)? };
        if !unsafe { ScreenToClient(self.hwnd, &mut cursor) }.as_bool() {
            return Err(io::Error::last_os_error().into());
        }
        Ok([cursor.x as f32, cursor.y as f32])
    }

    pub fn left_mouse_down(&self) -> bool {
        unsafe { GetAsyncKeyState(0x01) as u16 & 0x8000 != 0 }
    }

    pub fn set_mouse_capture(&mut self, capture: bool) -> Result<(), Box<dyn Error>> {
        let size = self.client_size();
        let capture = capture && size.0 > 0 && size.1 > 0;
        if capture == self.mouse_captured {
            if capture && size != self.mouse_clip_size {
                self.update_mouse_clip()?;
            }
            return Ok(());
        }

        if capture {
            self.update_mouse_clip()?;
            if let Err(error) = self.recenter_cursor() {
                unsafe {
                    let _ = ClipCursor(None);
                }
                return Err(error);
            }
            loop {
                self.cursor_hide_count += 1;
                if unsafe { ShowCursor(false) } < 0 {
                    break;
                }
            }
            self.mouse_captured = true;
        } else {
            unsafe { ClipCursor(None)? };
            self.restore_cursor_visibility();
            self.mouse_captured = false;
        }
        Ok(())
    }

    pub fn mouse_delta(&self) -> Result<crate::input::MouseDelta, Box<dyn Error>> {
        if !self.mouse_captured {
            return Ok(crate::input::MouseDelta::default());
        }

        let center = self.client_center_on_screen()?;
        let mut cursor = POINT::default();
        unsafe { GetCursorPos(&mut cursor)? };
        let delta = crate::input::MouseDelta {
            x: (cursor.x - center.x) as f32,
            y: (cursor.y - center.y) as f32,
        };
        if delta.x != 0.0 || delta.y != 0.0 {
            unsafe { SetCursorPos(center.x, center.y)? };
        }
        Ok(delta)
    }

    pub fn pump_events(&self) -> bool {
        let mut message = MSG::default();
        unsafe {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_QUIT {
                    return false;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        true
    }

    fn update_mouse_clip(&mut self) -> Result<(), Box<dyn Error>> {
        let mut client = RECT::default();
        unsafe { GetClientRect(self.hwnd, &mut client)? };
        let mut top_left = POINT {
            x: client.left,
            y: client.top,
        };
        let mut bottom_right = POINT {
            x: client.right,
            y: client.bottom,
        };
        if !unsafe { windows::Win32::Graphics::Gdi::ClientToScreen(self.hwnd, &mut top_left) }
            .as_bool()
            || !unsafe {
                windows::Win32::Graphics::Gdi::ClientToScreen(self.hwnd, &mut bottom_right)
            }
            .as_bool()
        {
            return Err(io::Error::last_os_error().into());
        }

        let screen_rect = RECT {
            left: top_left.x,
            top: top_left.y,
            right: bottom_right.x,
            bottom: bottom_right.y,
        };
        unsafe { ClipCursor(Some(&screen_rect))? };
        self.mouse_clip_size = self.client_size();
        Ok(())
    }

    fn client_center_on_screen(&self) -> Result<POINT, Box<dyn Error>> {
        let (width, height) = self.client_size();
        let mut center = POINT {
            x: (width / 2) as i32,
            y: (height / 2) as i32,
        };
        if !unsafe { windows::Win32::Graphics::Gdi::ClientToScreen(self.hwnd, &mut center) }
            .as_bool()
        {
            return Err(io::Error::last_os_error().into());
        }
        Ok(center)
    }

    fn recenter_cursor(&self) -> Result<(), Box<dyn Error>> {
        let center = self.client_center_on_screen()?;
        unsafe { SetCursorPos(center.x, center.y)? };
        Ok(())
    }

    fn restore_cursor_visibility(&mut self) {
        for _ in 0..self.cursor_hide_count {
            unsafe {
                ShowCursor(true);
            }
        }
        self.cursor_hide_count = 0;
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        if self.mouse_captured {
            unsafe {
                let _ = ClipCursor(None);
            }
            self.restore_cursor_visibility();
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_SIZE => {
            let size = lparam.0 as u32;
            CLIENT_WIDTH.store(size & 0xffff, Ordering::Relaxed);
            CLIENT_HEIGHT.store(size >> 16, Ordering::Relaxed);
            LRESULT(0)
        }
        WM_DESTROY => {
            windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}
