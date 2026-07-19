/* 扫描王 ScanKing — 前端主逻辑（vanilla JS，无框架无构建） */
"use strict";

const { invoke, convertFileSrc } = window.__TAURI__.core;
const openerApi = (window.__TAURI__ && window.__TAURI__.opener) || null;

// ---------------- 小工具 ----------------
const $ = (id) => document.getElementById(id);
let toastTimer = null;
function toast(msg, ms = 2400) {
  const t = $("toast");
  t.textContent = msg;
  t.style.display = "block";
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.style.display = "none"), ms);
}
function loading(show, text = "处理中…") {
  $("loading").style.display = show ? "flex" : "none";
  $("loading-text").textContent = text;
}
async function call(cmd, args) {
  try {
    return await invoke(cmd, args);
  } catch (e) {
    toast(String(e));
    throw e;
  }
}
function esc(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}
function src(path, bust) {
  return convertFileSrc(path) + (bust ? "?v=" + bust : "");
}
function fmtTime(ms) {
  const d = new Date(ms);
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}
async function blobToB64(blob) {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(r.result.split(",")[1]);
    r.onerror = reject;
    r.readAsDataURL(blob);
  });
}
function bufToB64(buf) {
  const bytes = new Uint8Array(buf);
  let bin = "";
  const CH = 0x8000;
  for (let i = 0; i < bytes.length; i += CH) {
    bin += String.fromCharCode.apply(null, bytes.subarray(i, i + CH));
  }
  return btoa(bin);
}
async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    toast("已复制");
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    ta.remove();
    toast("已复制");
  }
}

// ---------------- 弹层 ----------------
function openSheet(html) {
  $("sheet").innerHTML = html;
  $("mask").classList.add("active");
}
function closeSheet() {
  $("mask").classList.remove("active");
}
$("mask").addEventListener("click", (e) => {
  if (e.target === $("mask")) closeSheet();
});
function menuSheet(title, items) {
  openSheet(
    `<h3>${esc(title)}</h3>` +
      items
        .map(
          (it, i) =>
            `<button class="sheet-item ${it.danger ? "danger" : ""}" data-i="${i}">${it.icon || ""}${esc(it.label)}</button>`
        )
        .join("")
  );
  $("sheet").querySelectorAll(".sheet-item").forEach((b) => {
    b.onclick = () => {
      closeSheet();
      items[+b.dataset.i].fn();
    };
  });
}
function promptSheet(title, value, placeholder, onOk) {
  openSheet(
    `<h3>${esc(title)}</h3>
     <input type="text" id="sheet-input" value="${esc(value)}" placeholder="${esc(placeholder)}" />
     <div class="row">
       <button class="btn" id="sheet-cancel">取消</button>
       <button class="btn primary" id="sheet-ok">确定</button>
     </div>`
  );
  const inp = $("sheet-input");
  inp.focus();
  $("sheet-cancel").onclick = closeSheet;
  $("sheet-ok").onclick = () => {
    closeSheet();
    onOk(inp.value);
  };
}
function confirmSheet(title, okLabel, onOk) {
  openSheet(
    `<h3>${esc(title)}</h3>
     <div class="row">
       <button class="btn" id="sheet-cancel">取消</button>
       <button class="btn danger" id="sheet-ok">${esc(okLabel)}</button>
     </div>`
  );
  $("sheet-cancel").onclick = closeSheet;
  $("sheet-ok").onclick = () => {
    closeSheet();
    onOk();
  };
}
function textSheet(title, text) {
  openSheet(
    `<h3>${esc(title)}</h3>
     <textarea readonly id="sheet-text">${esc(text)}</textarea>
     <div class="row">
       <button class="btn" id="sheet-close2">关闭</button>
       <button class="btn primary" id="sheet-copy">复制全部</button>
     </div>`
  );
  $("sheet-close2").onclick = closeSheet;
  $("sheet-copy").onclick = () => copyText($("sheet-text").value);
}

// ---------------- 视图切换 ----------------
function show(viewId) {
  document.querySelectorAll(".view").forEach((v) => v.classList.remove("active"));
  $(viewId).classList.add("active");
}

