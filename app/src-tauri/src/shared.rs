//! WebView2 SharedBuffer transport (ADR-002): frame bytes go into shared memory that script
//! maps directly, with no copy through the custom-protocol stream. Script receives them as a
//! `sharedbufferreceived` event on `window.chrome.webview`.

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Environment12, ICoreWebView2Environment6, ICoreWebView2_17, ICoreWebView2_7, COREWEBVIEW2_PRINT_ORIENTATION_LANDSCAPE, COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY,
};
use webview2_com::PrintToPdfCompletedHandler;
use windows_core::{Interface, PCWSTR};

/// Copies `data` into a new shared buffer and posts it to the page with `meta` (JSON) as the
/// event's `additionalData`. Runs the COM calls on the UI thread and waits for them.
pub fn post(window: &tauri::WebviewWindow, data: Vec<u8>, meta: String) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    window
        .with_webview(move |pw| {
            let r = (|| -> windows_core::Result<()> {
                unsafe {
                    let env: ICoreWebView2Environment12 = pw.environment().cast()?;
                    let core: ICoreWebView2_17 = pw.controller().CoreWebView2()?.cast()?;
                    let buf = env.CreateSharedBuffer(data.len() as u64)?;
                    let mut p = std::ptr::null_mut();
                    buf.Buffer(&mut p)?;
                    std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
                    let meta: Vec<u16> = meta.encode_utf16().chain(Some(0)).collect();
                    core.PostSharedBufferToScript(&buf, COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY, PCWSTR(meta.as_ptr()))?;
                    // Dropping our reference is fine: script keeps the mapping until it calls
                    // `chrome.webview.releaseBuffer`.
                }
                Ok(())
            })();
            let _ = tx.send(r.map_err(|e| format!("SharedBuffer: {e}")));
        })
        .map_err(|e| format!("SharedBuffer: {e}"))?;
    rx.recv().map_err(|e| format!("SharedBuffer: {e}"))?
}

/// Prints the page in `window` to a PDF at `path` (A4 landscape, backgrounds on, no browser
/// header or footer) with WebView2's `PrintToPdf`, waiting up to a minute for it to finish.
pub fn print_to_pdf(window: &tauri::WebviewWindow, path: &std::path::Path) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let target: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
    window
        .with_webview(move |pw| {
            let tx2 = tx.clone();
            let r = (|| -> windows_core::Result<()> {
                unsafe {
                    let env: ICoreWebView2Environment6 = pw.environment().cast()?;
                    let core: ICoreWebView2_7 = pw.controller().CoreWebView2()?.cast()?;
                    let settings = env.CreatePrintSettings()?;
                    // A4 (inches), turned to landscape; 10 mm margins.
                    settings.SetPageWidth(8.27)?;
                    settings.SetPageHeight(11.69)?;
                    settings.SetOrientation(COREWEBVIEW2_PRINT_ORIENTATION_LANDSCAPE)?;
                    settings.SetMarginTop(0.39)?;
                    settings.SetMarginBottom(0.39)?;
                    settings.SetMarginLeft(0.39)?;
                    settings.SetMarginRight(0.39)?;
                    settings.SetShouldPrintBackgrounds(true)?;
                    settings.SetShouldPrintHeaderAndFooter(false)?;
                    let handler = PrintToPdfCompletedHandler::create(Box::new(move |hr, ok| {
                        let _ = tx2.send(if hr.is_ok() && ok { Ok(()) } else { Err("PDF printing failed".into()) });
                        Ok(())
                    }));
                    core.PrintToPdf(PCWSTR(target.as_ptr()), &settings, &handler)?;
                }
                Ok(())
            })();
            if let Err(e) = r {
                let _ = tx.send(Err(format!("PDF: {e}")));
            }
        })
        .map_err(|e| format!("PDF: {e}"))?;
    rx.recv_timeout(std::time::Duration::from_secs(60)).map_err(|_| "PDF printing timed out".to_string())?
}
