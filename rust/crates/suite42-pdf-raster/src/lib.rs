#![cfg(windows)]
//! Windows PDF rendering runs in a dedicated WinRT MTA. Requests use immutable
//! serialized snapshots; responses carry a generation so stale work is ignored.
use std::sync::mpsc::{self, Receiver, Sender};
use windows::{
    Data::Pdf::{PdfDocument, PdfPageRenderOptions},
    Graphics::Imaging::{
        BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform, ColorManagementMode,
        ExifOrientationMode,
    },
    Storage::Streams::{DataWriter, InMemoryRandomAccessStream},
    Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
};

pub struct Request {
    pub generation: u64,
    pub bytes: Vec<u8>,
    pub page: u32,
    pub scale: f64,
}
pub struct Response {
    pub generation: u64,
    pub result: Result<(u32, u32, Vec<u8>), String>,
}
pub struct Worker {
    tx: Sender<Request>,
    pub rx: Receiver<Response>,
}
impl Worker {
    pub fn new() -> Self {
        let (tx, requests) = mpsc::channel::<Request>();
        let (responses, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let initialized = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
            while let Ok(mut request) = requests.recv() {
                while let Ok(newer) = requests.try_recv() {
                    request = newer;
                }
                let result = match &initialized {
                    Ok(()) => render(&request).map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                if responses
                    .send(Response {
                        generation: request.generation,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
            }
            if initialized.is_ok() {
                unsafe { RoUninitialize() };
            }
        });
        Self { tx, rx }
    }
    pub fn submit(&self, request: Request) -> Result<(), String> {
        self.tx
            .send(request)
            .map_err(|_| "PDF renderer stopped".into())
    }
}
pub fn render(request: &Request) -> windows::core::Result<(u32, u32, Vec<u8>)> {
    let input = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&input.GetOutputStreamAt(0)?)?;
    writer.WriteBytes(&request.bytes)?;
    writer.StoreAsync()?.join()?;
    writer.DetachStream()?;
    input.Seek(0)?;
    let document = PdfDocument::LoadFromStreamAsync(&input)?.join()?;
    let page = document.GetPage(request.page)?;
    let size = page.Size()?;
    let scale = request.scale.clamp(0.25, 3.125);
    let width = ((size.Width as f64 * scale).round() as u32).clamp(1, 4096);
    let height = ((size.Height as f64 * scale).round() as u32).clamp(1, 4096);
    let options = PdfPageRenderOptions::new()?;
    options.SetDestinationWidth(width)?;
    options.SetDestinationHeight(height)?;
    let output = InMemoryRandomAccessStream::new()?;
    page.RenderWithOptionsToStreamAsync(&output, &options)?
        .join()?;
    page.Close()?;
    output.Seek(0)?;
    let decoder = BitmapDecoder::CreateAsync(&output)?.join()?;
    let width = decoder.PixelWidth()?;
    let height = decoder.PixelHeight()?;
    let transform = BitmapTransform::new()?;
    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Straight,
            &transform,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .join()?
        .DetachPixelData()?;
    Ok((width, height, pixels.to_vec()))
}

/// A WinRT apartment must live for the lifetime of a PDF session on the same thread.
pub struct Apartment;
impl Apartment {
    pub fn new() -> Result<Self, String> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|e| e.to_string())?;
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() }
    }
}
/// A loaded snapshot, used only on its creating MTA thread.
pub struct Session {
    document: PdfDocument,
    _apartment: Apartment,
}
impl Session {
    pub fn open(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 64 * 1024 * 1024 {
            return Err("PDF exceeds 64 MB import limit".into());
        }
        let apartment = Apartment::new()?;
        let load = || -> windows::core::Result<PdfDocument> {
            let input = InMemoryRandomAccessStream::new()?;
            let writer = DataWriter::CreateDataWriter(&input.GetOutputStreamAt(0)?)?;
            writer.WriteBytes(bytes)?;
            writer.StoreAsync()?.join()?;
            writer.DetachStream()?;
            input.Seek(0)?;
            PdfDocument::LoadFromStreamAsync(&input)?.join()
        };
        Ok(Self {
            document: load().map_err(|e| e.to_string())?,
            _apartment: apartment,
        })
    }
    pub fn page_count(&self) -> Result<u32, String> {
        self.document.PageCount().map_err(|e| e.to_string())
    }
    pub fn render_page(&self, page_index: u32, dpi: u32) -> Result<(u32, u32, Vec<u8>), String> {
        if !(36..=600).contains(&dpi) {
            return Err("DPI must be 36–600".into());
        }
        let render = || -> windows::core::Result<(u32, u32, Vec<u8>)> {
            let page = self.document.GetPage(page_index)?;
            let size = page.Size()?;
            let width = (size.Width as f64 * dpi as f64 / 96.0).round() as u64;
            let height = (size.Height as f64 * dpi as f64 / 96.0).round() as u64;
            if width == 0
                || height == 0
                || width > 10000
                || height > 10000
                || width * height > 4_000_000
            {
                return Err(windows::core::Error::new(
                    windows::core::HRESULT(0x80070057u32 as i32),
                    "Page exceeds scanner pixel limits; choose a lower DPI",
                ));
            }
            let options = PdfPageRenderOptions::new()?;
            options.SetDestinationWidth(width as u32)?;
            options.SetDestinationHeight(height as u32)?;
            let output = InMemoryRandomAccessStream::new()?;
            page.RenderWithOptionsToStreamAsync(&output, &options)?
                .join()?;
            page.Close()?;
            output.Seek(0)?;
            let decoder = BitmapDecoder::CreateAsync(&output)?.join()?;
            let transform = BitmapTransform::new()?;
            let pixels = decoder
                .GetPixelDataTransformedAsync(
                    BitmapPixelFormat::Rgba8,
                    BitmapAlphaMode::Straight,
                    &transform,
                    ExifOrientationMode::IgnoreExifOrientation,
                    ColorManagementMode::DoNotColorManage,
                )?
                .join()?
                .DetachPixelData()?;
            Ok((
                decoder.PixelWidth()?,
                decoder.PixelHeight()?,
                pixels.to_vec(),
            ))
        };
        render().map_err(|e| e.to_string())
    }
}
