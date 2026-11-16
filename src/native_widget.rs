//! Windows WebView2 host for an isolated HTML block.
use std::{cell::RefCell, rc::Rc, time::{Duration, Instant}};
use gpui::{AsyncApp, Bounds, Pixels};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler, PermissionRequestedEventHandler};
use windows::core::{HSTRING, Interface, PCWSTR, PWSTR, w};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, SetWindowPos, ShowWindow, SW_HIDE,
    SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, WINDOW_EX_STYLE, WS_CHILD, WS_CLIPCHILDREN};

type Slot<T> = Rc<RefCell<Option<T>>>;

struct Host(HWND);
impl Drop for Host { fn drop(&mut self) { unsafe { let _ = DestroyWindow(self.0); } } }

pub struct NativeWidget {
    controller: ICoreWebView2Controller,
    host: Host,
    #[cfg(feature = "devtools")]
    parent: usize,
    _server: crate::widget_server::WidgetServer,
    #[cfg(feature = "devtools")]
    frame: Slot<ICoreWebView2Frame2>,
}

async fn wait<T>(slot: &Slot<T>, cx: &mut AsyncApp) -> Option<T> {
    let started = Instant::now();
    loop {
        if let Some(value) = slot.borrow_mut().take() { return Some(value); }
        if started.elapsed() > Duration::from_secs(20) { return None; }
        cx.background_executor().timer(Duration::from_millis(30)).await;
    }
}

