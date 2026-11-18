#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_int, c_void};

pub enum SDL_Window {}
pub enum SDL_Renderer {}
pub enum SDL_Texture {}

#[repr(C)]
pub struct SDL_Rect {
    pub x: c_int,
    pub y: c_int,
    pub w: c_int,
    pub h: c_int,
}

// sdl2 declares SDL_Event as a 56-byte union; the extra room is never read
#[repr(C, align(8))]
pub struct SDL_Event {
    bytes: [u8; 64],
}

impl Default for SDL_Event {
    fn default() -> Self {
        Self { bytes: [0; 64] }
    }
}

impl SDL_Event {
    pub fn kind(&self) -> u32 {
        u32::from_ne_bytes([self.bytes[0], self.bytes[1], self.bytes[2], self.bytes[3]])
    }

    pub fn key_repeat(&self) -> bool {
        self.bytes[13] != 0
    }

    pub fn key_scancode(&self) -> i32 {
        i32::from_ne_bytes([
            self.bytes[16],
            self.bytes[17],
            self.bytes[18],
            self.bytes[19],
        ])
    }
}

pub const SDL_INIT_TIMER: u32 = 0x0000_0001;
pub const SDL_INIT_VIDEO: u32 = 0x0000_0020;
pub const SDL_INIT_EVENTS: u32 = 0x0000_4000;
pub const SDL_WINDOWPOS_CENTERED: c_int = 0x2FFF_0000;
pub const SDL_WINDOW_SHOWN: u32 = 0x0000_0004;
pub const SDL_PIXELFORMAT_ARGB8888: u32 = 0x1636_2004;
pub const SDL_TEXTUREACCESS_STREAMING: c_int = 1;
pub const SDL_QUIT: u32 = 0x100;
pub const SDL_KEYDOWN: u32 = 0x300;
pub const SDL_KEYUP: u32 = 0x301;

#[link(name = "SDL2")]
unsafe extern "C" {
    pub fn SDL_Init(flags: u32) -> c_int;
    pub fn SDL_Quit();
    pub fn SDL_GetError() -> *const c_char;
    pub fn SDL_SetHint(name: *const c_char, value: *const c_char) -> c_int;
    pub fn SDL_CreateWindow(
        title: *const c_char,
        x: c_int,
        y: c_int,
        w: c_int,
        h: c_int,
        flags: u32,
    ) -> *mut SDL_Window;
    pub fn SDL_DestroyWindow(window: *mut SDL_Window);
    pub fn SDL_CreateRenderer(
        window: *mut SDL_Window,
        index: c_int,
        flags: u32,
    ) -> *mut SDL_Renderer;
    pub fn SDL_DestroyRenderer(renderer: *mut SDL_Renderer);
    pub fn SDL_CreateTexture(
        renderer: *mut SDL_Renderer,
        format: u32,
        access: c_int,
        w: c_int,
        h: c_int,
    ) -> *mut SDL_Texture;
    pub fn SDL_DestroyTexture(texture: *mut SDL_Texture);
    pub fn SDL_UpdateTexture(
        texture: *mut SDL_Texture,
        rect: *const SDL_Rect,
        pixels: *const c_void,
        pitch: c_int,
    ) -> c_int;
    pub fn SDL_RenderClear(renderer: *mut SDL_Renderer) -> c_int;
    pub fn SDL_RenderCopy(
        renderer: *mut SDL_Renderer,
        texture: *mut SDL_Texture,
        source: *const SDL_Rect,
        target: *const SDL_Rect,
    ) -> c_int;
    pub fn SDL_RenderPresent(renderer: *mut SDL_Renderer);
    pub fn SDL_RenderReadPixels(
        renderer: *mut SDL_Renderer,
        rect: *const SDL_Rect,
        format: u32,
        pixels: *mut c_void,
        pitch: c_int,
    ) -> c_int;
    pub fn SDL_PollEvent(event: *mut SDL_Event) -> c_int;
    pub fn SDL_GetKeyboardState(count: *mut c_int) -> *const u8;
    pub fn SDL_GetScancodeFromName(name: *const c_char) -> c_int;
    pub fn SDL_GetTicks64() -> u64;
    pub fn SDL_GetPerformanceCounter() -> u64;
    pub fn SDL_GetPerformanceFrequency() -> u64;
    pub fn SDL_Delay(milliseconds: u32);
}
