//! Poster frames for video previews. Windows asks the shell for the same
//! thumbnail Explorer shows; other platforms and missing codecs return None.
use std::path::Path;

#[cfg(not(target_os = "windows"))]
pub fn poster(_: &Path) -> Option<Vec<u8>> { None }

#[cfg(target_os = "windows")]
pub fn poster(path: &Path) -> Option<Vec<u8>> {
    let (width, height, mut pixels) = windows::poster(path)?;
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
        if pixel[3] == 0 { pixel[3] = 255; }
    }
    let mut png = Vec::new();
    image::ImageEncoder::write_image(image::codecs::png::PngEncoder::new(&mut png), &pixels, width, height, image::ExtendedColorType::Rgba8).ok()?;
    Some(png)
}

#[cfg(target_os = "windows")]
mod windows {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_THUMBNAILONLY};

    pub fn poster(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
        let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
        let result = poster_inner(path);
        if initialized { unsafe { CoUninitialize() } }
        result
    }

    fn poster_inner(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
        unsafe {
            let wide = path.as_os_str().encode_wide().collect::<Vec<u16>>();
            let name = HSTRING::from_wide(&wide);
            let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&name, None).ok()?;
            let bitmap = factory.GetImage(SIZE { cx: 640, cy: 400 }, SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK).ok()?;
            let mut info = BITMAP::default();
            let read = GetObjectW(HGDIOBJ(bitmap.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut info as *mut BITMAP as *mut std::ffi::c_void));
            if read == 0 || info.bmWidth <= 0 || info.bmHeight <= 0 { let _ = DeleteObject(HGDIOBJ(bitmap.0)); return None; }
            let (width, height) = (info.bmWidth as u32, info.bmHeight as u32);
            let mut pixels = vec![0u8; width as usize * height as usize * 4];
            let mut header = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: info.bmWidth,
                    biHeight: -info.bmHeight,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: 0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let dc = CreateCompatibleDC(None);
            let lines = GetDIBits(dc, bitmap, 0, height, Some(pixels.as_mut_ptr() as *mut _), &mut header, DIB_RGB_COLORS);
            let _ = DeleteDC(dc);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            if lines == 0 { return None; }
            Some((width, height, pixels))
        }
    }
}
