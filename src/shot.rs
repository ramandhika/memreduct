//! Screenshot support: save the window's picture as PNG.
//!
//! Primary path (Windows): grab the window client area with GDI `BitBlt`,
//! which always reflects what is actually shown.
//! Fallback: read back the glow default framebuffer (contents after a swap
//! are not guaranteed, so this is only used when GDI is not available).

use eframe::egui;

pub fn capture(ctx: &egui::Context, frame: &mut eframe::Frame) -> Option<(image::RgbaImage, (u32, u32))> {
    #[cfg(windows)]
    {
        if let Some(img) = capture_window() {
            let dims = (img.width(), img.height());
            return Some((img, dims));
        }
    }
    let img = capture_glow(ctx, frame)?;
    let dims = (img.width(), img.height());
    Some((img, dims))
}

// ---------------------------------------------------------------- Windows GDI

#[cfg(windows)]
pub fn capture_window() -> Option<image::RgbaImage> {
    use std::ffi::c_void;

    type HWND = *mut c_void;
    type HDC = *mut c_void;
    type HBITMAP = *mut c_void;

    const SRCCOPY: u32 = 0x00CC_0020;
    const DIB_RGB_COLORS: u32 = 0;

    #[repr(C)]
    struct RECT {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    struct POINT {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    struct BITMAPINFOHEADER {
        bi_size: u32,
        bi_width: i32,
        bi_height: i32,
        bi_planes: u16,
        bi_bit_count: u16,
        bi_compression: u32,
        bi_size_image: u32,
        bi_x_ppm: i32,
        bi_y_ppm: i32,
        bi_clr_used: u32,
        bi_clr_important: u32,
    }

    #[repr(C)]
    struct BITMAPINFO {
        bmi_header: BITMAPINFOHEADER,
    }

    #[link(name = "user32")]
    extern "system" {
        fn FindWindowW(class: *const u16, title: *const u16) -> HWND;
        fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> i32;
        fn ClientToScreen(hwnd: HWND, pt: *mut POINT) -> i32;
        fn GetDC(hwnd: HWND) -> HDC;
        fn ReleaseDC(hwnd: HWND, hdc: HDC) -> i32;
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn CreateCompatibleDC(hdc: HDC) -> HDC;
        fn CreateCompatibleBitmap(hdc: HDC, width: i32, height: i32) -> HBITMAP;
        fn SelectObject(hdc: HDC, obj: HBITMAP) -> HBITMAP;
        fn DeleteObject(obj: HBITMAP) -> i32;
        fn DeleteDC(hdc: HDC) -> i32;
        fn BitBlt(
            dest: HDC,
            dx: i32,
            dy: i32,
            width: i32,
            height: i32,
            src: HDC,
            sx: i32,
            sy: i32,
            rop: u32,
        ) -> i32;
        fn GetDIBits(
            hdc: HDC,
            bitmap: HBITMAP,
            start: u32,
            lines: u32,
            data: *mut c_void,
            info: *const BITMAPINFO,
            usage: u32,
        ) -> i32;
    }

    unsafe {
        let title: Vec<u16> = "Mem Reduct\0".encode_utf16().collect();
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd.is_null() {
            return None;
        }

        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetClientRect(hwnd, &mut rect) == 0 {
            return None;
        }
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        if w <= 0 || h <= 0 || w > 8192 || h > 8192 {
            return None;
        }

        let mut origin = POINT { x: rect.left, y: rect.top };
        let _ = ClientToScreen(hwnd, &mut origin); // not needed for client DC, kept for safety

        let window_dc = GetDC(hwnd);
        if window_dc.is_null() {
            return None;
        }
        let mem_dc = CreateCompatibleDC(window_dc);
        if mem_dc.is_null() {
            ReleaseDC(hwnd, window_dc);
            return None;
        }
        let bitmap = CreateCompatibleBitmap(window_dc, w, h);
        if bitmap.is_null() {
            DeleteDC(mem_dc);
            ReleaseDC(hwnd, window_dc);
            return None;
        }
        let old = SelectObject(mem_dc, bitmap);

        let ok = BitBlt(mem_dc, 0, 0, w, h, window_dc, 0, 0, SRCCOPY);

        let bmi = BITMAPINFO {
            bmi_header: BITMAPINFOHEADER {
                bi_size: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                bi_width: w,
                bi_height: -h, // top-down rows
                bi_planes: 1,
                bi_bit_count: 32,
                bi_compression: 0, // BI_RGB
                bi_size_image: 0,
                bi_x_ppm: 0,
                bi_y_ppm: 0,
                bi_clr_used: 0,
                bi_clr_important: 0,
            },
        };

        let mut out = None;
        if ok != 0 {
            let mut bgra = vec![0u8; (w * h * 4) as usize];
            let lines = GetDIBits(
                mem_dc,
                bitmap,
                0,
                h as u32,
                bgra.as_mut_ptr() as *mut c_void,
                &bmi,
                DIB_RGB_COLORS,
            );
            if lines == h {
                // BGRA -> RGBA
                for px in bgra.chunks_exact_mut(4) {
                    px.swap(0, 2);
                    px[3] = 255; // the captured window is opaque
                }
                out = image::RgbaImage::from_raw(w as u32, h as u32, bgra);
            }
        }

        SelectObject(mem_dc, old);
        DeleteObject(bitmap);
        DeleteDC(mem_dc);
        ReleaseDC(hwnd, window_dc);
        out
    }
}

// ---------------------------------------------------------------- glow fallback

pub fn capture_glow(ctx: &egui::Context, frame: &mut eframe::Frame) -> Option<image::RgbaImage> {
    use eframe::glow::HasContext;
    let gl = frame.gl()?;
    let rect = ctx.screen_rect();
    let ppp = ctx.pixels_per_point();
    let w = (rect.width() * ppp).round() as i32;
    let h = (rect.height() * ppp).round() as i32;
    if w <= 0 || h <= 0 || w > 8192 || h > 8192 {
        return None;
    }
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    unsafe {
        gl.read_pixels(
            0,
            0,
            w,
            h,
            eframe::glow::RGBA,
            eframe::glow::UNSIGNED_BYTE,
            eframe::glow::PixelPackData::Slice(Some(&mut pixels)),
        );
    }
    let img = image::RgbaImage::from_raw(w as u32, h as u32, pixels)?;
    Some(image::imageops::flip_vertical(&img))
}

pub fn save(img: &image::RgbaImage, path: &str) -> Result<(), String> {
    img.save(path).map_err(|e| e.to_string())
}

/// True when the picture clearly contains painted UI rather than a blank
/// or completely uniform framebuffer.
pub fn looks_painted(img: &image::RgbaImage) -> bool {
    let step = ((img.width() * img.height()).max(1) / 2000).max(1) as usize * 4;
    let data = img.as_raw();
    if data.len() < 8 {
        return false;
    }
    let (mut first, mut differing) = (0usize, 0usize);
    let mut samples = 0usize;
    let mut i = 0usize;
    while i + 3 < data.len() {
        let px = (data[i], data[i + 1], data[i + 2]);
        if samples == 0 {
            first = i;
        } else if px != (data[first], data[first + 1], data[first + 2]) {
            differing += 1;
        }
        samples += 1;
        i += step;
    }
    samples > 20 && differing * 20 >= samples
}
