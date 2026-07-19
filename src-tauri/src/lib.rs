//! Tauri 命令层：薄封装，所有重逻辑都在 scanking-core

#[cfg(target_os = "android")]
mod android;

use base64::Engine;
use scanking_core::store::{DocMeta, DocSummary, PageInfo, Store};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use tauri::Manager;

struct AppState {
    store: Arc<Store>,
    #[cfg(feature = "ocr")]
    ocr: Mutex<Option<Arc<scanking_core::ocr::OcrEngine>>>,
}

type CmdResult<T> = Result<T, String>;

fn err_str<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Serialize)]
struct DocDetail {
    meta: DocMeta,
    pages: Vec<PageInfo>,
}

fn decode_b64(data: &str) -> CmdResult<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| format!("base64 解码失败: {}", e))
}

// ---------------- 文档库 ----------------

#[tauri::command]
async fn lib_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<DocSummary>> {
    state.store.list_docs().map_err(err_str)
}

#[tauri::command]
async fn lib_search(state: tauri::State<'_, AppState>, query: String) -> CmdResult<Vec<DocSummary>> {
    state.store.search_docs(&query).map_err(err_str)
}

#[tauri::command]
async fn doc_create(state: tauri::State<'_, AppState>, title: String) -> CmdResult<DocMeta> {
    state.store.create_doc(&title).map_err(err_str)
}

#[tauri::command]
async fn doc_get(state: tauri::State<'_, AppState>, doc_id: String) -> CmdResult<DocDetail> {
    let meta = state.store.get_doc(&doc_id).map_err(err_str)?;
    let mut pages = Vec::with_capacity(meta.pages.len());
    for p in &meta.pages {
        pages.push(state.store.page_info(&doc_id, &p.id).map_err(err_str)?);
    }
    Ok(DocDetail { meta, pages })
}

#[tauri::command]
async fn doc_rename(state: tauri::State<'_, AppState>, doc_id: String, title: String) -> CmdResult<()> {
    state.store.rename_doc(&doc_id, &title).map_err(err_str)
}

#[tauri::command]
async fn doc_set_tags(state: tauri::State<'_, AppState>, doc_id: String, tags: Vec<String>) -> CmdResult<()> {
    state.store.set_tags(&doc_id, tags).map_err(err_str)
}

#[tauri::command]
async fn doc_delete(state: tauri::State<'_, AppState>, doc_id: String) -> CmdResult<()> {
    state.store.delete_doc(&doc_id).map_err(err_str)
}

// ---------------- 页面 ----------------

/// 导入一页（图片以 base64 传输）。耗时操作放到阻塞线程池。
#[tauri::command]
async fn page_import(
    state: tauri::State<'_, AppState>,
    doc_id: String,
    data: String,
) -> CmdResult<PageInfo> {
    let bytes = decode_b64(&data)?;
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || store.add_page(&doc_id, &bytes).map_err(err_str))
        .await
        .map_err(err_str)?
}

#[tauri::command]
async fn page_redetect(
    state: tauri::State<'_, AppState>,
    doc_id: String,
    page_id: String,
) -> CmdResult<[[f32; 2]; 4]> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || store.detect_page(&doc_id, &page_id).map_err(err_str))
        .await
        .map_err(err_str)?
}

#[tauri::command]
async fn page_set_geometry(
    state: tauri::State<'_, AppState>,
    doc_id: String,
    page_id: String,
    quad: Option<[[f32; 2]; 4]>,
    rotation: u32,
) -> CmdResult<PageInfo> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.set_geometry(&doc_id, &page_id, quad, rotation).map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

#[tauri::command]
async fn page_set_filter(
    state: tauri::State<'_, AppState>,
    doc_id: String,
    page_id: String,
    filter: String,
) -> CmdResult<PageInfo> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.set_filter(&doc_id, &page_id, &filter).map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

#[tauri::command]
async fn page_delete(state: tauri::State<'_, AppState>, doc_id: String, page_id: String) -> CmdResult<()> {
    state.store.delete_page(&doc_id, &page_id).map_err(err_str)
}

#[tauri::command]
async fn pages_reorder(
    state: tauri::State<'_, AppState>,
    doc_id: String,
    page_ids: Vec<String>,
) -> CmdResult<()> {
    state.store.reorder_pages(&doc_id, &page_ids).map_err(err_str)
}

