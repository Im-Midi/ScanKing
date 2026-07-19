//! Android 原生能力（JNI 直调，无需第三方 SDK）：
//! - 系统分享面板（ACTION_SEND + FileProvider，微信/QQ 等自动出现）
//! - 保存图片到系统相册（MediaStore，Android 10+ 免存储权限）
#![cfg(target_os = "android")]

use jni::objects::{JClass, JObject, JValue};
use jni::JavaVM;
use std::sync::atomic::{AtomicUsize, Ordering};

// 复用 Tauri 自带的 FileProvider（同类名 provider 只能有一个实例在响应请求）
const AUTHORITY: &str = "com.ng.scanking.fileprovider";
const FLAG_GRANT_READ: i32 = 0x0000_0001;
const FLAG_NEW_TASK: i32 = 0x1000_0000;

// 由 MainActivity.nativeInit 填充的 JavaVM 与 Application Context（全局引用，进程级持有）
static VM_PTR: AtomicUsize = AtomicUsize::new(0);
static CONTEXT_PTR: AtomicUsize = AtomicUsize::new(0);
// FileProvider 是 APK 里的类，后台线程的 FindClass 用系统类加载器找不到它，
// 必须在主线程（类加载器正确）提前解析并缓存
static FILE_PROVIDER_CLASS: AtomicUsize = AtomicUsize::new(0);

/// MainActivity 启动时调用：Java_com_ng_scanking_MainActivity_nativeInit
#[no_mangle]
pub extern "system" fn Java_com_ng_scanking_MainActivity_nativeInit(
    mut env: jni::JNIEnv,
    _class: JClass,
    context: JObject,
) {
    if let (Ok(vm), Ok(global)) = (env.get_java_vm(), env.new_global_ref(&context)) {
        VM_PTR.store(vm.get_java_vm_pointer() as usize, Ordering::SeqCst);
        CONTEXT_PTR.store(global.as_obj().as_raw() as usize, Ordering::SeqCst);
        std::mem::forget(global); // 全局引用与进程同寿命
    }
    if let Ok(cls) = env.find_class("androidx/core/content/FileProvider") {
        if let Ok(g) = env.new_global_ref(&cls) {
            FILE_PROVIDER_CLASS.store(g.as_obj().as_raw() as usize, Ordering::SeqCst);
            std::mem::forget(g);
        }
    }
}

/// 取缓存的 FileProvider 类（后台线程可用）
fn file_provider_class() -> jni::errors::Result<JClass<'static>> {
    let p = FILE_PROVIDER_CLASS.load(Ordering::SeqCst);
    if p == 0 {
        return Err(jni::errors::Error::NullPtr(
            "FileProvider 类未缓存（nativeInit 未完成）",
        ));
    }
    Ok(unsafe { JClass::from_raw(p as jni::sys::jclass) })
}

fn runtime_handles() -> Result<(JavaVM, jni::sys::jobject), String> {
    let (vp, cp) = (VM_PTR.load(Ordering::SeqCst), CONTEXT_PTR.load(Ordering::SeqCst));
    if vp != 0 && cp != 0 {
        let vm = unsafe { JavaVM::from_raw(vp as *mut jni::sys::JavaVM) }.map_err(|e| e.to_string())?;
        return Ok((vm, cp as jni::sys::jobject));
    }
    // 兜底：部分环境 ndk-context 可用（内部会 panic，需捕获）
    let ctx = std::panic::catch_unwind(ndk_context::android_context)
        .map_err(|_| "原生环境未初始化（MainActivity.nativeInit 未生效，请确认用最新脚本重打包）".to_string())?;
    let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }.map_err(|e| e.to_string())?;
    Ok((vm, ctx.context() as jni::sys::jobject))
}