impl NativeWidget {
    pub async fn create(parent: usize, server: crate::widget_server::WidgetServer, cx: &mut AsyncApp) -> Result<Self, String> {
        let host = unsafe { CreateWindowExW(WINDOW_EX_STYLE::default(), w!("STATIC"), PCWSTR::null(), WS_CHILD | WS_CLIPCHILDREN,
            0, 0, 1, 1, Some(HWND(parent as *mut std::ffi::c_void)), None, None, None) }.map(Host).map_err(|e| e.to_string())?;
        let data = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("markdownx").join("WidgetWebView2");
        let env_slot: Slot<windows::core::Result<ICoreWebView2Environment>> = Rc::default();
        let result = env_slot.clone();
        unsafe {
            CreateCoreWebView2EnvironmentWithOptions(PCWSTR::null(), &HSTRING::from(data.as_os_str()), None::<&ICoreWebView2EnvironmentOptions>,
                &CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |hr, env| {
                    *result.borrow_mut() = Some(hr.and_then(|_| env.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))));
                    Ok(())
                })))
        }.map_err(|e| e.to_string())?;
        let env = wait(&env_slot, cx).await.ok_or("WebView2 environment timed out")?.map_err(|e| e.to_string())?;
        let controller_slot: Slot<windows::core::Result<ICoreWebView2Controller>> = Rc::default();
        let result = controller_slot.clone();
        unsafe {
            env.CreateCoreWebView2Controller(host.0, &CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |hr, controller| {
                *result.borrow_mut() = Some(hr.and_then(|_| controller.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))));
                Ok(())
            })))
        }.map_err(|e| e.to_string())?;
        let controller = wait(&controller_slot, cx).await.ok_or("WebView2 controller timed out")?.map_err(|e| e.to_string())?;
        let widget = Self { controller, host, _server: server, #[cfg(feature = "devtools")] parent, #[cfg(feature = "devtools")] frame: Rc::default() };
        widget.configure()?;
        Ok(widget)
    }

    fn configure(&self) -> Result<(), String> {
        let fail = |e: windows::core::Error| e.to_string();
        unsafe {
            let web = self.controller.CoreWebView2().map_err(fail)?;
            let settings = web.Settings().map_err(fail)?;
            settings.SetIsWebMessageEnabled(false).map_err(fail)?;
            settings.SetAreHostObjectsAllowed(false).map_err(fail)?;
            settings.SetAreDevToolsEnabled(false).map_err(fail)?;
            settings.SetAreDefaultContextMenusEnabled(false).map_err(fail)?;
            settings.SetAreDefaultScriptDialogsEnabled(false).map_err(fail)?;
            settings.SetIsStatusBarEnabled(false).map_err(fail)?;
            settings.SetIsZoomControlEnabled(false).map_err(fail)?;
            if let Ok(settings) = settings.cast::<ICoreWebView2Settings3>() { settings.SetAreBrowserAcceleratorKeysEnabled(false).map_err(fail)?; }
            if let Ok(settings) = settings.cast::<ICoreWebView2Settings4>() {
                settings.SetIsPasswordAutosaveEnabled(false).map_err(fail)?;
                settings.SetIsGeneralAutofillEnabled(false).map_err(fail)?;
            }
            if let Ok(controller) = self.controller.cast::<ICoreWebView2Controller4>() { controller.SetAllowExternalDrop(false).map_err(fail)?; }
            let url = self._server.url();
            let expected = url.clone();
            let mut token = 0;
            #[cfg(feature = "devtools")]
            {
                let frame = self.frame.clone();
                let web4: ICoreWebView2_4 = web.cast().map_err(fail)?;
                web4.add_FrameCreated(&webview2_com::FrameCreatedEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args { *frame.borrow_mut() = args.Frame().ok().and_then(|value| value.cast().ok()); }
                    Ok(())
                })), &mut token).map_err(fail)?;
            }
            web.add_NavigationStarting(&NavigationStartingEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut raw = PWSTR::null();
                    let valid = args.Uri(&mut raw).is_ok() && webview2_com::take_pwstr(raw) == expected;
                    args.SetCancel(!valid)?;
                }
                Ok(())
            })), &mut token).map_err(fail)?;
            let restore = web.clone();
            let recovered = Rc::new(std::cell::Cell::new(false));
            web.add_FrameNavigationStarting(&NavigationStartingEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut raw = PWSTR::null();
                    let uri = if args.Uri(&mut raw).is_ok() { webview2_com::take_pwstr(raw) } else { String::new() };
                    let valid = matches!(uri.as_str(), "about:blank" | "about:srcdoc");
                    #[cfg(feature = "devtools")]
                    crate::logging::write(&format!("inline-html: frame navigation allowed={valid} url={uri}"));
                    args.SetCancel(!valid)?;
                    // A cancelled cross-process iframe navigation can leave a
                    // blank frame. Reload once, without allowing hostile code
                    // to turn repeated redirects into an endless reload loop.
                    if !valid && !recovered.replace(true) {
                        let script = HSTRING::from("(()=>{const f=document.querySelector('iframe');if(f)f.srcdoc=f.srcdoc;})()");
                        let _ = restore.ExecuteScript(&script, &webview2_com::ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(()))));
                    }
                }
                Ok(())
            })), &mut token).map_err(fail)?;
            web.add_NewWindowRequested(&NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args { args.SetHandled(true)?; }
                Ok(())
            })), &mut token).map_err(fail)?;
            web.add_PermissionRequested(&PermissionRequestedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args { args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?; }
                Ok(())
            })), &mut token).map_err(fail)?;
            if let Ok(web4) = web.cast::<ICoreWebView2_4>() {
                web4.add_DownloadStarting(&webview2_com::DownloadStartingEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args { args.SetCancel(true)?; args.SetHandled(true)?; }
                    Ok(())
                })), &mut token).map_err(fail)?;
            }
            if let Ok(web18) = web.cast::<ICoreWebView2_18>() {
                web18.add_LaunchingExternalUriScheme(&webview2_com::LaunchingExternalUriSchemeEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args { args.SetCancel(true)?; }
                    Ok(())
                })), &mut token).map_err(fail)?;
            }
            web.Navigate(&HSTRING::from(url)).map_err(fail)?;
        }
        Ok(())
    }

    /// The host is clipped to GPUI's current content mask; the browser keeps
    /// the full block size and is offset behind the clipped child window.
    pub fn layout(&self, bounds: Bounds<Pixels>, clip: Bounds<Pixels>, scale: f32) {
        let visible = bounds.intersect(&clip);
        let coord = |p: Pixels| (f32::from(p) * scale).round() as i32;
        let left = coord(visible.left());
        let top = coord(visible.top());
        let width = coord(visible.right()) - left;
        let height = coord(visible.bottom()) - top;
        if width <= 0 || height <= 0 { self.hide(); return; }
        let rect = RECT { left: coord(bounds.left()) - left, top: coord(bounds.top()) - top,
            right: coord(bounds.right()) - left, bottom: coord(bounds.bottom()) - top };
        unsafe {
            let _ = SetWindowPos(self.host.0, None, left, top, width, height, SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            let _ = self.controller.SetBounds(rect);
            let _ = self.controller.SetIsVisible(true);
        }
    }

    pub fn hide(&self) { unsafe { let _ = self.controller.SetIsVisible(false); let _ = ShowWindow(self.host.0, SW_HIDE); } }

    #[cfg(feature = "devtools")]
    pub fn frame(&self) -> Result<ICoreWebView2Frame2, String> { self.frame.borrow().clone().ok_or_else(|| "interactive iframe not loaded".into()) }

    #[cfg(feature = "devtools")]
    pub fn web(&self) -> Result<ICoreWebView2, String> { unsafe { self.controller.CoreWebView2() }.map_err(|error| error.to_string()) }

    #[cfg(feature = "devtools")]
    pub fn shown_and_size(&self) -> (bool, i32, i32) {
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, GetWindowRect, GWL_STYLE, WS_VISIBLE};
        let mut rect = RECT::default();
        unsafe {
            let _ = GetWindowRect(self.host.0, &mut rect);
            let shown = (GetWindowLongPtrW(self.host.0, GWL_STYLE) as u32 & WS_VISIBLE.0) != 0;
            (shown, rect.right - rect.left, rect.bottom - rect.top)
        }
    }

    #[cfg(feature = "devtools")]
    pub fn geometry_matches(&self, bounds: Bounds<Pixels>, clip: Bounds<Pixels>, scale: f32) -> bool {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::ClientToScreen;
        use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
        let visible = bounds.intersect(&clip);
        let coord = |p: Pixels| (f32::from(p) * scale).round() as i32;
        let mut origin = POINT { x: coord(visible.left()), y: coord(visible.top()) };
        let mut actual = RECT::default();
        unsafe {
            if !ClientToScreen(HWND(self.parent as *mut std::ffi::c_void), &mut origin).as_bool() || GetWindowRect(self.host.0, &mut actual).is_err() { return false; }
        }
        let expected = [origin.x, origin.y, origin.x + coord(visible.right()) - coord(visible.left()), origin.y + coord(visible.bottom()) - coord(visible.top())];
        [actual.left, actual.top, actual.right, actual.bottom].iter().zip(expected).all(|(a, b)| (a - b).abs() <= 2)
    }
}

