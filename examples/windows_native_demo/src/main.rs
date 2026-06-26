#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows_native_demo only runs on Windows.");
}

#[cfg(target_os = "windows")]
fn main() -> erika::Result<()> {
    windows_demo::run()
}

#[cfg(target_os = "windows")]
mod windows_demo {
    use std::ptr;
    use std::thread;
    use std::time::{Duration, Instant};

    use erika::{
        MediaRequest, PlatformSurface, RendererBackendPreference, WgpuSurfaceHandle,
        WgpuSurfaceKind,
        presenter::{PresenterConfig, PresenterRuntime},
    };
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_LEFT, VK_RIGHT, VK_SPACE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW,
        GetClientRect, IDC_ARROW, LoadCursorW, MSG, PM_REMOVE, PeekMessageW, PostQuitMessage,
        RegisterClassW, SW_SHOW, SetWindowTextW, ShowWindow, TranslateMessage, WM_DESTROY,
        WM_KEYDOWN, WM_QUIT, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };

    const SEEK_STEP: Duration = Duration::from_secs(5);
    const TITLE_UPDATE_INTERVAL: Duration = Duration::from_millis(250);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum DemoCommand {
        TogglePlayback,
        SeekBackward,
        SeekForward,
    }

    pub fn run() -> erika::Result<()> {
        let args = std::env::args().collect::<Vec<_>>();
        let media = args.get(1).cloned();
        let hwnd = unsafe { create_window()? };
        let hinstance = unsafe { GetModuleHandleW(ptr::null()) };
        let mut presenter = PresenterRuntime::new(presenter_config())?;
        let (mut width, mut height) = client_size(hwnd);
        let mut scale = dpi_scale(hwnd);
        presenter.attach_surface(PlatformSurface::Wgpu(WgpuSurfaceHandle::new(
            WgpuSurfaceKind::WindowsHwnd,
            hwnd as usize as u64,
            hinstance as usize as u64,
            width,
            height,
            scale,
        )))?;

        if let Some(media) = media {
            presenter.open(MediaRequest::new(media))?;
            presenter.play()?;
        }

        let start = Instant::now();
        let mut last_title_update = Instant::now() - TITLE_UPDATE_INTERVAL;
        let mut running = true;
        while running {
            let (next_running, commands) = pump_messages();
            running = next_running;
            for command in commands {
                apply_command(&mut presenter, command);
            }
            let (next_width, next_height) = client_size(hwnd);
            let next_scale = dpi_scale(hwnd);
            if next_width != width || next_height != height || (next_scale - scale).abs() > 0.001 {
                width = next_width;
                height = next_height;
                scale = next_scale;
                presenter.resize_surface(width, height, scale)?;
            }
            presenter.render_tick(start.elapsed().as_secs_f64())?;
            if last_title_update.elapsed() >= TITLE_UPDATE_INTERVAL {
                update_window_title(hwnd, &presenter);
                last_title_update = Instant::now();
            }
            thread::sleep(Duration::from_millis(16));
        }

        presenter.detach_surface()?;
        Ok(())
    }

    fn presenter_config() -> PresenterConfig {
        let mut config = PresenterConfig::default();
        config.player.renderer = RendererBackendPreference::WgpuFallback;
        config.render_test_pattern_when_idle = true;
        config
    }

    unsafe fn create_window() -> erika::Result<HWND> {
        let hinstance = unsafe { GetModuleHandleW(ptr::null()) };
        let class_name = wide("ErikaWindowsNativeDemo");
        let title = wide("Erika Windows Native Demo");
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
            lpszClassName: class_name.as_ptr(),
            ..Default::default()
        };
        if unsafe { RegisterClassW(&window_class) } == 0 {
            return Err(erika::PlayerError::Renderer(
                "RegisterClassW failed".to_string(),
            ));
        }

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1280,
                720,
                ptr::null_mut(),
                ptr::null_mut(),
                hinstance,
                ptr::null_mut(),
            )
        };
        if hwnd.is_null() {
            return Err(erika::PlayerError::Renderer(
                "CreateWindowExW failed".to_string(),
            ));
        }
        unsafe { ShowWindow(hwnd, SW_SHOW) };
        Ok(hwnd)
    }

    fn pump_messages() -> (bool, Vec<DemoCommand>) {
        let mut msg = MSG::default();
        let mut commands = Vec::new();
        unsafe {
            while PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    return (false, commands);
                }
                if msg.message == WM_KEYDOWN {
                    if let Some(command) = command_from_key(msg.wParam as u16) {
                        commands.push(command);
                    }
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        (true, commands)
    }

    fn command_from_key(key: u16) -> Option<DemoCommand> {
        match key {
            VK_SPACE => Some(DemoCommand::TogglePlayback),
            VK_LEFT => Some(DemoCommand::SeekBackward),
            VK_RIGHT => Some(DemoCommand::SeekForward),
            _ => None,
        }
    }

    fn apply_command(presenter: &mut PresenterRuntime, command: DemoCommand) {
        let result = match command {
            DemoCommand::TogglePlayback => {
                if presenter.is_playing() {
                    presenter.pause()
                } else {
                    presenter.play()
                }
            }
            DemoCommand::SeekBackward => {
                let target = presenter.media_time().saturating_sub(SEEK_STEP);
                presenter.seek(target)
            }
            DemoCommand::SeekForward => {
                let target = presenter
                    .media_time()
                    .checked_add(SEEK_STEP)
                    .unwrap_or(Duration::MAX);
                let target = presenter
                    .duration()
                    .map_or(target, |duration| target.min(duration));
                presenter.seek(target)
            }
        };
        if let Err(error) = result {
            eprintln!("Erika Windows native demo command {command:?} failed: {error}");
        }
    }

    fn update_window_title(hwnd: HWND, presenter: &PresenterRuntime) {
        let snapshot = presenter.runtime_snapshot();
        let duration = snapshot.media_time.as_secs_f64().max(0.0);
        let title = format!(
            "Erika Windows Native Demo  t={duration:.2}s  sw={} hw={} zero={} cpu={} audio_q={} underflow={}",
            snapshot.renderer.software_video_frames,
            snapshot.renderer.hardware_video_frames,
            snapshot.renderer.zero_copy_video_frames,
            snapshot.renderer.cpu_video_frame_fallbacks,
            snapshot.audio_output_queued_frames,
            snapshot.audio_output_underflow_frames,
        );
        let title = wide(&title);
        unsafe {
            SetWindowTextW(hwnd, title.as_ptr());
        }
    }

    fn client_size(hwnd: HWND) -> (u32, u32) {
        let mut rect = RECT::default();
        let ok = unsafe { GetClientRect(hwnd, &mut rect) } != 0;
        if !ok {
            return (1, 1);
        }
        let width = (rect.right - rect.left).max(1) as u32;
        let height = (rect.bottom - rect.top).max(1) as u32;
        (width, height)
    }

    fn dpi_scale(hwnd: HWND) -> f64 {
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 }
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}
