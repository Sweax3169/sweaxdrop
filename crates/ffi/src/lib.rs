//! The SweaxDrop core as a native library for the phone apps.
//!
//! * Android loads `libsweaxdrop.so` and talks to it over JNI
//!   (`com.sweaxstudios.sweaxdrop.Core`).
//! * iOS links `libsweaxdrop.a` and calls the C functions `sd_start` /
//!   `sd_call` with a table of callbacks.
//!
//! Both sides exchange the same JSON as the desktop app; the host only adds
//! file access (content URIs, MediaStore, Photos) through [`Platform`].

use std::ffi::{c_char, c_void, CStr, CString};
use std::io;
use std::sync::{Arc, OnceLock};

use sweaxdrop_core::storage::{OutputHandle, OutputRequest, Platform};
use sweaxdrop_core::{Config, Host};

static HOST: OnceLock<Host> = OnceLock::new();

fn config(data_dir: &str, name: &str, model: &str, platform: &str, discovery: bool, p: Arc<dyn Platform>) -> Config {
    let mut cfg = Config::new(data_dir.into());
    cfg.platform_name = platform.to_string();
    cfg.model = model.to_string();
    if !name.is_empty() {
        cfg.default_name = name.to_string();
    }
    cfg.discovery = discovery;
    cfg.platform = Some(p);
    cfg
}

fn parse_handle(json: &str) -> io::Result<OutputHandle> {
    serde_json::from_str(json).map_err(|e| io::Error::other(format!("host answered {json}: {e}")))
}

// ------------------------------------------------------------------ iOS / C

/// Callbacks the iOS app hands to `sd_start`. Strings passed to them are
/// valid only during the call. Answers are written into `out` (UTF-8) and
/// the length is returned (or -1).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SdCallbacks {
    pub ctx: *mut c_void,
    pub on_event: extern "C" fn(*mut c_void, *const c_char),
    pub on_reply: extern "C" fn(*mut c_void, i32, *const c_char),
    pub open_read: extern "C" fn(*mut c_void, *const c_char) -> i64,
    pub create_output: extern "C" fn(*mut c_void, *const c_char, *mut c_char, i32) -> i32,
    pub finish_output: extern "C" fn(*mut c_void, *const c_char, bool, i64, *mut c_char, i32) -> i32,
    pub set_clipboard: extern "C" fn(*mut c_void, *const c_char),
    pub notify: extern "C" fn(*mut c_void, *const c_char, *const c_char),
}

// The context pointer belongs to the app object that lives as long as the process.
unsafe impl Send for SdCallbacks {}
unsafe impl Sync for SdCallbacks {}

static CB: OnceLock<SdCallbacks> = OnceLock::new();

