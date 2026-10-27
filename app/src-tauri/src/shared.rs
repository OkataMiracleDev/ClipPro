//! WebView2 SharedBuffer transport (ADR-002): frame bytes go into shared memory that script
//! maps directly, with no copy through the custom-protocol stream. Script receives them as a
//! `sharedbufferreceived` event on `window.chrome.webview`.

use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2Environment12, ICoreWebView2_17, COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY};
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
