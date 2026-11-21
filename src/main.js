import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import "./style.css";

// Modellfamilien: Art (Bild/Video), benötigtes Zubehör und Startwerte.
// refs = kann Referenzbilder (Vorlage, Gesichter) verarbeiten.
const FAMILIES = {
  sd: { label: "SD 1.5 / SDXL (Checkpoint)", kind: "image", refs: false, parts: ["vae"], required: [], steps: 25, cfg: 7, size: 512, sampler: "euler_a", negative: true, offload: false },
  flux1: { label: "FLUX.1", kind: "image", refs: false, parts: ["vae", "clip_l", "t5xxl"], required: ["vae", "clip_l", "t5xxl"], steps: 4, cfg: 1, size: 768, sampler: "euler", negative: false, offload: true },
  flux3: { label: "FLUX.3 Klein", kind: "image", refs: true, parts: ["vae", "llm"], required: ["vae", "llm"], steps: 16, cfg: 1, size: 768, sampler: "euler", negative: false, offload: true },
  other: { label: "Anderes Bildmodell (eigene Komponenten)", kind: "image", refs: true, parts: ["vae", "clip_l", "clip_g", "t5xxl", "llm"], required: [], steps: 20, cfg: 4, size: 768, sampler: "euler", negative: true, offload: true },
  wan22: { label: "Wan 2.2 TI2V 5B (Video)", kind: "video", parts: ["vae", "t5xxl"], required: ["vae", "t5xxl"], steps: 20, cfg: 6, flow: 3, fps: 24, sampler: "euler", negative: true, offload: true },
  wan: { label: "Anderes Videomodell (z. B. Wan 2.1, Wan 2.2 14B)", kind: "video", parts: ["vae", "t5xxl", "clip_vision", "high_noise"], required: ["vae", "t5xxl"], steps: 20, cfg: 6, flow: 3, fps: 16, sampler: "euler", negative: true, offload: true },
};
const ROLES = {
  checkpoint: "Checkpoint (komplettes Modell)",
  diffusion: "Diffusionsmodell (z. B. FLUX, Wan)",
  high_noise: "High-Noise-Modell (Wan 2.2 14B)",
  vae: "VAE",
  clip_l: "Text-Encoder CLIP-L",
  clip_g: "Text-Encoder CLIP-G",
  t5xxl: "Text-Encoder T5-XXL / UMT5",
  llm: "Text-Encoder LLM (z. B. Qwen)",
  clip_vision: "CLIP Vision (Wan 2.1 I2V)",
};
// Ein-Klick-Pakete: kleinste frei zugängliche Varianten.
const PRESETS = {
  flux3: {
    label: "Bild: FLUX.3 Klein · ca. 6,4 GB",
    files: [
      { source: "https://huggingface.co/leejet/flux-3-image-klein-GGUF/blob/main/flux3-image-klein-Q4_K.gguf", role: "diffusion", family: "flux3" },
      { source: "https://huggingface.co/Qwen/Qwen3-VL-4B-Instruct-GGUF/blob/main/Qwen3VL-4B-Instruct-Q4_K_M.gguf", role: "llm", family: "flux3" },
      { source: "https://huggingface.co/Comfy-Org/Flux-2-Dev/blob/main/split_files/vae/flux2-vae.safetensors", role: "vae", family: "flux3" },
    ],
  },
  wan22: {
    label: "Video: Wan 2.2 5B · ca. 8 GB",
    files: [
      { source: "https://huggingface.co/QuantStack/Wan2.2-TI2V-5B-GGUF/blob/main/Wan2.2-TI2V-5B-Q4_K_S.gguf", role: "diffusion", family: "wan22" },
      { source: "https://huggingface.co/city96/umt5-xxl-encoder-gguf/blob/main/umt5-xxl-encoder-Q4_K_S.gguf", role: "t5xxl", family: "wan22" },
      { source: "https://huggingface.co/QuantStack/Wan2.2-TI2V-5B-GGUF/blob/main/VAE/Wan2.2_VAE.safetensors", role: "vae", family: "wan22" },
    ],
  },
};
// Übliche Negativ-Beschreibung für Wan, wenn das Feld leer bleibt.
const WAN_NEGATIVE = "色调艳丽，过曝，静态，细节模糊不清，字幕，风格，作品，画作，画面，静止，整体发灰，最差质量，低质量，JPEG压缩残留，丑陋的，残缺的，多余的手指，画得不好的手部，画得不好的脸部，畸形的，毁容的，形态畸形的肢体，手指融合，静止不动的画面，杂乱的背景，三条腿，背景人很多，倒着走";
const IMAGE_SIZES = [512, 640, 768, 896, 1024];
const VIDEO_SIZES = { "832x480": "832 × 480 (quer)", "480x832": "480 × 832 (hoch)", "640x640": "640 × 640 (quadratisch)", "1280x704": "1280 × 704 (HD, langsam)" };
const VIDEO_LENGTHS = { 17: "kurz (17 Bilder)", 33: "mittel (33 Bilder)", 49: "lang (49 Bilder)", 81: "sehr lang (81 Bilder)" };