fn cstr(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

fn answer(f: impl FnOnce(*mut c_char, i32) -> i32) -> Option<String> {
    let mut buf = vec![0u8; 8192];
    let n = f(buf.as_mut_ptr() as *mut c_char, buf.len() as i32);
    if n < 0 {
        return None;
    }
    buf.truncate(n as usize);
    String::from_utf8(buf).ok()
}

struct CPlatform(SdCallbacks);

impl Platform for CPlatform {
    fn open_read(&self, token: &str) -> io::Result<i64> {
        let t = cstr(token);
        let fd = (self.0.open_read)(self.0.ctx, t.as_ptr());
        if fd < 0 {
            return Err(io::Error::other("could not open the file"));
        }
        Ok(fd)
    }
    fn create_output(&self, req: &OutputRequest) -> io::Result<OutputHandle> {
        let j = cstr(&serde_json::to_string(req).unwrap_or_default());
        let out = answer(|b, n| (self.0.create_output)(self.0.ctx, j.as_ptr(), b, n)).ok_or_else(|| io::Error::other("could not create the file"))?;
        parse_handle(&out)
    }
    fn finish_output(&self, handle: &str, ok: bool, mtime: Option<i64>) -> io::Result<String> {
        let h = cstr(handle);
        answer(|b, n| (self.0.finish_output)(self.0.ctx, h.as_ptr(), ok, mtime.unwrap_or(0), b, n)).ok_or_else(|| io::Error::other("could not save the file"))
    }
    fn set_clipboard(&self, text: &str) {
        let t = cstr(text);
        (self.0.set_clipboard)(self.0.ctx, t.as_ptr());
    }
    fn notify(&self, title: &str, body: &str) {
        let (a, b) = (cstr(title), cstr(body));
        (self.0.notify)(self.0.ctx, a.as_ptr(), b.as_ptr());
    }
}

fn emit_event(cb: &SdCallbacks, json: &str) {
    let c = cstr(json);
    (cb.on_event)(cb.ctx, c.as_ptr());
}

fn emit_reply(cb: &SdCallbacks, id: i32, json: &str) {
    let c = cstr(json);
    (cb.on_reply)(cb.ctx, id, c.as_ptr());
}

unsafe fn rstr(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    CStr::from_ptr(p).to_string_lossy().into_owned()
}

/// Starts the core once. Returns false if it could not start.
///
/// # Safety
/// Pointers must be valid NUL-terminated strings; `cb` must stay valid for
/// the life of the process.
#[no_mangle]
pub unsafe extern "C" fn sd_start(data_dir: *const c_char, name: *const c_char, model: *const c_char, platform: *const c_char, discovery: bool, cb: SdCallbacks) -> bool {
    if HOST.get().is_some() {
        return true;
    }
    let _ = CB.set(cb);
    let p: Arc<dyn Platform> = Arc::new(CPlatform(cb));
    let cfg = config(&rstr(data_dir), &rstr(name), &rstr(model), &rstr(platform), discovery, p);
    match Host::start(cfg) {
        Ok(h) => {
            h.node.set_event_sink(Arc::new(move |json: String| emit_event(&cb, &json)));
            HOST.set(h).is_ok()
        }
        Err(e) => {
            eprintln!("sweaxdrop: {e:#}");
            false
        }
    }
}

/// Runs a JSON command; the answer arrives through `on_reply(id, json)`.
///
/// # Safety
/// `req` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn sd_call(id: i32, req: *const c_char) {
    let req = rstr(req);
    let (Some(h), Some(cb)) = (HOST.get(), CB.get().copied()) else { return };
    h.call_async(
        req,
        Box::new(move |out| emit_reply(&cb, id, &out)),
    );
}

/// The TCP port the core listens on (for Bonjour advertising on iOS).
#[no_mangle]
pub extern "C" fn sd_port() -> u16 {
    HOST.get().map(|h| h.node.port()).unwrap_or(0)
}

/// This device's id (16 chars, static string valid until exit).
#[no_mangle]
pub extern "C" fn sd_device_id() -> *const c_char {
    static ID: OnceLock<CString> = OnceLock::new();
    match HOST.get() {
        Some(h) => ID.get_or_init(|| cstr(&h.node.id())).as_ptr(),
        None => std::ptr::null(),
    }
}

// ------------------------------------------------------------------ Android / JNI

#[cfg(target_os = "android")]
mod android {
    use super::*;
    use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
    use jni::sys::{jboolean, jint, JNI_FALSE, JNI_TRUE};
    use jni::{JNIEnv, JavaVM};

    static VM: OnceLock<JavaVM> = OnceLock::new();
    static HOSTCB: OnceLock<GlobalRef> = OnceLock::new();

    fn with_env<R>(f: impl FnOnce(&mut JNIEnv) -> jni::errors::Result<R>) -> Option<R> {
        let vm = VM.get()?;
        let mut env = vm.attach_current_thread_permanently().ok()?;
        let r = f(&mut env);
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        r.ok()
    }

    pub(crate) fn event(method: &str, json: &str) {
        with_env(|env| {
            let cb = HOSTCB.get().unwrap().as_obj();
            let s = env.new_string(json)?;
            env.call_method(cb, method, "(Ljava/lang/String;)V", &[JValue::Object(&s)])?;
            env.delete_local_ref(s)?;
            Ok(())
        });
    }

    struct JniPlatform;