// ---------------- 主题 ----------------
function applyTheme(mode) {
  document.documentElement.dataset.theme = mode;
  try { localStorage.setItem("theme", mode); } catch {}
}
$("btn-theme").onclick = () => {
  applyTheme(document.documentElement.dataset.theme === "dark" ? "light" : "dark");
};
(function initTheme() {
  let saved = null;
  try { saved = localStorage.getItem("theme"); } catch {}
  if (saved) applyTheme(saved);
  else if (matchMedia("(prefers-color-scheme: dark)").matches) applyTheme("dark");
})();

// ---------------- 全局状态 ----------------
let currentDocId = null;
let currentDoc = null; // DocDetail {meta, pages}
let currentPageIdx = -1;
let selectMode = false;
let selected = [];
let bust = Date.now();

// ---------------- 文档库 ----------------
async function loadLibrary(query = "") {
  const docs = await call(query ? "lib_search" : "lib_list", query ? { query } : undefined);
  const grid = $("doc-grid");
  grid.innerHTML = "";
  $("lib-empty").style.display = docs.length ? "none" : "block";
  for (const d of docs) {
    const card = document.createElement("div");
    card.className = "doc-card";
    const cover = d.cover
      ? `<img class="doc-cover" src="${src(d.cover, bust)}" loading="lazy" />`
      : `<div class="doc-cover empty">无页面</div>`;
    card.innerHTML = `
      ${cover}
      <div class="doc-info">
        <div class="doc-title">${esc(d.title)}</div>
        <div class="doc-sub">${d.page_count} 页 · ${fmtTime(d.updated_ms)}</div>
        ${d.tags.length ? `<div class="doc-tags">${d.tags.map((t) => `<span class="tag">${esc(t)}</span>`).join("")}</div>` : ""}
      </div>`;
    attachPress(
      card,
      () => openDoc(d.id),
      () => docCardMenu(d)
    );
    grid.appendChild(card);
  }
}
function docCardMenu(d) {
  menuSheet(d.title, [
    { label: "重命名", fn: () => promptSheet("重命名", d.title, "文档名", async (v) => { await call("doc_rename", { docId: d.id, title: v }); loadLibrary($("search-input").value); }) },
    { label: "编辑标签（逗号分隔）", fn: () => promptSheet("标签", d.tags.join(","), "如: 发票,报销", async (v) => {
        const tags = v.split(/[,，]/).map((s) => s.trim()).filter(Boolean);
        await call("doc_set_tags", { docId: d.id, tags });
        loadLibrary($("search-input").value);
      }) },
    { label: "删除文档", danger: true, fn: () => confirmSheet(`删除「${d.title}」？所有页面将一并删除`, "删除", async () => { await call("doc_delete", { docId: d.id }); loadLibrary($("search-input").value); }) },
  ]);
}
let searchTimer = null;
$("search-input").addEventListener("input", (e) => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => loadLibrary(e.target.value), 250);
});

// 长按支持
function attachPress(el, onTap, onLong) {
  let timer = null, longed = false;
  el.addEventListener("pointerdown", () => {
    longed = false;
    timer = setTimeout(() => { longed = true; onLong && onLong(); }, 520);
  });
  const cancel = () => clearTimeout(timer);
  el.addEventListener("pointermove", cancel);
  el.addEventListener("pointerleave", cancel);
  el.addEventListener("pointerup", () => { cancel(); if (!longed) onTap && onTap(); });
  el.addEventListener("contextmenu", (e) => e.preventDefault());
}