fn with_env<T>(
    f: impl FnOnce(&mut jni::JNIEnv, &JObject) -> jni::errors::Result<T>,
) -> Result<T, String> {
    let (vm, ctx_raw) = runtime_handles()?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let context = unsafe { JObject::from_raw(ctx_raw) };
    let result = f(&mut env, &context);
    match result {
        Ok(v) => Ok(v),
        Err(e) => match throwable_to_string(&mut env) {
            Some(d) => Err(format!("Java 异常: {}", d)),
            None => Err(format!("系统调用失败: {}", e)),
        },
    }
}

/// 把挂起的 Java 异常转成字符串并清除
fn throwable_to_string(env: &mut jni::JNIEnv) -> Option<String> {
    if !env.exception_check().unwrap_or(false) {
        return None;
    }
    let throwable = env.exception_occurred().ok()?;
    let _ = env.exception_clear();
    let val = env
        .call_method(&throwable, "toString", "()Ljava/lang/String;", &[])
        .ok()?;
    let obj = val.l().ok()?;
    let js = jni::objects::JString::from(obj);
    let text: String = env.get_string(&js).ok()?.into();
    Some(text)
}

/// 弹出系统分享面板分享一个文件（微信、QQ 等都会出现在面板里）
pub fn share_file(path: &str, mime: &str, title: &str) -> Result<(), String> {
    with_env(|env, context| {
        // Uri uri = FileProvider.getUriForFile(context, AUTHORITY, new File(path))
        let jpath = env.new_string(path)?;
        let file = env.new_object("java/io/File", "(Ljava/lang/String;)V", &[JValue::Object(&jpath)])?;
        let auth = env.new_string(AUTHORITY)?;
        let fp = file_provider_class()?;
        let uri = env
            .call_static_method(
                &fp,
                "getUriForFile",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/io/File;)Landroid/net/Uri;",
                &[JValue::Object(context), JValue::Object(&auth), JValue::Object(&file)],
            )?
            .l()?;

        // Intent intent = new Intent(ACTION_SEND).setType(mime).putExtra(EXTRA_STREAM, uri)
        let action = env.new_string("android.intent.action.SEND")?;
        let intent =
            env.new_object("android/content/Intent", "(Ljava/lang/String;)V", &[JValue::Object(&action)])?;
        let jmime = env.new_string(mime)?;
        env.call_method(
            &intent,
            "setType",
            "(Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&jmime)],
        )?;
        let extra = env.new_string("android.intent.extra.STREAM")?;
        env.call_method(
            &intent,
            "putExtra",
            "(Ljava/lang/String;Landroid/os/Parcelable;)Landroid/content/Intent;",
            &[JValue::Object(&extra), JValue::Object(&uri)],
        )?;
        // 关键：URI 藏在 extra 里时读取授权不会传递给目标 App（微信会报"获取资源失败"），
        // 必须同时挂 ClipData 才能让授权生效
        let label = env.new_string("file")?;
        let clip = env
            .call_static_method(
                "android/content/ClipData",
                "newRawUri",
                "(Ljava/lang/CharSequence;Landroid/net/Uri;)Landroid/content/ClipData;",
                &[JValue::Object(&label), JValue::Object(&uri)],
            )?
            .l()?;
        env.call_method(
            &intent,
            "setClipData",
            "(Landroid/content/ClipData;)V",
            &[JValue::Object(&clip)],
        )?;
        env.call_method(
            &intent,
            "addFlags",
            "(I)Landroid/content/Intent;",
            &[JValue::Int(FLAG_GRANT_READ)],
        )?;

        // startActivity(Intent.createChooser(intent, title).addFlags(NEW_TASK))
        let jtitle = env.new_string(title)?;
        let chooser = env
            .call_static_method(
                "android/content/Intent",
                "createChooser",
                "(Landroid/content/Intent;Ljava/lang/CharSequence;)Landroid/content/Intent;",
                &[JValue::Object(&intent), JValue::Object(&jtitle)],
            )?
            .l()?;
        env.call_method(
            &chooser,
            "addFlags",
            "(I)Landroid/content/Intent;",
            &[JValue::Int(FLAG_NEW_TASK | FLAG_GRANT_READ)],
        )?;
        env.call_method(
            context,
            "startActivity",
            "(Landroid/content/Intent;)V",
            &[JValue::Object(&chooser)],
        )?;
        Ok(())
    })
}