    impl Platform for JniPlatform {
        fn open_read(&self, token: &str) -> io::Result<i64> {
            let fd = with_env(|env| {
                let cb = HOSTCB.get().unwrap().as_obj();
                let s = env.new_string(token)?;
                let fd = env.call_method(cb, "openRead", "(Ljava/lang/String;)I", &[JValue::Object(&s)])?.i()?;
                env.delete_local_ref(s)?;
                Ok(fd)
            })
            .unwrap_or(-1);
            if fd < 0 {
                return Err(io::Error::other("could not open the file"));
            }
            Ok(fd as i64)
        }
        fn create_output(&self, req: &OutputRequest) -> io::Result<OutputHandle> {
            let j = serde_json::to_string(req).unwrap_or_default();
            let out = with_env(|env| {
                let cb = HOSTCB.get().unwrap().as_obj();
                let s = env.new_string(&j)?;
                let v = env.call_method(cb, "createOutput", "(Ljava/lang/String;)Ljava/lang/String;", &[JValue::Object(&s)])?.l()?;
                let r: String = if v.is_null() { String::new() } else { env.get_string(&JString::from(v))?.into() };
                env.delete_local_ref(s)?;
                Ok(r)
            })
            .filter(|s| !s.is_empty())
            .ok_or_else(|| io::Error::other("could not create the file"))?;
            parse_handle(&out)
        }
        fn finish_output(&self, handle: &str, ok: bool, mtime: Option<i64>) -> io::Result<String> {
            with_env(|env| {
                let cb = HOSTCB.get().unwrap().as_obj();
                let h = env.new_string(handle)?;
                let v = env
                    .call_method(
                        cb,
                        "finishOutput",
                        "(Ljava/lang/String;ZJ)Ljava/lang/String;",
                        &[JValue::Object(&h), JValue::Bool(ok as u8), JValue::Long(mtime.unwrap_or(0))],
                    )?
                    .l()?;
                let r: String = if v.is_null() { String::new() } else { env.get_string(&JString::from(v))?.into() };
                env.delete_local_ref(h)?;
                Ok(r)
            })
            .ok_or_else(|| io::Error::other("could not save the file"))
        }
        fn set_clipboard(&self, text: &str) {
            event("setClipboard", text);
        }
        fn notify(&self, title: &str, body: &str) {
            event("notify", &serde_json::json!({"title": title, "body": body}).to_string());
        }
    }

    fn jstr(env: &mut JNIEnv, s: &JString) -> String {
        env.get_string(s).map(|s| s.into()).unwrap_or_default()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_sweaxstudios_sweaxdrop_Core_start(
        mut env: JNIEnv,
        _class: JClass,
        data_dir: JString,
        name: JString,
        model: JString,
        host: JObject,
    ) -> jboolean {
        android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("sweaxdrop"));
        if HOST.get().is_some() {
            return JNI_TRUE;
        }
        let Ok(vm) = env.get_java_vm() else { return JNI_FALSE };
        let _ = VM.set(vm);
        let Ok(g) = env.new_global_ref(host) else { return JNI_FALSE };
        let _ = HOSTCB.set(g);
        let (d, n, m) = (jstr(&mut env, &data_dir), jstr(&mut env, &name), jstr(&mut env, &model));
        let cfg = config(&d, &n, &m, "android", true, Arc::new(JniPlatform));
        match Host::start(cfg) {
            Ok(h) => {
                h.node.set_event_sink(Arc::new(|json: String| event("onEvent", &json)));
                if HOST.set(h).is_ok() {
                    JNI_TRUE
                } else {
                    JNI_FALSE
                }
            }
            Err(e) => {
                log::error!("start failed: {e:#}");
                JNI_FALSE
            }
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_com_sweaxstudios_sweaxdrop_Core_call(mut env: JNIEnv, _class: JClass, id: jint, req: JString) {
        let req = jstr(&mut env, &req);
        let Some(h) = HOST.get() else { return };
        h.call_async(
            req,
            Box::new(move |out| {
                with_env(|env| {
                    let cb = HOSTCB.get().unwrap().as_obj();
                    let s = env.new_string(&out)?;
                    env.call_method(cb, "onReply", "(ILjava/lang/String;)V", &[JValue::Int(id), JValue::Object(&s)])?;
                    env.delete_local_ref(s)?;
                    Ok(())
                });
            }),
        );
    }

    #[no_mangle]
    pub extern "system" fn Java_com_sweaxstudios_sweaxdrop_Core_port(_env: JNIEnv, _class: JClass) -> jint {
        super::sd_port() as jint
    }
}