// ---------------- 文档详情 ----------------
async function openDoc(docId) {
  currentDocId = docId;
  selectMode = false;
  selected = [];
  await refreshDoc();
  show("view-doc");
}
async function refreshDoc() {
  currentDoc = await call("doc_get", { docId: currentDocId });
  $("doc-title").value = currentDoc.meta.title;
  const grid = $("page-grid");
  grid.innerHTML = "";
  $("doc-empty").style.display = currentDoc.pages.length ? "none" : "block";
  currentDoc.pages.forEach((p, i) => {
    const cell = document.createElement("div");
    cell.className = "page-cell";
    const selIdx = selected.indexOf(p.meta.id);
    if (selIdx >= 0) cell.classList.add("selected");
    cell.innerHTML = `
      <img src="${src(p.thumb, bust)}" loading="lazy" />
      <span class="pnum">${i + 1}</span>
      ${selIdx >= 0 ? `<span class="sel-badge">${selIdx + 1}</span>` : p.meta.ocr_done ? `<span class="ocr-mark">已识别</span>` : ""}`;
    attachPress(
      cell,
      () => (selectMode ? toggleSelect(p.meta.id) : openPage(i)),
      () => pageMenu(i)
    );
    grid.appendChild(cell);
  });
}
$("btn-doc-back").onclick = () => { loadLibrary($("search-input").value); show("view-library"); };
$("doc-title").addEventListener("change", async (e) => {
  await call("doc_rename", { docId: currentDocId, title: e.target.value });
});
$("btn-doc-menu").onclick = () => {
  menuSheet("文档操作", [
    { label: "编辑标签（逗号分隔）", fn: () => promptSheet("标签", currentDoc.meta.tags.join(","), "如: 发票,报销", async (v) => {
        const tags = v.split(/[,，]/).map((s) => s.trim()).filter(Boolean);
        await call("doc_set_tags", { docId: currentDocId, tags });
        refreshDoc();
      }) },
    { label: "分享 PDF（微信等）", fn: async () => {
        if (!currentDoc.pages.length) return toast("文档没有页面");
        loading(true, "正在准备分享…");
        try {
          const path = await call("doc_export_pdf", { docId: currentDocId, mode: "fit", quality: 85 });
          loading(false);
          sharePdf(path);
        } catch { loading(false); }
      } },
    { label: "证件拼页（选两页合成 A4）", fn: startIdCard },
    { label: "查看全部识别文本", fn: async () => {
        const t = await call("doc_ocr_text", { docId: currentDocId });
        t.trim() ? textSheet("识别文本", t) : toast("还没有已识别的页面");
      } },
    { label: "删除文档", danger: true, fn: () => confirmSheet("删除整个文档？", "删除", async () => {
        await call("doc_delete", { docId: currentDocId });
        loadLibrary();
        show("view-library");
      }) },
  ]);
};
function startIdCard() {
  if (currentDoc.pages.length < 2) return toast("至少需要两页");
  selectMode = true;
  selected = [];
  toast("依次点选两页（正面、反面）");
  refreshDoc();
}
async function toggleSelect(pageId) {
  const i = selected.indexOf(pageId);
  if (i >= 0) selected.splice(i, 1);
  else selected.push(pageId);
  if (selected.length === 2) {
    const [a, b] = selected;
    selectMode = false;
    selected = [];
    loading(true, "正在合成证件页…");
    try {
      await call("idcard_compose", { docId: currentDocId, pageA: a, pageB: b });
      bust = Date.now();
      toast("已生成拼页，追加在文档末尾");
    } finally {
      loading(false);
      refreshDoc();
    }
  } else {
    refreshDoc();
  }
}
function pageMenu(idx) {
  const p = currentDoc.pages[idx];
  const ids = currentDoc.pages.map((x) => x.meta.id);
  menuSheet(`第 ${idx + 1} 页`, [
    { label: "编辑（裁剪/滤镜）", fn: () => openEditor(idx) },
    { label: "识别文字", fn: async () => { openPage(idx); runPageOcr(); } },
    { label: "保存图片到相册", fn: () => savePageToGallery(idx) },
    { label: "分享图片（微信等）", fn: () => sharePage(idx) },
    ...(idx > 0 ? [{ label: "前移", fn: async () => { [ids[idx - 1], ids[idx]] = [ids[idx], ids[idx - 1]]; await call("pages_reorder", { docId: currentDocId, pageIds: ids }); refreshDoc(); } }] : []),
    ...(idx < ids.length - 1 ? [{ label: "后移", fn: async () => { [ids[idx + 1], ids[idx]] = [ids[idx], ids[idx + 1]]; await call("pages_reorder", { docId: currentDocId, pageIds: ids }); refreshDoc(); } }] : []),
    { label: "删除此页", danger: true, fn: () => confirmSheet("删除此页？", "删除", async () => { await call("page_delete", { docId: currentDocId, pageId: p.meta.id }); refreshDoc(); }) },
  ]);
}