const $ = (id) => document.getElementById(id);
const esc = (v) => String(v).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const options = (map, selected) => Object.entries(map).map(([k, v]) => `<option value="${k}"${String(k) === String(selected) ? " selected" : ""}>${esc(v.label ?? v)}</option>`).join("");
const family = (entry) => FAMILIES[entry?.family] || FAMILIES.other;

$("app").innerHTML = `<main>
<header><h1>Mosaic</h1><label class="device">Gerät<select id="device"><option value="arc">Intel Core Ultra · Arc (Windows)</option><option value="metal">Apple Silicon · Metal (macOS)</option></select></label></header>

<section class="card">
 <div class="tabs" role="tablist">
  <button type="button" role="tab" class="tab" data-mode="image" aria-selected="true">Bild erstellen</button>
  <button type="button" role="tab" class="tab" data-mode="video" aria-selected="false">Video erstellen</button>
 </div>
 <form id="studio">
  <label>Modell<select id="model"></select></label>
  <div id="parts" class="row"></div>

  <div class="block">
   <div class="block-head"><span id="template-title">Vorlage (optional)</span><span class="hint-inline" id="template-hint"></span></div>
   <div class="template" id="template-zone">
    <div id="template-empty" class="template-empty">Bild hierher ziehen oder <label class="link">hochladen<input type="file" id="template-file" accept="image/png,image/jpeg,image/webp" hidden></label></div>
    <div id="template-filled" hidden><img id="template-img" alt="Vorlage"><button type="button" class="ghost" id="template-clear">Entfernen</button></div>
   </div>
   <div class="row" id="template-options" hidden>
    <label>Verwendung<select id="template-mode"><option value="ref">Als Referenz (Inhalt/Stil übernehmen)</option><option value="init">Als Ausgangsbild (abwandeln)</option></select></label>
    <label id="strength-wrap">Stärke der Veränderung <span id="strength-value"></span><input type="range" id="strength" min="0.1" max="1" step="0.05" value="0.6"></label>
   </div>
  </div>

  <div class="block">
   <div class="block-head"><span>Gesichter (optional)</span><span class="hint-inline" id="faces-hint"></span></div>
   <div id="face-picker" class="face-picker"></div>
   <label id="keyframe-wrap" hidden>Startbild mit Gesichtern erzeugen mit<select id="keyframe-model"></select></label>
  </div>

  <label>Beschreibung<textarea required maxlength="2000" id="prompt" rows="3" placeholder="Ein gelber Stadtbus auf einer regennassen Berliner Straße …"></textarea></label>
  <label id="negative-wrap">Negativer Prompt (optional)<input id="negative" maxlength="2000" placeholder="unscharf, verzerrt, Text"></label>
  <details><summary>Einstellungen</summary><div class="row">
   <label class="image-only">Breite<select id="width">${IMAGE_SIZES.map((s) => `<option>${s}</option>`).join("")}</select></label>
   <label class="image-only">Höhe<select id="height">${IMAGE_SIZES.map((s) => `<option>${s}</option>`).join("")}</select></label>
   <label class="video-only">Format<select id="video-size">${options(VIDEO_SIZES, "832x480")}</select></label>
   <label class="video-only">Länge<select id="frames">${options(VIDEO_LENGTHS, 33)}</select></label>
   <label class="video-only">Bilder pro Sekunde<input id="fps" type="number" min="1" max="60"></label>
   <label>Schritte<input id="steps" type="number" min="1" max="150"></label>
   <label>CFG<input id="cfg" type="number" min="0" max="30" step="0.5"></label>
   <label>Sampler<select id="sampler"><option>euler</option><option>euler_a</option><option>dpm++2m</option><option>dpm++2s_a</option><option>lcm</option></select></label>
   <label>Seed<input id="seed" placeholder="zufällig"></label>
   <label class="check"><input type="checkbox" id="offload"> Speicher sparen</label>
  </div></details>
  <button id="create">Bild erstellen</button>
 </form>
 <figure hidden id="result"><img id="picture" alt="Ergebnis" hidden><video id="movie" controls loop playsinline hidden></video><figcaption id="path"></figcaption></figure>
</section>

<section class="card">
 <h2>Gesichter</h2>
 <p class="hint">Lege Personen an und lade eigene Fotos hoch (am besten 1–3 gut beleuchtete Porträts). Die Fotos bleiben nur auf diesem Gerät und werden beim Erstellen als Referenz verwendet.</p>
 <div class="inline-form"><input id="face-name" maxlength="60" placeholder="Name der Person, z. B. Justin"><button type="button" id="face-add" class="secondary">Person anlegen</button></div>
 <div id="face-list"></div>
</section>

<section class="card" id="drop">
 <h2>Modelle</h2>
 <p class="hint">Lade ein Paket mit einem Klick oder ziehe eigene Modelldateien (.safetensors, .gguf) in dieses Fenster.</p>
 <div class="presets">${Object.entries(PRESETS).map(([k, p]) => `<button type="button" class="secondary" data-preset="${k}">${esc(p.label)}</button>`).join("")}</div>
 <h3>Eigenes Modell hinzufügen</h3>
 <label>Lokaler Dateipfad oder Hugging-Face-Link<input id="source" spellcheck="false" placeholder="/Users/du/Modelle/mein-modell.safetensors  oder  https://huggingface.co/…/blob/main/datei.gguf"></label>
 <div class="row">
  <label>Art der Datei<select id="role">${options(ROLES, "checkpoint")}</select></label>
  <label>Modellfamilie<select id="family">${options(FAMILIES, "sd")}</select></label>
 </div>
 <button type="button" id="add">Hinzufügen</button>
 <h3>Bibliothek</h3>
 <div id="library"></div>
</section>
<p role="status" id="status"></p>
</main>`;

