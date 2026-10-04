//! Only this module crosses JNI. Handles are checked IDs, never pointers to Rust
//! objects. Rendering/IO stays on the engine worker, not Android's event thread.
use jni::{
    JNIEnv,
    objects::{JByteBuffer, JClass, JString},
    sys::{jboolean, jint, jlong, jobjectArray},
};
use readall::mobile::{Command, Config, Effect, Reader, Snapshot, UiAction, UiCommand};
use std::{
    collections::BTreeMap,
    error::Error,
    io,
    path::PathBuf,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicI64, Ordering},
    },
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
static READERS: OnceLock<Mutex<BTreeMap<i64, Reader>>> = OnceLock::new();
static NEXT: AtomicI64 = AtomicI64::new(1);
fn readers() -> &'static Mutex<BTreeMap<i64, Reader>> {
    READERS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
fn bad(message: &str) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message))
}
fn with_reader<T>(handle: i64, f: impl FnOnce(&Reader) -> Result<T>) -> Result<T> {
    let readers = readers()
        .lock()
        .map_err(|_| bad("reader registry unavailable"))?;
    f(readers
        .get(&handle)
        .ok_or_else(|| bad("unknown or closed ReadAll handle"))?)
}
fn guard<T: Default>(
    env: &mut JNIEnv<'_>,
    operation: impl FnOnce(&mut JNIEnv<'_>) -> Result<T>,
) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(env))) {
        Ok(Ok(result)) => result,
        result => {
            let message = match result {
                Ok(Err(e)) => e.to_string(),
                _ => "ReadAll JNI operation panicked".into(),
            };
            // A JNI error may already have raised a Java exception; preserve it.
            if !env.exception_check().unwrap_or(true) {
                let _ = env.throw_new("java/lang/IllegalStateException", message);
            }
            T::default()
        }
    }
}
fn text(env: &mut JNIEnv<'_>, value: &JString<'_>) -> Result<String> {
    let result: String = env.get_string(value)?.into();
    if result.len() > 8192 || result.contains('\0') {
        return Err(bad("invalid native path length or NUL"));
    }
    Ok(result)
}
fn number(n: jint) -> Result<u32> {
    u32::try_from(n).map_err(|_| bad("negative native dimension/index"))
}
fn strings(env: &mut JNIEnv<'_>, values: &[String]) -> Result<jobjectArray> {
    let size = i32::try_from(values.len()).map_err(|_| bad("native string array too large"))?;
    let array = env.new_object_array(size, "java/lang/String", jni::objects::JObject::null())?;
    for (index, value) in values.iter().enumerate() {
        let string = env.new_string(value)?;
        env.set_object_array_element(&array, index as i32, &string)?;
        env.delete_local_ref(string)?;
    }
    Ok(array.into_raw())
}
fn state_fields(s: Snapshot) -> Vec<String> {
    let (serial, width, height) = s.frame.as_ref().map_or((0, 0, 0), |f| {
        (f.serial, f.surface.width(), f.surface.height())
    });
    let status = if s.closed {
        "closed"
    } else if s.busy {
        "loading"
    } else {
        "ready"
    };
    vec![
        "2".into(),
        status.into(),
        s.phase.into(),
        s.done.to_string(),
        s.total.to_string(),
        serial.to_string(),
        width.to_string(),
        height.to_string(),
        s.title,
        s.position,
        format!("{:.1}", s.progress * 100.0),
        s.locator,
        s.notice,
        s.revision.to_string(),
        s.ui_mode.into(),
        s.page_mode.into(),
        u8::from(s.animating).to_string(),
        u8::from(s.editing).to_string(),
        s.input,
    ]
}
fn command(code: jint, a: jint, b: jint) -> Result<Command> {
    Ok(match code {
        1 => Command::Next,
        2 => Command::Previous,
        3 => Command::First,
        4 => Command::Last,
        5 => Command::Larger,
        6 => Command::Smaller,
        7 => Command::Contents,
        8 => Command::Jump {
            spine: number(a)? as usize,
            offset: number(b)? as usize,
        },
        9 => Command::CycleTheme,
        10 => Command::Save,
        11 => Command::Bookmark,
        12 => Command::Resize {
            width: number(a)?,
            height: number(b)?,
        },
        13 => Command::Back,
        14 if matches!(a, 0 | 1) => Command::Pause(a != 0),
        20 => Command::Ui(UiAction::Command(UiCommand::Find)),
        21 => Command::Ui(UiAction::Command(UiCommand::Bookmarks)),
        22 => Command::Ui(UiAction::Command(UiCommand::Settings)),
        23 => Command::Ui(UiAction::Command(UiCommand::Select)),
        24 => Command::Ui(UiAction::Command(UiCommand::Copy)),
        25 => Command::Ui(UiAction::Command(UiCommand::Paste)),
        26 => Command::Ui(UiAction::Command(UiCommand::Note)),
        27 => Command::Ui(UiAction::Command(UiCommand::Highlight)),
        28 => Command::Ui(UiAction::Command(UiCommand::Delete)),
        29 => Command::Ui(UiAction::Activate),
        30 => Command::Ui(UiAction::Close),
        31 => Command::Ui(UiAction::Back),
        40..=49 => Command::Touch {
            kind: (code - 40) as u32,
            x: a,
            y: b,
        },
        _ => return Err(bad("unknown mobile reader command")),
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_xin_soymilk_readall_NativeReader_nativeOpen(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    book: JString<'_>,
    font: JString<'_>,
    state: JString<'_>,
    width: jint,
    height: jint,
    size: jint,
    margin: jint,
) -> jlong {
    guard(&mut env, |env| {
        let config = Config {
            book: PathBuf::from(text(env, &book)?),
            font: PathBuf::from(text(env, &font)?),
            state_dir: PathBuf::from(text(env, &state)?),
            width: number(width)?,
            height: number(height)?,
            font_size: number(size)?,
            margin: number(margin)?,
        };
        let mut readers = readers()
            .lock()
            .map_err(|_| bad("reader registry unavailable"))?;
        if readers.len() >= 4 {
            return Err(bad("too many open mobile readers"));
        }
        let id = NEXT
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |id| id.checked_add(1))
            .map_err(|_| bad("reader handle range exhausted"))?;
        readers.insert(id, Reader::open(config)?);
        Ok(id)
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_xin_soymilk_readall_NativeReader_nativeState(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
) -> jobjectArray {
    guard(&mut env, |env| {
        let state = with_reader(handle, |r| Ok(r.snapshot()))?;
        strings(env, &state_fields(state))
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_xin_soymilk_readall_NativeReader_nativeContents(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
) -> jobjectArray {
    guard(&mut env, |env| {
        let snapshot = with_reader(handle, |r| Ok(r.snapshot()))?;
        let fields: Vec<String> = snapshot
            .contents
            .iter()
            .flat_map(|entry| {
                [
                    entry.title.clone(),
                    entry.depth.to_string(),
                    entry.spine.to_string(),
                    entry.offset.to_string(),
                ]
            })
            .collect();
        strings(env, &fields)
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_xin_soymilk_readall_NativeReader_nativeCommand(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    code: jint,
    a: jint,
    b: jint,
) {
    guard(&mut env, |_| {
        with_reader(handle, |r| Ok(r.command(command(code, a, b)?)?))
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_xin_soymilk_readall_NativeReader_nativeCopyPixels(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    serial: jlong,
    buffer: JByteBuffer<'_>,
) -> jboolean {
    guard(&mut env, |env| {
        let Some(frame) = with_reader(handle, |r| Ok(r.snapshot().frame))? else {
            return Ok(0);
        };
        if serial <= 0 || frame.serial != serial as u64 {
            return Ok(0);
        }
        let length = frame.byte_len();
        if env.call_method(&buffer, "isReadOnly", "()Z", &[])?.z()? {
            return Err(bad("pixel buffer must be writable"));
        }
        let capacity = env.get_direct_buffer_capacity(&buffer)?;
        if capacity < length {
            return Err(bad("pixel buffer is smaller than the frame"));
        }
        let pointer = env.get_direct_buffer_address(&buffer)?;
        if pointer.is_null() || length > isize::MAX as usize {
            return Err(bad("invalid direct pixel buffer"));
        }
        // SAFETY: JNI keeps buffer alive for this call. It is direct, writable,
        // and has verified capacity. NativeReader owns it exclusively during the
        // synchronous copy; no pointer or mutable slice is retained after return.
        let pixels = unsafe { std::slice::from_raw_parts_mut(pointer, length) };
        frame.write_rgba(pixels)?;
        Ok(1)
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_xin_soymilk_readall_NativeReader_nativeClose(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
) {
    guard(&mut env, |_| {
        let reader = readers()
            .lock()
            .map_err(|_| bad("reader registry unavailable"))?
            .remove(&handle);
        drop(reader); // cancellation only; never join a layout thread on Android UI.
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_xin_soymilk_readall_NativeReader_nativeInput(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    mode: JString<'_>,
    value: JString<'_>,
) {
    guard(&mut env, |env| {
        let mode = text(env, &mode)?;
        let value = text(env, &value)?;
        with_reader(handle, |r| {
            Ok(r.command(Command::Input { mode, text: value })?)
        })
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_xin_soymilk_readall_NativeReader_nativeHostReply(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    kind: jint,
    value: JString<'_>,
) {
    guard(&mut env, |env| {
        let text: String = env.get_string(&value)?.into();
        if text.len() > 128 * 1024 {
            return Err(bad("host reply exceeds text limit"));
        }
        with_reader(handle, |r| {
            Ok(r.command(Command::HostReply {
                kind: number(kind)?,
                text,
            })?)
        })
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_xin_soymilk_readall_NativeReader_nativeEffects(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
) -> jobjectArray {
    guard(&mut env, |env| {
        let effects = with_reader(handle, |r| Ok(r.take_effects()))?;
        let fields: Vec<String> = effects
            .into_iter()
            .flat_map(|effect| match effect {
                Effect::Copy(text) => ["copy".into(), text],
                Effect::Paste => ["paste".into(), String::new()],
                Effect::OpenUrl(url) => ["url".into(), url],
            })
            .collect();
        strings(env, &fields)
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_commands_and_stale_handles_do_not_access_native_objects() {
        assert!(command(0, 0, 0).is_err());
        assert!(command(8, -1, 0).is_err());
        assert!(command(12, 1, -1).is_err());
        assert!(command(14, 2, 0).is_err());
        assert!(matches!(
            command(42, 30, 60).unwrap(),
            Command::Touch {
                kind: 2,
                x: 30,
                y: 60
            }
        ));
        assert!(matches!(
            command(22, 0, 0).unwrap(),
            Command::Ui(UiAction::Command(UiCommand::Settings))
        ));
        assert!(with_reader(-100, |r| Ok(r.snapshot())).is_err());
    }
    #[test]
    fn state_protocol_keeps_unicode_and_optional_frame_fields_separate() {
        let state = Snapshot {
            title: "中文\t\n😀".into(),
            notice: "a\"b".into(),
            ..Snapshot::default()
        };
        let values = state_fields(state);
        assert_eq!(values.len(), 19);
        assert_eq!(values[0], "2");
        assert_eq!(values[14], "expanded");
        assert_eq!(values[15], "slide");
        assert_eq!(values[8], "中文\t\n😀");
        assert_eq!(values[5], "0");
        assert_eq!(values[12], "a\"b");
    }
}
