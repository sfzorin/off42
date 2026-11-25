//! WIA 2 acquisition in an isolated STA. The system dialog owns cancellation.
pub type Pages = Vec<(u32, u32, Vec<u8>, u32, u32)>;
pub type ResultMessage = (u64, Result<Pages, String>);

pub fn start(
    id: u64,
    parent: usize,
    byte_budget: usize,
    page_budget: usize,
) -> std::sync::mpsc::Receiver<ResultMessage> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send((id, acquire_sta(parent, byte_budget, page_budget)));
    });
    rx
}

#[cfg(not(windows))]
fn acquire_sta(_: usize, _: usize, _: usize) -> Result<Pages, String> {
    Err("WIA scanner acquisition is available only on Windows".into())
}

#[cfg(windows)]
fn acquire_sta(parent: usize, byte_budget: usize, page_budget: usize) -> Result<Pages, String> {
    use windows::{
        core::BSTR,
        Win32::{
            Devices::ImageAcquisition::{IWiaDevMgr2, IWiaItem2, WiaDevMgr2},
            Foundation::HWND,
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
                CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED,
            },
        },
    };
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| e.to_string())?;
        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                unsafe {
                    CoUninitialize();
                }
            }
        }
        let _apartment = Apartment;
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let manager: IWiaDevMgr2 = CoCreateInstance(&WiaDevMgr2, None, CLSCTX_LOCAL_SERVER)
            .map_err(|e| format!("WIA unavailable: {e}"))?;
        let folder = BSTR::from(dir.path().to_string_lossy().as_ref());
        let filename = BSTR::from("scan");
        let mut count = 0;
        let mut paths: *mut BSTR = std::ptr::null_mut();
        let mut item: Option<IWiaItem2> = None;
        let result = manager.GetImageDlg(
            0,
            &BSTR::new(),
            HWND(parent as *mut core::ffi::c_void),
            &folder,
            &filename,
            &mut count,
            &mut paths,
            &mut item,
        );
        // Always reclaim the BSTR array, including when the driver reports an error.
        let mut names = Vec::new();
        if !paths.is_null() {
            if (0..=1024).contains(&count) {
                for i in 0..count as usize {
                    names.push(std::ptr::read(paths.add(i)).to_string());
                }
            }
            CoTaskMemFree(Some(paths.cast()));
        }
        result.map_err(|e| format!("WIA acquisition failed or was cancelled: {e}"))?;
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let mut pages = Vec::new();
        let mut total = 0usize;
        for name in names {
            let remaining_bytes = byte_budget
                .checked_sub(total)
                .ok_or("Acquired images exceed memory limit")?;
            let remaining_pages = page_budget
                .checked_sub(pages.len())
                .ok_or("Acquired images exceed page limit")?;
            let batch = decode_frames(&name, remaining_bytes, remaining_pages)?;
            for page in batch {
                total = total
                    .checked_add(page.2.len())
                    .ok_or("Acquired images exceed memory limit")?;
                pages.push(page);
            }
        }
        Ok(pages)
    }
}

#[cfg(windows)]
unsafe fn decode_frames(
    path: &str,
    byte_budget: usize,
    page_budget: usize,
) -> Result<Pages, String> {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::GENERIC_READ,
            Graphics::Imaging::{
                CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICImagingFactory,
                WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom,
                WICDecodeMetadataCacheOnDemand,
            },
            System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
        },
    };
    let factory: IWICImagingFactory =
        CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| e.to_string())?;
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let decoder = factory
        .CreateDecoderFromFilename(
            PCWSTR(wide.as_ptr()),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )
        .map_err(|e| format!("Cannot decode acquired image: {e}"))?;
    let count = decoder.GetFrameCount().map_err(|e| e.to_string())?;
    if count == 0 || count as usize > page_budget {
        return Err("Acquired image has an unsupported frame count".into());
    }
    let mut pages = Vec::new();
    let mut total = 0usize;
    for i in 0..count {
        let frame = decoder.GetFrame(i).map_err(|e| e.to_string())?;
        let converter = factory.CreateFormatConverter().map_err(|e| e.to_string())?;
        converter
            .Initialize(
                &frame,
                &GUID_WICPixelFormat32bppRGBA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(|e| e.to_string())?;
        let (mut w, mut h) = (0, 0);
        converter
            .GetSize(&mut w, &mut h)
            .map_err(|e| e.to_string())?;
        let size = (w as usize)
            .checked_mul(h as usize)
            .and_then(|v| v.checked_mul(4))
            .ok_or("Invalid acquired dimensions")?;
        total = total.checked_add(size).ok_or("Acquired images too large")?;
        if w == 0 || h == 0 || w > 10000 || h > 10000 || total > byte_budget {
            return Err("Acquired images exceed page/memory limits".into());
        }
        let mut pixels = vec![0; size];
        converter
            .CopyPixels(std::ptr::null(), w * 4, &mut pixels)
            .map_err(|e| e.to_string())?;
        let (mut dx, mut dy) = (0.0f64, 0.0f64);
        let (dx, dy) = if frame.GetResolution(&mut dx, &mut dy).is_ok()
            && dx.is_finite()
            && dy.is_finite()
            && (1.0..=9600.0).contains(&dx)
            && (1.0..=9600.0).contains(&dy)
        {
            (dx.round() as u32, dy.round() as u32)
        } else {
            (150, 150)
        };
        pages.push((w, h, pixels, dx, dy));
    }
    Ok(pages)
}