let busy = false, entries = [], faces = [], mode = "image";
const templates = { image: null, video: null }; // Pfad der Vorlage je Modus
const selectedFaces = { image: new Set(), video: new Set() };
const blobs = new Map(); // Pfad → Blob-URL
const status = (text) => ($("status").textContent = text);

function controls(value) {
  busy = value;
  for (const el of document.querySelectorAll("button, input, select, textarea")) el.disabled = value;
  if (!value) $("create").disabled = !$("model").value;
}
async function work(message, fn) {
  if (busy) return;
  controls(true);
  status(message);
  try { await fn(); } catch (e) { status(String(e)); } finally { controls(false); }
}
async function mediaUrl(path) {
  if (blobs.has(path)) return blobs.get(path);
  const data = await invoke("media", { path });
  const type = path.endsWith(".webm") ? "video/webm" : path.endsWith(".png") ? "image/png" : path.endsWith(".webp") ? "image/webp" : "image/jpeg";
  const url = URL.createObjectURL(new Blob([data], { type }));
  blobs.set(path, url);
  return url;
}
const extOf = (name) => (name.split(".").pop() || "").toLowerCase().replace("jpeg", "jpg");
async function uploadFile(file, kind, face = "") {
  const ext = extOf(file.name);
  if (!["png", "jpg", "webp"].includes(ext)) throw new Error("Bitte ein PNG-, JPG- oder WebP-Bild wählen.");
  const bytes = new Uint8Array(await file.arrayBuffer());
  return invoke("upload", bytes, { headers: { "x-kind": kind, "x-ext": ext, "x-face": face } });
}