#[tauri::command]
async fn idcard_compose(
    state: tauri::State<'_, AppState>,
    doc_id: String,
    page_a: String,
    page_b: String,
) -> CmdResult<PageInfo> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.idcard_compose(&doc_id, &page_a, &page_b).map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

// ---------------- 导出 ----------------

#[tauri::command]
async fn doc_export_pdf(
    state: tauri::State<'_, AppState>,
    doc_id: String,
    mode: String,
    quality: u8,
) -> CmdResult<String> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .export_pdf(&doc_id, &mode, quality)
            .map(|p| p.to_string_lossy().to_string())
            .map_err(err_str)
    })
    .await
    .map_err(err_str)?
}

// ---------------- 分享 / 相册 / 打开 ----------------

/// 校验路径必须在应用数据目录内，防止任意文件读取
fn ensure_in_store(state: &AppState, path: &str) -> Result<std::path::PathBuf, String> {
    let root = state.store.root().canonicalize().map_err(err_str)?;
    let p = std::path::Path::new(path).canonicalize().map_err(err_str)?;
    if !p.starts_with(&root) {
        return Err("路径不允许".into());
    }
    Ok(p)
}

fn mime_of(p: &std::path::Path) -> &'static str {
    if p.extension().map(|e| e == "pdf").unwrap_or(false) {
        "application/pdf"
    } else {
        "image/jpeg"
    }
}

/// 在阻塞线程池执行原生调用，并把 panic 转成可见错误（避免前端永远等不到回应）
#[cfg(target_os = "android")]
async fn run_native<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
            Ok(r) => r,
            Err(p) => {
                let msg = p
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| p.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "未知原生崩溃".into());
                Err(format!("原生调用异常: {}", msg))
            }
        }
    })
    .await
    .map_err(err_str)?
}

/// 调起系统分享面板（微信/QQ 等自动出现）。支持 PDF 与页面图片。
#[tauri::command]
async fn share_file(state: tauri::State<'_, AppState>, path: String) -> CmdResult<()> {
    let p = ensure_in_store(&state, &path)?;
    #[cfg(target_os = "android")]
    {
        let mime = mime_of(&p);
        let ps = p.to_string_lossy().to_string();
        return run_native(move || android::share_file(&ps, mime, "分享到")).await;
    }
    #[allow(unreachable_code)]
    {
        let _ = p;
        Err("分享功能在手机端可用；桌面端请用「打开」按钮".into())
    }
}

/// 用系统默认应用打开文件（安卓原生实现，不依赖 opener 插件）
#[tauri::command]
async fn open_file(state: tauri::State<'_, AppState>, path: String) -> CmdResult<()> {
    let p = ensure_in_store(&state, &path)?;
    #[cfg(target_os = "android")]
    {
        let mime = mime_of(&p);
        let ps = p.to_string_lossy().to_string();
        return run_native(move || android::open_file(&ps, mime)).await;
    }
    #[allow(unreachable_code)]
    {
        let _ = p;
        Err("桌面端请通过前端 opener 打开".into())
    }
}

/// 保存页面图片：安卓存入系统相册（Pictures/ScanKing），Windows 存入图片文件夹
#[tauri::command]
async fn save_page_gallery(
    state: tauri::State<'_, AppState>,
    doc_id: String,
    page_id: String,
) -> CmdResult<String> {
    let src = state.store.work_image_path(&doc_id, &page_id);
    let name = format!(
        "ScanKing_{}_{}.jpg",
        &doc_id[..6.min(doc_id.len())],
        &page_id[..6.min(page_id.len())]
    );
    #[cfg(target_os = "android")]
    {
        let bytes = std::fs::read(&src).map_err(err_str)?;
        let n = name.clone();
        run_native(move || android::save_image_to_gallery(&bytes, &n)).await?;
        return Ok("相册 · Pictures/ScanKing".into());
    }
    #[cfg(all(not(target_os = "android"), target_os = "windows"))]
    {
        let home = std::env::var("USERPROFILE").map_err(err_str)?;
        let dir = std::path::PathBuf::from(home).join("Pictures").join("ScanKing");
        std::fs::create_dir_all(&dir).map_err(err_str)?;
        let dest = dir.join(&name);
        std::fs::copy(&src, &dest).map_err(err_str)?;
        return Ok(dest.to_string_lossy().to_string());
    }
    #[allow(unreachable_code)]
    {
        let _ = src;
        Err("此平台暂不支持".into())
    }
}

// ---------------- OCR ----------------

