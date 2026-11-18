#![allow(clippy::missing_safety_doc)]

mod ffi;
mod frame;

use aelys_native::aelys_module;

#[aelys_module(name = "sdl", version = "0.1.0")]
mod exports {
    use crate::ffi;
    use crate::frame::{self, Screen};
    use std::cell::RefCell;
    use std::ffi::{CStr, CString, c_int};

    pub const EVENT_NONE: i64 = 0;
    pub const EVENT_QUIT: i64 = 1;
    pub const EVENT_KEY_DOWN: i64 = 2;
    pub const EVENT_KEY_UP: i64 = 3;
    pub const EVENT_OTHER: i64 = 4;

    struct State {
        window: *mut ffi::SDL_Window,
        renderer: *mut ffi::SDL_Renderer,
        texture: *mut ffi::SDL_Texture,
        screen: Screen,
        scale: usize,
        converted: Vec<u8>,
        last_key: i64,
    }

    impl Drop for State {
        fn drop(&mut self) {
            unsafe {
                ffi::SDL_DestroyTexture(self.texture);
                ffi::SDL_DestroyRenderer(self.renderer);
                ffi::SDL_DestroyWindow(self.window);
                ffi::SDL_Quit();
            }
        }
    }

    thread_local! {
        static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
        static CLOCK_ORIGIN: u64 = unsafe { ffi::SDL_GetPerformanceCounter() };
    }

    fn sdl_error(step: &str) -> String {
        let message = unsafe { CStr::from_ptr(ffi::SDL_GetError()) };
        format!("{step}: {}", message.to_string_lossy())
    }

    fn with_state<T>(body: impl FnOnce(&mut State) -> Result<T, String>) -> Result<T, String> {
        STATE.with(|cell| match cell.borrow_mut().as_mut() {
            Some(state) => body(state),
            None => Err("sdl::open has not been called".to_string()),
        })
    }

    #[aelys_export]
    pub fn open(title: String, width: i64, height: i64, scale: i64) -> Result<(), String> {
        let screen = Screen::new(width, height)?;
        if !(1..=16).contains(&scale) {
            return Err(format!("scale {scale} is outside 1..=16"));
        }
        if STATE.with(|cell| cell.borrow().is_some()) {
            return Err("sdl::open was already called; call sdl::close first".to_string());
        }
        let title = CString::new(title).map_err(|_| "the title contains a NUL byte")?;
        let scale = scale as usize;
        unsafe {
            if ffi::SDL_Init(ffi::SDL_INIT_VIDEO | ffi::SDL_INIT_TIMER | ffi::SDL_INIT_EVENTS) != 0
            {
                return Err(sdl_error("SDL_Init"));
            }
            ffi::SDL_SetHint(c"SDL_RENDER_SCALE_QUALITY".as_ptr(), c"0".as_ptr());
            let window = ffi::SDL_CreateWindow(
                title.as_ptr(),
                ffi::SDL_WINDOWPOS_CENTERED,
                ffi::SDL_WINDOWPOS_CENTERED,
                (screen.width * scale) as c_int,
                (screen.height * scale) as c_int,
                ffi::SDL_WINDOW_SHOWN,
            );
            if window.is_null() {
                let error = sdl_error("SDL_CreateWindow");
                ffi::SDL_Quit();
                return Err(error);
            }
            let renderer = ffi::SDL_CreateRenderer(window, -1, 0);
            if renderer.is_null() {
                let error = sdl_error("SDL_CreateRenderer");
                ffi::SDL_DestroyWindow(window);
                ffi::SDL_Quit();
                return Err(error);
            }
            let texture = ffi::SDL_CreateTexture(
                renderer,
                ffi::SDL_PIXELFORMAT_ARGB8888,
                ffi::SDL_TEXTUREACCESS_STREAMING,
                screen.width as c_int,
                screen.height as c_int,
            );
            if texture.is_null() {
                let error = sdl_error("SDL_CreateTexture");
                ffi::SDL_DestroyRenderer(renderer);
                ffi::SDL_DestroyWindow(window);
                ffi::SDL_Quit();
                return Err(error);
            }
            let converted = vec![0; screen.argb_len()];
            STATE.with(|cell| {
                *cell.borrow_mut() = Some(State {
                    window,
                    renderer,
                    texture,
                    screen,
                    scale,
                    converted,
                    last_key: 0,
                })
            });
        }
        Ok(())
    }

    fn upload(state: &State, argb: &[u8]) -> Result<(), String> {
        unsafe {
            if ffi::SDL_UpdateTexture(
                state.texture,
                std::ptr::null(),
                argb.as_ptr().cast(),
                (state.screen.width * 4) as c_int,
            ) != 0
            {
                return Err(sdl_error("SDL_UpdateTexture"));
            }
            draw(state)?;
            ffi::SDL_RenderPresent(state.renderer);
        }
        Ok(())
    }