// ---------- Modus (Bild / Video) ----------
for (const tab of document.querySelectorAll(".tab")) {
  tab.onclick = () => { if (!busy) setMode(tab.dataset.mode); };
}
function setMode(next) {
  mode = next;
  for (const tab of document.querySelectorAll(".tab")) tab.setAttribute("aria-selected", String(tab.dataset.mode === mode));
  document.body.dataset.mode = mode;
  $("create").textContent = mode === "video" ? "Video erstellen" : "Bild erstellen";
  $("template-title").textContent = mode === "video" ? "Startbild (optional)" : "Vorlage (optional)";
  $("prompt").placeholder = mode === "video" ? "Die Kamera fährt langsam an einem gelben Bus vorbei, Regen glitzert im Licht …" : "Ein gelber Stadtbus auf einer regennassen Berliner Straße …";
  renderGenerator();
  renderTemplate();
  renderFacePicker();
}

// ---------- Vorlage ----------
$("template-file").onchange = (ev) => {
  const file = ev.target.files[0];
  ev.target.value = "";
  if (file) work("Vorlage wird gespeichert …", async () => { templates[mode] = await uploadFile(file, "template"); await renderTemplate(); status("Vorlage bereit."); });
};
$("template-clear").onclick = () => { templates[mode] = null; renderTemplate(); };
$("strength").oninput = () => ($("strength-value").textContent = Math.round($("strength").value * 100) + " %");
let templateChoice = "ref"; // Wahl des Nutzers für Bildmodelle mit Referenz-Unterstützung
$("template-mode").onchange = () => { templateChoice = $("template-mode").value; renderTemplate(); };
async function renderTemplate() {
  const path = templates[mode];
  $("template-empty").hidden = !!path;
  $("template-filled").hidden = !path;
  const fam = currentFamily();
  $("template-options").hidden = !path || mode === "video";
  if (mode === "image") {
    $("template-mode").querySelector('[value="ref"]').disabled = !fam?.refs;
    $("template-mode").value = fam?.refs ? templateChoice : "init";
  }
  $("strength-wrap").hidden = $("template-mode").value !== "init";
  $("strength").oninput();
  $("template-hint").textContent = mode === "video"
    ? "Das Video beginnt mit diesem Bild."
    : fam?.refs ? "Wird als Referenz oder Ausgangsbild genutzt." : "Dieses Modell nutzt die Vorlage als Ausgangsbild.";
  if (path) $("template-img").src = await mediaUrl(path);
}

// ---------- Gesichter-Datenbank ----------
async function refreshFaces() {
  faces = await invoke("faces");
  for (const set of Object.values(selectedFaces)) for (const id of [...set]) if (!faces.some((f) => f.id === id)) set.delete(id);
  await renderFaceList();
  renderFacePicker();
}
async function renderFaceList() {
  $("face-list").innerHTML = faces.length ? faces.map((f) => `<div class="person" data-id="${esc(f.id)}">
      <div class="person-head"><strong>${esc(f.name)}</strong><span class="hint-inline">${f.images.length} Foto(s)</span>
        <label class="button-like secondary">Fotos hinzufügen<input type="file" accept="image/png,image/jpeg,image/webp" multiple hidden data-face-upload></label>
        <button type="button" class="ghost" data-face-delete>Person löschen</button></div>
      <div class="thumbs">${f.images.map((img) => `<div class="thumb"><img data-src="${esc(img)}" alt=""><button type="button" class="ghost" data-face-image="${esc(img)}" title="Foto entfernen">✕</button></div>`).join("") || '<span class="hint-inline">Noch keine Fotos.</span>'}</div>
    </div>`).join("") : `<p class="hint">Noch keine Personen.</p>`;
  for (const img of $("face-list").querySelectorAll("img[data-src]")) {
    const face = faces.find((f) => f.images.includes(img.dataset.src));
    const path = facePath(face, img.dataset.src);
    if (path) mediaUrl(path).then((u) => (img.src = u)).catch(() => {});
  }
}
// Absoluter Pfad eines Gesichtsfotos (vom Backend beim Upload geliefert, sonst aus dem Datenordner abgeleitet).
let facesDir = "";
const facePath = (face, img) => (facesDir ? `${facesDir}${img}` : null);