impl Drop for NativeWidget {
    fn drop(&mut self) { unsafe { let _ = self.controller.Close(); } }
}

#[cfg(feature = "devtools")]
async fn eval(frame: &ICoreWebView2Frame2, expression: &str, cx: &mut AsyncApp) -> Result<serde_json::Value, String> {
    use webview2_com::ExecuteScriptCompletedHandler;
    let slot: Slot<Result<String, String>> = Rc::default();
    let result = slot.clone();
    unsafe {
        frame.ExecuteScript(&HSTRING::from(expression),
            &ExecuteScriptCompletedHandler::create(Box::new(move |hr, json| {
                *result.borrow_mut() = Some(hr.map(|_| json).map_err(|error| error.to_string()));
                Ok(())
            })))
    }.map_err(|error| error.to_string())?;
    let json = wait(&slot, cx).await.ok_or("frame script timed out")??;
    serde_json::from_str(&json).map_err(|error| error.to_string())
}

#[cfg(feature = "devtools")]
async fn cdp(web: &ICoreWebView2, method: &str, params: serde_json::Value, cx: &mut AsyncApp) -> Result<(), String> {
    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    let slot: Slot<Result<(), String>> = Rc::default();
    let result = slot.clone();
    unsafe {
        web.CallDevToolsProtocolMethod(&HSTRING::from(method), &HSTRING::from(params.to_string()),
            &CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |hr, _| {
                *result.borrow_mut() = Some(hr.map_err(|error| error.to_string()));
                Ok(())
            })))
    }.map_err(|error| error.to_string())?;
    wait(&slot, cx).await.ok_or("browser input timed out")?
}