// ---------------- 导出 ----------------
$("btn-export").onclick = () => {
  if (!currentDoc.pages.length) return toast("文档没有页面");
  openSheet(
    `<h3>导出 PDF</h3>
     <div class="seg" id="mode-seg">
       <button data-m="fit" class="active">自适应大小</button>
       <button data-m="a4">A4 页面</button>
     </div>
     <div class="slider-row">图像质量 <input type="range" id="q-slider" min="50" max="95" value="85" /> <span id="q-val">85</span></div>
     <div class="row">
       <button class="btn" id="sheet-cancel">取消</button>
       <button class="btn primary" id="sheet-ok">导出</button>
     </div>`
  );
  let mode = "fit";
  $("mode-seg").querySelectorAll("button").forEach((b) => {
    b.onclick = () => {
      $("mode-seg").querySelectorAll("button").forEach((x) => x.classList.remove("active"));
      b.classList.add("active");
      mode = b.dataset.m;
    };
  });
  $("q-slider").oninput = (e) => ($("q-val").textContent = e.target.value);
  $("sheet-cancel").onclick = closeSheet;
  $("sheet-ok").onclick = async () => {
    const quality = +$("q-slider").value;
    closeSheet();
    loading(true, "正在生成 PDF…");
    try {
      const path = await call("doc_export_pdf", { docId: currentDocId, mode, quality });
      loading(false);
      menuSheet("导出成功", [
        { label: "打开 PDF", fn: () => openPdf(path) },
        { label: "分享（微信 / QQ 等）", fn: () => sharePdf(path) },
        { label: "查看保存位置", fn: () => toast(path, 6000) },
      ]);
    } catch {
      loading(false);
    }
  };
};

// ---------------- 相机 / 导入 ----------------
let camStream = null;
let importedInSession = 0;

$("btn-new-scan").onclick = async () => {
  const doc = await call("doc_create", { title: "" });
  currentDocId = doc.id;
  currentDoc = { meta: doc, pages: [] };
  openCapture();
};
$("btn-add-page").onclick = openCapture;

async function openCapture() {
  importedInSession = 0;
  $("queue-strip").innerHTML = "";
  updateQueueCount();
  show("view-capture");
  $("cam-fallback").style.display = "none";
  try {
    camStream = await navigator.mediaDevices.getUserMedia({
      video: { facingMode: "environment", width: { ideal: 2560 }, height: { ideal: 1440 } },
      audio: false,
    });
    $("cam-video").srcObject = camStream;
  } catch (e) {
    $("cam-fallback").style.display = "flex";
  }
}
function stopCam() {
  if (camStream) {
    camStream.getTracks().forEach((t) => t.stop());
    camStream = null;
  }
  $("cam-video").srcObject = null;
}
async function finishCapture() {
  stopCam();
  await refreshDoc();
  show("view-doc");
}
$("btn-cam-close").onclick = finishCapture;
$("btn-cam-done").onclick = finishCapture;

$("btn-shutter").onclick = async () => {
  const v = $("cam-video");
  if (!camStream || !v.videoWidth) return toast("相机未就绪，试试相册导入");
  const c = document.createElement("canvas");
  c.width = v.videoWidth;
  c.height = v.videoHeight;
  c.getContext("2d").drawImage(v, 0, 0);
  const blob = await new Promise((r) => c.toBlob(r, "image/jpeg", 0.93));
  await importBlob(blob);
};
$("btn-gallery").onclick = () => $("file-gallery").click();
$("btn-fallback-gallery").onclick = () => $("file-gallery").click();
$("btn-native-cam").onclick = () => $("file-camera").click();
$("file-gallery").addEventListener("change", async (e) => {
  for (const f of e.target.files) await importBlob(f);
  e.target.value = "";
});
$("file-camera").addEventListener("change", async (e) => {
  for (const f of e.target.files) await importBlob(f);
  e.target.value = "";
});