    unsafe fn draw(state: &State) -> Result<(), String> {
        unsafe {
            if ffi::SDL_RenderClear(state.renderer) != 0
                || ffi::SDL_RenderCopy(
                    state.renderer,
                    state.texture,
                    std::ptr::null(),
                    std::ptr::null(),
                ) != 0
            {
                return Err(sdl_error("SDL_RenderCopy"));
            }
        }
        Ok(())
    }

    #[aelys_export]
    pub fn present_argb(frame: &[u8]) -> Result<(), String> {
        with_state(|state| {
            state.screen.check_argb(frame)?;
            upload(state, &frame[..state.screen.argb_len()])
        })
    }

    #[aelys_export]
    pub fn present_indexed(pixels: &[u8], palette: &[u8]) -> Result<(), String> {
        with_state(|state| {
            let mut converted = std::mem::take(&mut state.converted);
            let outcome = state
                .screen
                .indexed_to_argb(pixels, palette, &mut converted)
                .and_then(|()| upload(state, &converted));
            state.converted = converted;
            outcome
        })
    }

    #[aelys_export]
    pub fn read_pixel(x: i64, y: i64) -> Result<i64, String> {
        with_state(|state| {
            let (x, y) = state.screen.check_point(x, y)?;
            let rect = ffi::SDL_Rect {
                x: (x * state.scale + state.scale / 2) as c_int,
                y: (y * state.scale + state.scale / 2) as c_int,
                w: 1,
                h: 1,
            };
            let mut pixel: u32 = 0;
            unsafe {
                draw(state)?;
                if ffi::SDL_RenderReadPixels(
                    state.renderer,
                    &rect,
                    ffi::SDL_PIXELFORMAT_ARGB8888,
                    (&mut pixel as *mut u32).cast(),
                    4,
                ) != 0
                {
                    return Err(sdl_error("SDL_RenderReadPixels"));
                }
            }
            Ok(i64::from(pixel & 0x00FF_FFFF))
        })
    }

    #[aelys_export]
    pub fn poll() -> i64 {
        STATE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(state) = slot.as_mut() else {
                return EVENT_NONE;
            };
            let mut event = ffi::SDL_Event::default();
            if unsafe { ffi::SDL_PollEvent(&mut event) } == 0 {
                return EVENT_NONE;
            }
            match event.kind() {
                ffi::SDL_QUIT => EVENT_QUIT,
                ffi::SDL_KEYDOWN | ffi::SDL_KEYUP if event.key_repeat() => EVENT_OTHER,
                kind @ (ffi::SDL_KEYDOWN | ffi::SDL_KEYUP) => {
                    state.last_key = i64::from(event.key_scancode());
                    if kind == ffi::SDL_KEYDOWN {
                        EVENT_KEY_DOWN
                    } else {
                        EVENT_KEY_UP
                    }
                }
                _ => EVENT_OTHER,
            }
        })
    }

    #[aelys_export]
    pub fn event_key() -> i64 {
        STATE.with(|cell| cell.borrow().as_ref().map_or(0, |state| state.last_key))
    }

    #[aelys_export]
    pub fn key_down(scancode: i64) -> bool {
        if STATE.with(|cell| cell.borrow().is_none()) || scancode < 0 {
            return false;
        }
        let mut count: c_int = 0;
        let keys = unsafe { ffi::SDL_GetKeyboardState(&mut count) };
        if keys.is_null() || scancode >= i64::from(count) {
            return false;
        }
        unsafe { *keys.add(scancode as usize) != 0 }
    }

    #[aelys_export]
    pub fn scancode(name: String) -> i64 {
        let Ok(name) = CString::new(name) else {
            return 0;
        };
        i64::from(unsafe { ffi::SDL_GetScancodeFromName(name.as_ptr()) })
    }

    #[aelys_export]
    pub fn ticks_ms() -> i64 {
        unsafe { ffi::SDL_GetTicks64() as i64 }
    }

    #[aelys_export]
    pub fn now_ns() -> i64 {
        let origin = CLOCK_ORIGIN.with(|origin| *origin);
        let (counter, frequency) = unsafe {
            (
                ffi::SDL_GetPerformanceCounter(),
                ffi::SDL_GetPerformanceFrequency(),
            )
        };
        frame::counter_to_ns(counter.wrapping_sub(origin), frequency)
    }

    #[aelys_export]
    pub fn delay_ms(milliseconds: i64) {
        unsafe { ffi::SDL_Delay(milliseconds.clamp(0, i64::from(u32::MAX)) as u32) }
    }

    #[aelys_export]
    pub fn close() {
        STATE.with(|cell| drop(cell.borrow_mut().take()));
    }
}