$("face-add").onclick = () => work("Person wird angelegt …", async () => {
  const face = await invoke("face_create", { name: $("face-name").value });
  $("face-name").value = "";
  await refreshFaces();
  status(`„${face.name}“ angelegt. Jetzt Fotos hinzufügen.`);
});
$("face-list").addEventListener("change", (ev) => {
  if (!ev.target.matches("[data-face-upload]")) return;
  const id = ev.target.closest(".person").dataset.id;
  const files = [...ev.target.files];
  ev.target.value = "";
  work("Fotos werden gespeichert …", async () => {
    for (const file of files) {
      const path = await uploadFile(file, "face", id);
      facesDir ||= path.slice(0, path.length - path.split(/[\\/]/).pop().length);
    }
    await refreshFaces();
    status(`${files.length} Foto(s) gespeichert.`);
  });
});
$("face-list").addEventListener("click", (ev) => {
  const person = ev.target.closest(".person");
  if (!person) return;
  const id = person.dataset.id;
  if (ev.target.matches("[data-face-image]")) {
    work("Foto wird entfernt …", async () => { await invoke("face_remove_image", { id, image: ev.target.dataset.faceImage }); await refreshFaces(); status("Foto entfernt."); });
  } else if (ev.target.matches("[data-face-delete]")) {
    if (!ev.target.classList.contains("armed")) { ev.target.classList.add("armed"); ev.target.textContent = "Wirklich löschen?"; return; }
    work("Person wird gelöscht …", async () => { await invoke("face_delete", { id }); await refreshFaces(); status("Person gelöscht."); });
  }
});

function renderFacePicker() {
  const set = selectedFaces[mode];
  const usable = faces.filter((f) => f.images.length);
  $("face-picker").innerHTML = usable.length
    ? usable.map((f) => `<label class="chip"><input type="checkbox" value="${esc(f.id)}"${set.has(f.id) ? " checked" : ""}> ${esc(f.name)}</label>`).join("")
    : `<span class="hint-inline">Noch keine Gesichter. Lege unten im Bereich „Gesichter“ Personen mit Fotos an.</span>`;
  const fam = currentFamily();
  const keyframeModels = imageModels().filter((m) => family(m).refs);
  if (mode === "image") {
    $("faces-hint").textContent = fam && !fam.refs ? "Für Gesichter ein Modell wie FLUX.3 Klein wählen." : "";
    $("keyframe-wrap").hidden = true;
  } else {
    $("faces-hint").textContent = "Mosaic erzeugt zuerst ein Startbild mit den Gesichtern und animiert es dann.";
    const keep = $("keyframe-model").value;
    $("keyframe-model").innerHTML = keyframeModels.length
      ? keyframeModels.map((m) => `<option value="${esc(m.id)}">${esc(m.name)}</option>`).join("")
      : `<option value="">Kein passendes Bildmodell – z. B. FLUX.3 Klein laden</option>`;
    if (keyframeModels.some((m) => m.id === keep)) $("keyframe-model").value = keep;
    $("keyframe-wrap").hidden = set.size === 0;
  }
}
$("face-picker").addEventListener("change", (ev) => {
  const set = selectedFaces[mode];
  ev.target.checked ? set.add(ev.target.value) : set.delete(ev.target.value);
  renderFacePicker();
});

// ---------- Bibliothek ----------
function guess(path) {
  const n = path.toLowerCase().split(/[\\/]/).pop();
  const fam = /wan.?2\.?2.*(ti2v|5b)/.test(n) ? "wan22" : /wan|umt5/.test(n) ? "wan" : /flux[-_.]?3|klein/.test(n) ? "flux3" : /flux/.test(n) ? "flux1" : /qwen|z[-_]?image|sd3|chroma|lumina/.test(n) ? "other" : "sd";
  let role = "checkpoint";
  if (/high.?noise/.test(n)) role = "high_noise";
  else if (/clip[-_]?vision/.test(n)) role = "clip_vision";
  else if (/(^|[-_.])(ae|vae)([-_.]|$)|vae/.test(n)) role = "vae";
  else if (/clip[-_]?l/.test(n)) role = "clip_l";
  else if (/clip[-_]?g/.test(n)) role = "clip_g";
  else if (/t5|umt5/.test(n)) role = "t5xxl";
  else if (/qwen|llm|mistral|gemma/.test(n)) role = "llm";
  else if (fam !== "sd") role = "diffusion";
  return { role, family: fam };
}
$("source").addEventListener("input", () => {
  const g = guess($("source").value.trim());
  $("role").value = g.role;
  $("family").value = g.family;
});