async function importBlob(blob) {
  loading(true, "正在检测边缘并矫正…");
  try {
    const data = await blobToB64(blob);
    const info = await call("page_import", { docId: currentDocId, data });
    importedInSession++;
    bust = Date.now();
    const img = document.createElement("img");
    img.src = src(info.thumb, bust);
    $("queue-strip").appendChild(img);
    if ($("queue-strip").children.length > 5) $("queue-strip").removeChild($("queue-strip").firstChild);
    updateQueueCount();
  } finally {
    loading(false);
  }
}
function updateQueueCount() {
  const el = $("queue-count");
  el.style.display = importedInSession ? "flex" : "none";
  el.textContent = importedInSession;
}

// ---------------- 分享 / 相册 ----------------
async function savePageToGallery(idx) {
  const p = currentDoc.pages[idx];
  loading(true, "正在保存到相册…");
  try {
    const where = await call("save_page_gallery", { docId: currentDocId, pageId: p.meta.id });
    toast("已保存：" + where, 3200);
  } finally {
    loading(false);
  }
}
async function sharePage(idx) {
  const p = currentDoc.pages[idx];
  try {
    await call("share_file", { path: p.work });
  } catch {}
}
async function sharePdf(path) {
  try {
    await call("share_file", { path });
  } catch {}
}
async function openPdf(path) {
  if (/android/i.test(navigator.userAgent)) {
    try { await call("open_file", { path }); } catch {}
  } else if (openerApi && openerApi.openPath) {
    try { await openerApi.openPath(path); } catch (e) { toast("无法打开：" + e); }
  } else {
    toast("已保存：" + path, 5000);
  }
}

// ---------------- 页面查看 ----------------
function openPage(idx) {
  currentPageIdx = idx;
  const p = currentDoc.pages[idx];
  $("page-title").textContent = `第 ${idx + 1} 页`;
  $("page-img").src = src(p.work, bust);
  show("view-page");
}
$("btn-page-back").onclick = () => { refreshDoc(); show("view-doc"); };
$("btn-page-edit").onclick = () => openEditor(currentPageIdx);
$("btn-page-save").onclick = () => savePageToGallery(currentPageIdx);
$("btn-page-share").onclick = () => sharePage(currentPageIdx);
$("btn-page-menu").onclick = () => {
  menuSheet("页面操作", [
    { label: "快速旋转 90°", fn: async () => {
        const p = currentDoc.pages[currentPageIdx];
        loading(true, "旋转中…");
        try {
          await call("page_set_geometry", { docId: currentDocId, pageId: p.meta.id, quad: p.meta.quad, rotation: (p.meta.rotation + 1) % 4 });
          bust = Date.now();
          currentDoc = await call("doc_get", { docId: currentDocId });
          openPage(currentPageIdx);
        } finally { loading(false); }
      } },
    { label: "查看本页识别文本", fn: async () => {
        const p = currentDoc.pages[currentPageIdx];
        if (!p.meta.ocr_done) return toast("本页还未识别");
        const t = await call("page_ocr", { docId: currentDocId, pageId: p.meta.id });
        textSheet("识别文本", t);
      } },
    { label: "删除此页", danger: true, fn: () => confirmSheet("删除此页？", "删除", async () => {
        const p = currentDoc.pages[currentPageIdx];
        await call("page_delete", { docId: currentDocId, pageId: p.meta.id });
        await refreshDoc();
        show("view-doc");
      }) },
  ]);
};