#[cfg(feature = "devtools")]
pub async fn capture_docx(web: ICoreWebView2, frame: ICoreWebView2Frame2, lower: bool, cx: &mut AsyncApp) -> Result<Vec<u8>, String> {
    use base64::Engine;
    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    if lower { eval(&frame, "window.scrollTo(0, 550); true", cx).await?; }
    cx.background_executor().timer(Duration::from_millis(300)).await;
    let slot: Slot<Result<String, String>> = Rc::default();
    let result = slot.clone();
    unsafe {
        web.CallDevToolsProtocolMethod(&HSTRING::from("Page.captureScreenshot"), &HSTRING::from(r#"{"format":"png"}"#),
            &CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |hr, json| {
                *result.borrow_mut() = Some(hr.map(|_| json).map_err(|error| error.to_string()));
                Ok(())
            })))
    }.map_err(|error| error.to_string())?;
    let json = wait(&slot, cx).await.ok_or("capture timed out")??;
    let value: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    let data = value["data"].as_str().ok_or("capture returned no image")?;
    base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| e.to_string())
}

/// Test code inspects the isolated frame through WebView2, never by relaxing
/// the frame's sandbox or granting the content access to the host.
#[cfg(feature = "devtools")]
pub async fn test_frame(web: ICoreWebView2, frame: ICoreWebView2Frame2, phase: &str, cx: &mut AsyncApp) -> Result<serde_json::Value, String> {
    if phase == "video" {
        eval(&frame, "(() => { const v=document.querySelector('video'); v.muted=true; v.play().catch(e=>window.playError=String(e)); const image=new Image(); image.src=v.poster; window.posterProbe=image; return true; })()", cx).await?;
        cx.background_executor().timer(Duration::from_millis(1200)).await;
        eval(&frame, "(() => { const v=document.querySelector('video'); return {time:v.currentTime, width:v.videoWidth, paused:v.paused, poster:window.posterProbe?.naturalWidth, ready:v.readyState, network:v.networkState, error:v.error?.message, playError:window.playError}; })()", cx).await
    } else if phase == "audio" {
        let rect = eval(&frame, "(() => { const a=document.querySelector('audio'); a.muted=true; const r=a.getBoundingClientRect(); return {x:r.x+24,y:r.y+r.height/2}; })()", cx).await?;
        let x = rect["x"].as_f64().ok_or("audio position unavailable")?;
        let y = rect["y"].as_f64().ok_or("audio position unavailable")?;
        cdp(&web, "Input.dispatchMouseEvent", serde_json::json!({"type":"mousePressed", "x":x, "y":y, "button":"left", "clickCount":1}), cx).await?;
        cdp(&web, "Input.dispatchMouseEvent", serde_json::json!({"type":"mouseReleased", "x":x, "y":y, "button":"left", "clickCount":1}), cx).await?;
        cx.background_executor().timer(Duration::from_millis(1200)).await;
        eval(&frame, "(() => { const a=document.querySelector('audio'); return {time:a.currentTime, paused:a.paused, ready:a.readyState, network:a.networkState, error:a.error?.message, playError:window.playError}; })()", cx).await
    } else if phase == "module-identity" {
        eval(&frame, "(() => ({runs:globalThis.moduleRuns || 0}))()", cx).await
    } else if phase == "docx" || phase == "docx-revisions" || phase == "docm" || phase == "dotx" {
        eval(&frame, "(() => { const p=document.getElementById('docx-preview'); return {ready:p?.dataset.ready, text:p?.innerText, tables:p?.querySelectorAll('table').length, images:[...(p?.querySelectorAll('img')||[])].filter(i=>i.naturalWidth>0).length, error:document.getElementById('docx-error')?.innerText}; })()", cx).await
    } else if phase == "epub" {
        eval(&frame, "(() => { const p=document.getElementById('epub-content'); return {text:p?.innerText, images:[...(p?.querySelectorAll('img')||[])].filter(i=>i.naturalWidth>0).length, math:p?.querySelectorAll('math').length, warning:document.getElementById('epub-warning')?.innerText, scripts:p?.querySelectorAll('script').length}; })()", cx).await
    } else if phase == "ofd" || phase == "ofd-next" {
        if phase == "ofd-next" {
            eval(&frame, "document.getElementById('next').click(); true", cx).await?;
            cx.background_executor().timer(Duration::from_millis(1000)).await;
        }
        eval(&frame, "(() => { const c=document.getElementById('ofd-canvas'), d=c.getContext('2d').getImageData(0,0,c.width,c.height).data; let dark=0; for(let i=0;i<d.length;i+=4) if(d[i+3]>0 && d[i]<180 && d[i+1]<180 && d[i+2]<180) dark++; return {ready:c.dataset.ready, page:document.getElementById('page').innerText, warning:document.getElementById('ofd-warning').innerText, status:document.getElementById('diagnostics').innerText, dark}; })()", cx).await
    } else if phase == "navigate" {
        eval(&frame, "(() => { location.href=window.navTarget; return true; })()", cx).await
    } else {
        let rect = eval(&frame, "(() => { const r=document.querySelector('canvas').getBoundingClientRect(); return {x:r.x+r.width/2,y:r.y+r.height/2}; })()", cx).await?;
        let x = rect["x"].as_f64().ok_or("canvas position unavailable")?;
        let y = rect["y"].as_f64().ok_or("canvas position unavailable")?;
        cdp(&web, "Input.dispatchMouseEvent", serde_json::json!({"type":"mousePressed", "x":x, "y":y, "button":"left", "clickCount":1}), cx).await?;
        cdp(&web, "Input.dispatchMouseEvent", serde_json::json!({"type":"mouseReleased", "x":x, "y":y, "button":"left", "clickCount":1}), cx).await?;
        if phase == "demo-external" || phase == "module" {
            eval(&frame, "(() => { const s=getComputedStyle(document.querySelector('canvas')).backgroundImage; const m=s.match(/url\\([\"']?(.*?)[\"']?\\)/); const i=new Image(); if(m)i.src=m[1]; window.cssProbe=i; return s; })()", cx).await?;
        }
        if phase == "inline-style" {
            eval(&frame, "(() => { const s=getComputedStyle(document.querySelector('canvas')); for(const [key, value] of [['inlineBg',s.backgroundImage],['inlineBorder',s.borderImageSource]]) { const m=value.match(/url\\([\"']?(.*?)[\"']?\\)/); const i=new Image(); if(m)i.src=m[1]; window[key]=i; } return true; })()", cx).await?;
        }
        if phase == "fetch" {
            eval(&frame, "fetch('https://example.invalid/').then(()=>document.body.dataset.network='allowed',()=>document.body.dataset.network='blocked'); true", cx).await?;
        }
        cx.background_executor().timer(Duration::from_millis(150)).await;
        eval(&frame, "(() => { const c=document.querySelector('canvas'); const s=c&&getComputedStyle(c); const image=document.querySelector('picture img'), candidates=document.querySelector('picture source')?.srcset || ''; return {score:document.body.innerText, canvas:!!c, color:s&&s.backgroundColor, outline:s&&s.outlineColor, cssImage:window.cssProbe?.naturalWidth, inlineBg:window.inlineBg?.naturalWidth, inlineBorder:window.inlineBorder?.naturalWidth, images:[...document.querySelectorAll('img')].filter(i=>i.naturalWidth>0).length, responsive:!!image?.naturalWidth && candidates.includes(image.currentSrc), network:document.body.dataset.network}; })()", cx).await
    }
}