const formatSize = (b) => (b > 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${Math.round(b / 1e6)} MB`);
function renderLibrary() {
  $("library").innerHTML = entries.length
    ? entries.map((e) => `<div class="entry" data-id="${esc(e.id)}">
        <div class="name" title="${esc(e.path)}">${esc(e.name)}<small>${formatSize(e.size)} · ${e.managed ? "heruntergeladen" : "importiert"}</small></div>
        <select data-field="role">${options(ROLES, e.role)}</select>
        <select data-field="family">${options(FAMILIES, e.family)}</select>
        <button type="button" class="ghost" data-remove title="${e.managed ? "Datei löschen" : "Aus der Bibliothek entfernen"}">✕</button>
      </div>`).join("")
    : `<p class="hint">Noch keine Modelle.</p>`;
}
$("library").addEventListener("change", (ev) => {
  const row = ev.target.closest(".entry");
  if (!row) return;
  work("Speichere …", async () => {
    await invoke("update_entry", { id: row.dataset.id, role: row.querySelector('[data-field="role"]').value, family: row.querySelector('[data-field="family"]').value });
    await refresh();
    status("Gespeichert.");
  });
});
$("library").addEventListener("click", (ev) => {
  if (!ev.target.matches("[data-remove]")) return;
  const row = ev.target.closest(".entry");
  const entry = entries.find((e) => e.id === row.dataset.id);
  // Erster Klick fragt nach, zweiter Klick entfernt.
  if (!ev.target.classList.contains("armed")) {
    ev.target.classList.add("armed");
    ev.target.textContent = entry.managed ? "Datei löschen?" : "Entfernen?";
    return status(entry.managed ? `Noch einmal klicken, um „${entry.name}“ zu löschen.` : `Noch einmal klicken, um „${entry.name}“ aus der Bibliothek zu entfernen. Die Datei bleibt erhalten.`);
  }
  work("Entferne …", async () => { await invoke("remove_entry", { id: entry.id }); await refresh(); status("Entfernt."); });
});

async function addSource(source, role, fam) {
  return /^https?:\/\/|^[\w.-]+\/[\w.-]+$/.test(source) && !source.startsWith("/")
    ? invoke("download", { source, role, family: fam })
    : invoke("import_file", { path: source, role, family: fam });
}
$("add").onclick = () => work("Modell wird hinzugefügt …", async () => {
  const entry = await addSource($("source").value.trim(), $("role").value, $("family").value);
  $("source").value = "";
  await refresh(entry.id);
  status(`„${entry.name}“ ist bereit.`);
});
for (const btn of document.querySelectorAll("[data-preset]")) {
  btn.onclick = () => work("Paket wird geladen …", async () => {
    const preset = PRESETS[btn.dataset.preset];
    let main;
    for (const [i, f] of preset.files.entries()) {
      status(`Datei ${i + 1} von ${preset.files.length} wird geladen …`);
      const entry = await invoke("download", f);
      if (f.role === "diffusion" || f.role === "checkpoint") main = entry;
    }
    if (main && family(main).kind !== mode) setMode(family(main).kind);
    await refresh(main?.id);
    status(`${preset.label.split(" · ")[0].replace(/^\w+: /, "")} ist bereit.`);
  });
}

// Drag & Drop: Bilder werden zur Vorlage, Modelldateien kommen in die Bibliothek.
const onDrop = (handler) => { try { return getCurrentWebview().onDragDropEvent(handler); } catch { return Promise.resolve(); } };
onDrop(({ payload }) => {
  $("drop").classList.toggle("dragging", payload.type === "enter" || payload.type === "over");
  $("template-zone").classList.toggle("dragging", payload.type === "enter" || payload.type === "over");
  if (payload.type !== "drop" || busy) return;
  const image = payload.paths.find((p) => /\.(png|jpe?g|webp)$/i.test(p));
  const models = payload.paths.filter((p) => /\.(safetensors|gguf|ckpt|sft)$/i.test(p));
  if (image) {
    return work("Vorlage wird übernommen …", async () => { templates[mode] = await invoke("import_template", { path: image }); await renderTemplate(); status("Vorlage bereit."); });
  }
  if (!models.length) return status("Bitte ein Bild (Vorlage) oder Modelldateien (.safetensors, .gguf) ablegen.");
  work("Modelle werden eingebunden …", async () => {
    let last;
    for (const p of models) { const g = guess(p); last = await invoke("import_file", { path: p, role: g.role, family: g.family }); }
    await refresh(last.id);
    status(`${models.length} Datei(en) eingebunden. Prüfe in der Bibliothek, ob Art und Familie stimmen.`);
  });
}).catch(() => {});

// ---------- Generator ----------
const mainModels = () => entries.filter((e) => e.role === "checkpoint" || e.role === "diffusion");
const imageModels = () => mainModels().filter((e) => family(e).kind === "image");
const modeModels = () => mainModels().filter((e) => family(e).kind === mode);
function currentFamily() {
  const m = entries.find((e) => e.id === $("model").value);
  return m ? family(m) : null;
}
const lastModel = { image: "", video: "" };
function renderGenerator(selected) {
  const models = modeModels();
  const keep = (selected && models.some((m) => m.id === selected) && selected) || lastModel[mode] || $("model").value;
  const empty = mode === "video" ? "Noch kein Videomodell – unten „Video: Wan 2.2“ laden" : "Noch kein Bildmodell – unten eines hinzufügen";
  $("model").innerHTML = models.length
    ? models.map((m) => `<option value="${esc(m.id)}">${esc(m.name)} — ${esc(family(m).label)}</option>`).join("")
    : `<option value="">${empty}</option>`;
  if (models.some((m) => m.id === keep)) $("model").value = keep;
  applyFamily();
}
function componentSelects(model, container) {
  const fam = family(model);
  container.innerHTML = fam.parts.map((role) => {
    const candidates = entries.filter((e) => e.role === role).sort((a, b) => (b.family === model.family) - (a.family === model.family));
    const required = fam.required.includes(role);
    const opts = candidates.map((c) => `<option value="${esc(c.id)}">${esc(c.name)}</option>`).join("");
    return `<label>${esc(ROLES[role])}${required ? "" : " (optional)"}<select data-role="${role}">${required ? "" : '<option value="">—</option>'}${opts || (required ? '<option value="">fehlt – bitte hinzufügen</option>' : "")}</select></label>`;
  }).join("");
}
function readComponents(container) {
  const components = {};
  for (const sel of container.querySelectorAll("select")) components[sel.dataset.role] = sel.value;
  return components;
}
function applyFamily() {
  const model = entries.find((e) => e.id === $("model").value);
  $("parts").innerHTML = "";
  if (!model) { renderTemplate(); renderFacePicker(); return controls(busy); }
  lastModel[mode] = model.id;
  const fam = family(model);
  componentSelects(model, $("parts"));
  $("negative-wrap").hidden = !fam.negative;
  if (fam.kind === "image") {
    $("width").value = $("height").value = String(fam.size);
  } else {
    $("fps").value = fam.fps;
  }
  $("steps").value = /schnell/i.test(model.name) ? 4 : /dev/i.test(model.name) && model.family === "flux1" ? 20 : fam.steps;
  $("cfg").value = fam.cfg;
  $("sampler").value = fam.sampler;
  $("offload").checked = fam.offload;
  renderTemplate();
  renderFacePicker();
  controls(busy);
}
$("model").addEventListener("change", applyFamily);

function seedValue() {
  const text = $("seed").value.trim();
  return /^\d+$/.test(text) ? Number(text) : Math.floor(Math.random() * 2 ** 31);
}
function missingParts(model, components) {
  return family(model).required.filter((r) => !components[r]).map((r) => ROLES[r]);
}

async function showResult(path, seed) {
  const url = await mediaUrl(path);
  const isVideo = path.endsWith(".webm");
  $("picture").hidden = isVideo;
  $("movie").hidden = !isVideo;
  if (isVideo) { $("movie").src = url; $("movie").play().catch(() => {}); } else $("picture").src = url;
  $("path").textContent = `${path} · Seed ${seed}`;
  $("result").hidden = false;
}

$("studio").onsubmit = (e) => {
  e.preventDefault();
  const model = entries.find((x) => x.id === $("model").value);
  if (!model) return status(mode === "video" ? "Bitte zuerst ein Videomodell laden." : "Bitte zuerst ein Bildmodell laden.");
  const fam = family(model);
  const components = readComponents($("parts"));
  const missing = missingParts(model, components);
  if (missing.length) return status(`Für dieses Modell fehlt noch: ${missing.join(", ")}.`);
  const faceIds = [...selectedFaces[mode]];
  const template = templates[mode];
  const seed = seedValue();
  const base = {
    model: model.id, components, prompt: $("prompt").value,
    negative: fam.negative ? $("negative").value : "",
    steps: Number($("steps").value) || fam.steps, cfg: Number($("cfg").value),
    seed, sampler: $("sampler").value, offload: $("offload").checked,
    init_image: null, refs: [], faces: [],
  };

  if (mode === "image") {
    if (faceIds.length && !fam.refs) return status("Gesichter funktionieren mit Modellen wie FLUX.3 Klein. Bitte ein solches Modell wählen.");
    const request = { ...base, mode: "image", width: Number($("width").value), height: Number($("height").value), faces: faceIds };
    if (template) {
      if (fam.refs && $("template-mode").value === "ref") request.refs = [template];
      else { request.init_image = template; request.strength = Number($("strength").value); }
    }
    return work("Bild wird lokal berechnet … (beim ersten Mal kann das einige Minuten dauern)", async () => {
      const path = await invoke("generate", { device: $("device").value, request });
      await showResult(path, seed);
      status("Bild gespeichert.");
    });
  }

  // Video
  const [width, height] = $("video-size").value.split("x").map(Number);
  const video = {
    ...base, mode: "video", width, height,
    negative: fam.negative ? ($("negative").value.trim() || WAN_NEGATIVE) : "",
    frames: Number($("frames").value), fps: Number($("fps").value) || fam.fps, flow_shift: fam.flow || 0,
    init_image: template,
  };
  let keyframeRequest = null;
  if (faceIds.length) {
    const keyModel = entries.find((x) => x.id === $("keyframe-model").value);
    if (!keyModel) return status("Für Gesichter im Video wird ein Bildmodell wie FLUX.3 Klein gebraucht. Bitte unten „Bild: FLUX.3 Klein“ laden.");
    const keyFam = family(keyModel);
    const keyParts = {};
    for (const role of keyFam.parts) {
      const candidates = entries.filter((x) => x.role === role).sort((a, b) => (b.family === keyModel.family) - (a.family === keyModel.family));
      if (candidates.length) keyParts[role] = candidates[0].id;
    }
    const keyMissing = missingParts(keyModel, keyParts);
    if (keyMissing.length) return status(`Für das Startbild-Modell fehlt noch: ${keyMissing.join(", ")}.`);
    keyframeRequest = {
      mode: "image", model: keyModel.id, components: keyParts, prompt: $("prompt").value, negative: "",
      width, height, steps: keyFam.steps, cfg: keyFam.cfg, seed, sampler: keyFam.sampler, offload: keyFam.offload,
      init_image: null, refs: template ? [template] : [], faces: faceIds,
    };
  }
  work(keyframeRequest ? "Schritt 1 von 2: Startbild mit Gesichtern wird berechnet …" : "Video wird lokal berechnet … (das kann mehrere Minuten dauern)", async () => {
    if (keyframeRequest) {
      const keyframe = await invoke("generate", { device: $("device").value, request: keyframeRequest });
      await showResult(keyframe, seed);
      status("Schritt 2 von 2: Video wird berechnet … (das kann mehrere Minuten dauern)");
      video.init_image = keyframe;
    }
    const path = await invoke("generate", { device: $("device").value, request: video });
    await showResult(path, seed);
    status("Video gespeichert.");
  });
};

async function refresh(selected) {
  entries = await invoke("library");
  renderLibrary();
  renderGenerator(selected);
}

work("Wird geladen …", async () => {
  await listen("progress", ({ payload: [done, total, name] }) => {
    status(total ? `Download ${name} · ${Math.floor((done / total) * 100)} %` : `Download ${name} · ${Math.floor(done / 1048576)} MB`);
  });
  $("device").value = await invoke("profile");
  facesDir = await invoke("faces_dir").catch(() => "");
  setMode("image");
  await refresh();
  await refreshFaces();
  status(mainModels().length ? "Bereit." : "Für den Einstieg ein Paket laden (Bild: FLUX.3 Klein, Video: Wan 2.2) oder ein eigenes Modell hinzufügen.");
});