// ---------------- OCR ----------------
let ocrInstalling = false;
async function ensureOcr() {
  if (await call("ocr_ready")) return true;
  if (ocrInstalling) { toast("OCR 初始化中，请稍候"); return false; }
  ocrInstalling = true;
  try {
    const files = ["det.onnx", "rec.onnx", "cls.onnx", "keys.txt"];
    const CHUNK = 2 * 1024 * 1024; // 2MB 分块，避免安卓 IPC 大消息截断
    for (let i = 0; i < files.length; i++) {
      const name = files[i];
      const resp = await fetch("models/" + name);
      if (!resp.ok) {
        if (name === "cls.onnx") continue; // cls 可选
        loading(false);
        toast("未找到内置模型，请先运行 scripts/fetch_models 再打包", 4200);
        return false;
      }
      const buf = await resp.arrayBuffer();
      const totalMB = (buf.byteLength / 1048576).toFixed(1);
      let written = 0;
      for (let off = 0; off < buf.byteLength; off += CHUNK) {
        const doneMB = (Math.min(off + CHUNK, buf.byteLength) / 1048576).toFixed(1);
        loading(true, `初始化离线 OCR：${name} ${doneMB}/${totalMB} MB`);
        written = await call("install_model", {
          name,
          data: bufToB64(buf.slice(off, off + CHUNK)),
          append: off > 0,
        });
      }
      if (written !== buf.byteLength) {
        loading(false);
        toast(`模型 ${name} 写入不完整 (${written}/${buf.byteLength})`, 4200);
        return false;
      }
    }
    loading(false);
    return await call("ocr_ready");
  } catch (e) {
    loading(false);
    toast("OCR 初始化失败: " + e);
    return false;
  } finally {
    ocrInstalling = false;
  }
}
async function runPageOcr() {
  const p = currentDoc.pages[currentPageIdx];
  if (!(await ensureOcr())) return;
  loading(true, "正在识别文字…");
  try {
    const text = await call("page_ocr", { docId: currentDocId, pageId: p.meta.id });
    loading(false);
    text.trim() ? textSheet("识别结果", text) : toast("没有识别到文字");
    currentDoc = await call("doc_get", { docId: currentDocId });
  } catch {
    loading(false);
  }
}
$("btn-page-ocr").onclick = runPageOcr;
$("btn-doc-ocr").onclick = async () => {
  if (!currentDoc.pages.length) return toast("文档没有页面");
  if (!(await ensureOcr())) return;
  try {
    for (let i = 0; i < currentDoc.pages.length; i++) {
      const p = currentDoc.pages[i];
      loading(true, `识别中 ${i + 1}/${currentDoc.pages.length} 页…`);
      await call("page_ocr", { docId: currentDocId, pageId: p.meta.id });
    }
    loading(false);
    const t = await call("doc_ocr_text", { docId: currentDocId });
    t.trim() ? textSheet("全文识别结果", t) : toast("没有识别到文字");
    refreshDoc();
  } catch {
    loading(false);
  }
};

// ---------------- 裁剪 / 滤镜编辑器 ----------------
const canvas = $("editor-canvas");
const ctx = canvas.getContext("2d");
const editor = {
  page: null,        // PageInfo
  img: null,         // preview 图（裁剪阶段）/ work 图（滤镜阶段）
  stage: "crop",     // crop | filter
  quad: null,        // 原图坐标
  rotation: 0,
  scale: 1,          // 原图 -> 画布
  ox: 0, oy: 0,      // 画布内偏移
  dragging: -1,
  pendingFilter: "enhance",
};

async function openEditor(idx) {
  currentPageIdx = idx;
  const p = currentDoc.pages[idx];
  editor.page = p;
  editor.stage = "crop";
  editor.quad = (p.meta.quad || p.quad).map((q) => [q[0], q[1]]);
  editor.rotation = p.meta.rotation;
  editor.pendingFilter = p.meta.filter;
  $("editor-title").textContent = "调整边框";
  $("crop-bar").style.display = "flex";
  $("filter-bar").style.display = "none";
  show("view-editor");
  await loadEditorImage(src(p.preview, bust), 1 / p.preview_scale);
  drawEditor();
}
function loadEditorImage(url, toOrig) {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      editor.img = img;
      editor.toOrig = toOrig; // 预览像素 -> 原图像素倍率
      fitCanvas();
      resolve();
    };
    img.onerror = () => { toast("图片加载失败"); resolve(); };
    img.src = url;
  });
}
function fitCanvas() {
  const wrap = $("editor-wrap");
  const dpr = window.devicePixelRatio || 1;
  canvas.width = wrap.clientWidth * dpr;
  canvas.height = wrap.clientHeight * dpr;
  const img = editor.img;
  if (!img) return;
  const pad = 26 * dpr;
  const s = Math.min((canvas.width - pad * 2) / img.width, (canvas.height - pad * 2) / img.height);
  editor.scale = s;
  editor.ox = (canvas.width - img.width * s) / 2;
  editor.oy = (canvas.height - img.height * s) / 2;
}
window.addEventListener("resize", () => { if ($("view-editor").classList.contains("active")) { fitCanvas(); drawEditor(); } });