/// 用系统默认应用打开文件（ACTION_VIEW），替代不可靠的 opener 插件
pub fn open_file(path: &str, mime: &str) -> Result<(), String> {
    with_env(|env, context| {
        let jpath = env.new_string(path)?;
        let file = env.new_object("java/io/File", "(Ljava/lang/String;)V", &[JValue::Object(&jpath)])?;
        let auth = env.new_string(AUTHORITY)?;
        let fp = file_provider_class()?;
        let uri = env
            .call_static_method(
                &fp,
                "getUriForFile",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/io/File;)Landroid/net/Uri;",
                &[JValue::Object(context), JValue::Object(&auth), JValue::Object(&file)],
            )?
            .l()?;
        let action = env.new_string("android.intent.action.VIEW")?;
        let intent =
            env.new_object("android/content/Intent", "(Ljava/lang/String;)V", &[JValue::Object(&action)])?;
        let jmime = env.new_string(mime)?;
        env.call_method(
            &intent,
            "setDataAndType",
            "(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&uri), JValue::Object(&jmime)],
        )?;
        env.call_method(
            &intent,
            "addFlags",
            "(I)Landroid/content/Intent;",
            &[JValue::Int(FLAG_GRANT_READ | FLAG_NEW_TASK)],
        )?;
        env.call_method(
            context,
            "startActivity",
            "(Landroid/content/Intent;)V",
            &[JValue::Object(&intent)],
        )?;
        Ok(())
    })
}

/// 把图片字节写入系统相册（Pictures/扫描王），Android 10+ 无需任何权限
pub fn save_image_to_gallery(bytes: &[u8], display_name: &str) -> Result<(), String> {
    with_env(|env, context| {
        // ContentValues values = ...
        let values = env.new_object("android/content/ContentValues", "()V", &[])?;
        let put = |env: &mut jni::JNIEnv, key: &str, val: &str| -> jni::errors::Result<()> {
            let k = env.new_string(key)?;
            let v = env.new_string(val)?;
            env.call_method(
                &values,
                "put",
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(&k), JValue::Object(&v)],
            )?;
            Ok(())
        };
        put(env, "_display_name", display_name)?;
        put(env, "mime_type", "image/jpeg")?;
        put(env, "relative_path", "Pictures/ScanKing")?;

        // Uri uri = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values)
        let resolver = env
            .call_method(context, "getContentResolver", "()Landroid/content/ContentResolver;", &[])?
            .l()?;
        let collection = env
            .get_static_field(
                "android/provider/MediaStore$Images$Media",
                "EXTERNAL_CONTENT_URI",
                "Landroid/net/Uri;",
            )?
            .l()?;
        let uri = env
            .call_method(
                &resolver,
                "insert",
                "(Landroid/net/Uri;Landroid/content/ContentValues;)Landroid/net/Uri;",
                &[JValue::Object(&collection), JValue::Object(&values)],
            )?
            .l()?;
        if uri.is_null() {
            return Err(jni::errors::Error::NullPtr("MediaStore insert 返回空"));
        }

        // OutputStream os = resolver.openOutputStream(uri); os.write(bytes); os.close()
        let os = env
            .call_method(
                &resolver,
                "openOutputStream",
                "(Landroid/net/Uri;)Ljava/io/OutputStream;",
                &[JValue::Object(&uri)],
            )?
            .l()?;
        let arr = env.byte_array_from_slice(bytes)?;
        env.call_method(&os, "write", "([B)V", &[JValue::Object(&arr)])?;
        env.call_method(&os, "flush", "()V", &[])?;
        env.call_method(&os, "close", "()V", &[])?;
        Ok(())
    })
}