#[tauri::command]
async fn ocr_ready(state: tauri::State<'_, AppState>) -> CmdResult<bool> {
    #[cfg(feature = "ocr")]
    {
        return Ok(scanking_core::ocr::OcrEngine::ready(&state.store.models_dir()));
    }
    #[allow(unreachable_code)]
    {
        let _ = &state;
        Ok(false)
    }
}

/// 安装 OCR 模型文件（首次启动时前端从内置资源分块上传）。
/// append=false 表示新文件（覆盖），append=true 表示追加分块。返回当前文件字节数。
#[tauri::command]
async fn install_model(
    state: tauri::State<'_, AppState>,
    name: String,
    data: String,
    append: bool,
) -> CmdResult<u64> {
    // 只允许固定文件名，防止路径注入
    let allowed = ["det.onnx", "rec.onnx", "cls.onnx", "keys.txt"];
    if !allowed.contains(&name.as_str()) {
        return Err("非法文件名".into());
    }
    let bytes = decode_b64(&data)?;
    let path = state.store.models_dir().join(&name);
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(&path)
        .map_err(err_str)?;
    f.write_all(&bytes).map_err(err_str)?;
    f.flush().map_err(err_str)?;
    drop(f);
    Ok(std::fs::metadata(&path).map_err(err_str)?.len())
}

#[cfg(feature = "ocr")]
fn get_ocr(state: &AppState) -> Result<Arc<scanking_core::ocr::OcrEngine>, String> {
    let mut guard = state.ocr.lock().unwrap();
    if let Some(e) = guard.as_ref() {
        return Ok(e.clone());
    }
    let dir = state.store.models_dir();
    if !scanking_core::ocr::OcrEngine::ready(&dir) {
        return Err("OCR 模型未安装".into());
    }
    let engine = match scanking_core::ocr::OcrEngine::load(&dir) {
        Ok(e) => e,
        Err(e) => {
            // 自愈：模型文件损坏时删掉，下次点击识别会自动重新安装
            scanking_core::ocr::OcrEngine::purge(&dir);
            return Err(format!("模型文件损坏已清理，请再点一次识别自动重装（{}）", e));
        }
    };
    let engine = Arc::new(engine);
    *guard = Some(engine.clone());
    Ok(engine)
}

#[tauri::command]
async fn page_ocr(
    state: tauri::State<'_, AppState>,
    doc_id: String,
    page_id: String,
) -> CmdResult<String> {
    #[cfg(feature = "ocr")]
    {
        // 已有缓存直接返回
        if let Some(t) = state.store.get_ocr_text(&doc_id, &page_id) {
            if !t.is_empty() {
                return Ok(t);
            }
        }
        let engine = get_ocr(&state)?;
        let store = state.store.clone();
        return tauri::async_runtime::spawn_blocking(move || {
            let img = store.load_work_image(&doc_id, &page_id).map_err(err_str)?;
            let text = engine.ocr_to_text(&img).map_err(err_str)?;
            store.save_ocr_text(&doc_id, &page_id, &text).map_err(err_str)?;
            Ok(text)
        })
        .await
        .map_err(err_str)?;
    }
    #[allow(unreachable_code)]
    {
        let _ = (&state, &doc_id, &page_id);
        Err("此版本未启用 OCR".into())
    }
}

#[tauri::command]
async fn doc_ocr_text(state: tauri::State<'_, AppState>, doc_id: String) -> CmdResult<String> {
    state.store.doc_ocr_text(&doc_id).map_err(err_str)
}

// ---------------- 应用入口 ----------------

static STORE_ROOT: OnceLock<PathBuf> = OnceLock::new();

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let root = data_dir.join("library");
            STORE_ROOT.set(root.clone()).ok();
            let store = Store::open(&root)?;
            app.manage(AppState {
                store: Arc::new(store),
                #[cfg(feature = "ocr")]
                ocr: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            lib_list,
            lib_search,
            doc_create,
            doc_get,
            doc_rename,
            doc_set_tags,
            doc_delete,
            page_import,
            page_redetect,
            page_set_geometry,
            page_set_filter,
            page_delete,
            pages_reorder,
            idcard_compose,
            doc_export_pdf,
            share_file,
            open_file,
            save_page_gallery,
            ocr_ready,
            install_model,
            page_ocr,
            doc_ocr_text
        ])
        .run(tauri::generate_context!())
        .expect("启动失败");
}