// 原图坐标 -> 画布坐标（quad 存原图坐标；预览图 = 原图 * preview_scale）
function o2c(pt) {
  const ps = 1 / editor.toOrig;
  return [editor.ox + pt[0] * ps * editor.scale, editor.oy + pt[1] * ps * editor.scale];
}
function c2o(x, y) {
  const ps = 1 / editor.toOrig;
  return [(x - editor.ox) / (ps * editor.scale), (y - editor.oy) / (ps * editor.scale)];
}
function drawEditor() {
  if (!editor.img) return;
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(editor.img, editor.ox, editor.oy, editor.img.width * editor.scale, editor.img.height * editor.scale);
  if (editor.stage !== "crop") return;

  const pts = editor.quad.map(o2c);
  const dpr = window.devicePixelRatio || 1;
  // 阴影遮罩
  ctx.save();
  ctx.fillStyle = "rgba(0,0,0,0.45)";
  ctx.beginPath();
  ctx.rect(0, 0, canvas.width, canvas.height);
  ctx.moveTo(pts[0][0], pts[0][1]);
  for (let i = 3; i >= 0; i--) ctx.lineTo(pts[i][0], pts[i][1]);
  ctx.closePath();
  ctx.fill("evenodd");
  ctx.restore();
  // 边框
  ctx.strokeStyle = "#3b82f6";
  ctx.lineWidth = 2.5 * dpr;
  ctx.beginPath();
  ctx.moveTo(pts[0][0], pts[0][1]);
  for (let i = 1; i < 4; i++) ctx.lineTo(pts[i][0], pts[i][1]);
  ctx.closePath();
  ctx.stroke();
  // 角点
  for (const [x, y] of pts) {
    ctx.beginPath();
    ctx.arc(x, y, 11 * dpr, 0, Math.PI * 2);
    ctx.fillStyle = "rgba(59,130,246,0.28)";
    ctx.fill();
    ctx.beginPath();
    ctx.arc(x, y, 5.5 * dpr, 0, Math.PI * 2);
    ctx.fillStyle = "#fff";
    ctx.fill();
    ctx.strokeStyle = "#3b82f6";
    ctx.lineWidth = 2 * dpr;
    ctx.stroke();
  }
  // 放大镜
  if (editor.dragging >= 0) {
    const [x, y] = pts[editor.dragging];
    const R = 52 * dpr, Z = 2.4;
    const mx = x < canvas.width / 2 ? canvas.width - R - 14 * dpr : R + 14 * dpr;
    const my = R + 14 * dpr;
    ctx.save();
    ctx.beginPath();
    ctx.arc(mx, my, R, 0, Math.PI * 2);
    ctx.clip();
    ctx.fillStyle = "#000";
    ctx.fillRect(mx - R, my - R, R * 2, R * 2);
    ctx.translate(mx, my);
    ctx.scale(Z, Z);
    ctx.translate(-x, -y);
    ctx.drawImage(editor.img, editor.ox, editor.oy, editor.img.width * editor.scale, editor.img.height * editor.scale);
    ctx.restore();
    // 放大镜十字
    ctx.strokeStyle = "#3b82f6";
    ctx.lineWidth = 1.5 * dpr;
    ctx.beginPath();
    ctx.moveTo(mx - 12 * dpr, my); ctx.lineTo(mx + 12 * dpr, my);
    ctx.moveTo(mx, my - 12 * dpr); ctx.lineTo(mx, my + 12 * dpr);
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(mx, my, R, 0, Math.PI * 2);
    ctx.stroke();
  }
}
function evPos(e) {
  const r = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  return [(e.clientX - r.left) * dpr, (e.clientY - r.top) * dpr];
}
canvas.addEventListener("pointerdown", (e) => {
  if (editor.stage !== "crop") return;
  const [x, y] = evPos(e);
  const dpr = window.devicePixelRatio || 1;
  const pts = editor.quad.map(o2c);
  let best = -1, bd = 40 * dpr;
  pts.forEach(([px, py], i) => {
    const d = Math.hypot(px - x, py - y);
    if (d < bd) { bd = d; best = i; }
  });
  editor.dragging = best;
  if (best >= 0) canvas.setPointerCapture(e.pointerId);
  drawEditor();
});
canvas.addEventListener("pointermove", (e) => {
  if (editor.dragging < 0 || editor.stage !== "crop") return;
  const [x, y] = evPos(e);
  const o = c2o(x, y);
  const p = editor.page;
  o[0] = Math.max(0, Math.min(p.meta.width - 1, o[0]));
  o[1] = Math.max(0, Math.min(p.meta.height - 1, o[1]));
  editor.quad[editor.dragging] = o;
  drawEditor();
});
canvas.addEventListener("pointerup", () => { editor.dragging = -1; drawEditor(); });

function quadIsConvex(q) {
  let sign = 0;
  for (let i = 0; i < 4; i++) {
    const a = q[i], b = q[(i + 1) % 4], c = q[(i + 2) % 4];
    const cr = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
    const s = cr > 0 ? 1 : -1;
    if (sign === 0) sign = s;
    else if (s !== sign) return false;
  }
  return true;
}

$("btn-editor-cancel").onclick = () => { refreshDoc(); show("view-doc"); };
$("btn-editor-rotate").onclick = async () => {
  editor.rotation = (editor.rotation + 1) % 4;
  toast(`将旋转 ${editor.rotation * 90}°（应用后生效）`);
};
$("btn-auto-detect").onclick = async () => {
  const p = editor.page;
  loading(true, "重新检测…");
  try {
    editor.quad = await call("page_redetect", { docId: currentDocId, pageId: p.meta.id });
    drawEditor();
  } finally { loading(false); }
};
$("btn-full-page").onclick = () => {
  const p = editor.page;
  editor.quad = [[0, 0], [p.meta.width - 1, 0], [p.meta.width - 1, p.meta.height - 1], [0, p.meta.height - 1]];
  drawEditor();
};
$("btn-crop-next").onclick = async () => {
  if (!quadIsConvex(editor.quad)) return toast("边框形状无效，四角不能交叉");
  const p = editor.page;
  loading(true, "矫正处理中…");
  try {
    const info = await call("page_set_geometry", {
      docId: currentDocId,
      pageId: p.meta.id,
      quad: editor.quad.map((q) => [q[0], q[1]]),
      rotation: editor.rotation,
    });
    bust = Date.now();
    editor.page = info;
    editor.stage = "filter";
    $("editor-title").textContent = "选择滤镜";
    $("crop-bar").style.display = "none";
    $("filter-bar").style.display = "flex";
    document.querySelectorAll(".filter-chip").forEach((c) => c.classList.toggle("active", c.dataset.filter === info.meta.filter));
    await loadEditorImage(src(info.work, bust), 1);
    drawEditor();
  } finally { loading(false); }
};
document.querySelectorAll(".filter-chip").forEach((chip) => {
  chip.onclick = async () => {
    const p = editor.page;
    loading(true, "应用滤镜…");
    try {
      const info = await call("page_set_filter", { docId: currentDocId, pageId: p.meta.id, filter: chip.dataset.filter });
      bust = Date.now();
      editor.page = info;
      document.querySelectorAll(".filter-chip").forEach((c) => c.classList.toggle("active", c === chip));
      await loadEditorImage(src(info.work, bust), 1);
      drawEditor();
    } finally { loading(false); }
  };
});
$("btn-editor-done").onclick = async () => {
  await refreshDoc();
  show("view-doc");
};

// ---------------- 启动 ----------------
loadLibrary();
