import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import "./style.css";

// Modellfamilien: welches Zubehör sie brauchen und mit welchen Werten sie starten.
const FAMILIES = {
  sd: { label: "SD 1.5 / SDXL (Checkpoint)", parts: ["vae"], required: [], steps: 25, cfg: 7, size: 512, sampler: "euler_a", negative: true, offload: false },
  flux1: { label: "FLUX.1", parts: ["vae", "clip_l", "t5xxl"], required: ["vae", "clip_l", "t5xxl"], steps: 4, cfg: 1, size: 768, sampler: "euler", negative: false, offload: true },
  flux3: { label: "FLUX.3 Klein", parts: ["vae", "llm"], required: ["vae", "llm"], steps: 16, cfg: 1, size: 768, sampler: "euler", negative: false, offload: true },
  other: { label: "Anderes Modell (eigene Komponenten)", parts: ["vae", "clip_l", "clip_g", "t5xxl", "llm"], required: [], steps: 20, cfg: 4, size: 768, sampler: "euler", negative: true, offload: true },
};
const ROLES = {
  checkpoint: "Checkpoint (komplettes Modell)",
  diffusion: "Diffusionsmodell (z. B. FLUX)",
  vae: "VAE",
  clip_l: "Text-Encoder CLIP-L",
  clip_g: "Text-Encoder CLIP-G",
  t5xxl: "Text-Encoder T5-XXL",
  llm: "Text-Encoder LLM (z. B. Qwen)",
};
// Ein-Klick-Paket: kleinste FLUX-Variante, frei zugänglich, ca. 6,4 GB.
const PRESETS = {
  flux3: {
    label: "FLUX.3 Klein · ca. 6,4 GB",
    files: [
      { source: "https://huggingface.co/leejet/flux-3-image-klein-GGUF/blob/main/flux3-image-klein-Q4_K.gguf", role: "diffusion", family: "flux3" },
      { source: "https://huggingface.co/Qwen/Qwen3-VL-4B-Instruct-GGUF/blob/main/Qwen3VL-4B-Instruct-Q4_K_M.gguf", role: "llm", family: "flux3" },
      { source: "https://huggingface.co/Comfy-Org/Flux-2-Dev/blob/main/split_files/vae/flux2-vae.safetensors", role: "vae", family: "flux3" },
    ],
  },
};

const $ = (id) => document.getElementById(id);
const esc = (v) => String(v).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const options = (map, selected) => Object.entries(map).map(([k, v]) => `<option value="${k}"${k === selected ? " selected" : ""}>${esc(v.label ?? v)}</option>`).join("");
const sizeOptions = [512, 640, 768, 896, 1024].map((s) => `<option value="${s}">${s}</option>`).join("");

$("app").innerHTML = `<main>
<header><h1>Mosaic</h1><label class="device">Gerät<select id="device"><option value="arc">Intel Core Ultra · Arc (Windows)</option><option value="metal">Apple Silicon · Metal (macOS)</option></select></label></header>

<section class="card">
 <h2>Bild erstellen</h2>
 <form id="studio">
  <label>Modell<select id="model"></select></label>
  <div id="parts" class="row"></div>
  <label>Bildbeschreibung<textarea required maxlength="2000" id="prompt" rows="3" placeholder="Ein gelber Stadtbus auf einer regennassen Berliner Straße …"></textarea></label>
  <label id="negative-wrap">Negativer Prompt<input id="negative" maxlength="2000" placeholder="unscharf, verzerrt, Text"></label>
  <details><summary>Einstellungen</summary><div class="row">
   <label>Breite<select id="width">${sizeOptions}</select></label>
   <label>Höhe<select id="height">${sizeOptions}</select></label>
   <label>Schritte<input id="steps" type="number" min="1" max="150"></label>
   <label>CFG<input id="cfg" type="number" min="0" max="30" step="0.5"></label>
   <label>Sampler<select id="sampler"><option>euler</option><option>euler_a</option><option>dpm++2m</option><option>dpm++2s_a</option><option>lcm</option></select></label>
   <label>Seed<input id="seed" placeholder="zufällig"></label>
   <label class="check"><input type="checkbox" id="offload"> Speicher sparen</label>
  </div></details>
  <button id="create">Bild erstellen</button>
 </form>
 <figure hidden id="result"><img id="picture" alt="Generiertes Bild"><figcaption id="path"></figcaption></figure>
</section>

<section class="card" id="drop">
 <h2>Modelle</h2>
 <p class="hint">Neu hier? Lade das kleinste FLUX-Paket mit einem Klick oder ziehe eigene Modelldateien (.safetensors, .gguf) in dieses Fenster.</p>
 <div class="presets">${Object.entries(PRESETS).map(([k, p]) => `<button type="button" class="secondary" data-preset="${k}">${esc(p.label)} laden</button>`).join("")}</div>
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

let busy = false, imageUrl, entries = [];
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

// ---------- Bibliothek ----------
function guess(path) {
  const n = path.toLowerCase().split(/[\\/]/).pop();
  const family = /flux[-_.]?3|klein/.test(n) ? "flux3" : /flux/.test(n) ? "flux1" : /qwen|z[-_]?image|sd3|chroma|lumina/.test(n) ? "other" : "sd";
  let role = "checkpoint";
  if (/(^|[-_.])(ae|vae)([-_.]|$)|vae/.test(n)) role = "vae";
  else if (/clip[-_]?l/.test(n)) role = "clip_l";
  else if (/clip[-_]?g/.test(n)) role = "clip_g";
  else if (/t5/.test(n)) role = "t5xxl";
  else if (/qwen|llm|mistral|gemma/.test(n)) role = "llm";
  else if (family !== "sd") role = "diffusion";
  return { role, family };
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

async function addSource(source, role, family) {
  return /^https?:\/\/|^[\w.-]+\/[\w.-]+$/.test(source) && !source.startsWith("/")
    ? invoke("download", { source, role, family })
    : invoke("import_file", { path: source, role, family });
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
      if (f.role === "diffusion" || f.role === "checkpoint") main = entry.id;
    }
    await refresh(main);
    status(`${preset.label.split(" · ")[0]} ist bereit.`);
  });
}

// Dateien per Drag & Drop
const onDrop = (handler) => { try { return getCurrentWebview().onDragDropEvent(handler); } catch { return Promise.resolve(); } };
onDrop(({ payload }) => {
  $("drop").classList.toggle("dragging", payload.type === "enter" || payload.type === "over");
  if (payload.type !== "drop" || busy) return;
  const paths = payload.paths.filter((p) => /\.(safetensors|gguf|ckpt|sft)$/i.test(p));
  if (!paths.length) return status("Bitte .safetensors-, .gguf-, .ckpt- oder .sft-Dateien ablegen.");
  work("Modelle werden eingebunden …", async () => {
    let last;
    for (const p of paths) { const g = guess(p); last = await invoke("import_file", { path: p, role: g.role, family: g.family }); }
    await refresh(last.id);
    status(`${paths.length} Datei(en) eingebunden. Prüfe in der Bibliothek, ob Art und Familie stimmen.`);
  });
}).catch(() => {});

// ---------- Generator ----------
const mainModels = () => entries.filter((e) => e.role === "checkpoint" || e.role === "diffusion");
function currentFamily() {
  const m = entries.find((e) => e.id === $("model").value);
  return m ? FAMILIES[m.family] || FAMILIES.other : null;
}
function renderGenerator(selected) {
  const models = mainModels();
  const keep = selected || $("model").value;
  $("model").innerHTML = models.length
    ? models.map((m) => `<option value="${esc(m.id)}">${esc(m.name)} — ${esc((FAMILIES[m.family] || FAMILIES.other).label)}</option>`).join("")
    : `<option value="">Noch kein Modell – unten eines hinzufügen</option>`;
  if (models.some((m) => m.id === keep)) $("model").value = keep;
  applyFamily();
}
function applyFamily() {
  const fam = currentFamily();
  const model = entries.find((e) => e.id === $("model").value);
  $("parts").innerHTML = "";
  if (!fam) return controls(busy);
  $("parts").innerHTML = fam.parts.map((role) => {
    const candidates = entries.filter((e) => e.role === role).sort((a, b) => (b.family === model.family) - (a.family === model.family));
    const required = fam.required.includes(role);
    const opts = candidates.map((c) => `<option value="${esc(c.id)}">${esc(c.name)}</option>`).join("");
    return `<label>${esc(ROLES[role])}${required ? "" : " (optional)"}<select data-role="${role}">${required ? "" : '<option value="">—</option>'}${opts || (required ? '<option value="">fehlt – bitte hinzufügen</option>' : "")}</select></label>`;
  }).join("");
  $("negative-wrap").hidden = !fam.negative;
  $("width").value = $("height").value = String(fam.size);
  $("steps").value = /schnell/i.test(model.name) ? 4 : /dev/i.test(model.name) && model.family === "flux1" ? 20 : fam.steps;
  $("cfg").value = fam.cfg;
  $("sampler").value = fam.sampler;
  $("offload").checked = fam.offload;
  controls(busy);
}
$("model").addEventListener("change", applyFamily);

$("studio").onsubmit = (e) => {
  e.preventDefault();
  const fam = currentFamily();
  const components = {};
  for (const sel of $("parts").querySelectorAll("select")) components[sel.dataset.role] = sel.value;
  const missing = fam.required.filter((r) => !components[r]);
  if (missing.length) return status(`Für dieses Modell fehlt noch: ${missing.map((r) => ROLES[r]).join(", ")}.`);
  const seedText = $("seed").value.trim();
  const request = {
    model: $("model").value, components,
    prompt: $("prompt").value, negative: fam.negative ? $("negative").value : "",
    width: Number($("width").value), height: Number($("height").value),
    steps: Number($("steps").value) || fam.steps, cfg: Number($("cfg").value),
    seed: /^\d+$/.test(seedText) ? Number(seedText) : Math.floor(Math.random() * 2 ** 31),
    sampler: $("sampler").value, offload: $("offload").checked,
  };
  work("Bild wird lokal berechnet … (beim ersten Mal kann das einige Minuten dauern)", async () => {
    const path = await invoke("generate", { device: $("device").value, request });
    const data = await invoke("preview", { name: path });
    if (imageUrl) URL.revokeObjectURL(imageUrl);
    imageUrl = URL.createObjectURL(new Blob([data], { type: "image/png" }));
    $("picture").src = imageUrl;
    $("path").textContent = `${path} · Seed ${request.seed}`;
    $("result").hidden = false;
    status("Bild gespeichert.");
  });
};

async function refresh(selected) {
  entries = await invoke("library");
  renderLibrary();
  renderGenerator(selected);
}

work("Modelle werden geladen …", async () => {
  await listen("progress", ({ payload: [done, total, name] }) => {
    status(total ? `Download ${name} · ${Math.floor((done / total) * 100)} %` : `Download ${name} · ${Math.floor(done / 1048576)} MB`);
  });
  $("device").value = await invoke("profile");
  await refresh();
  status(mainModels().length ? "Bereit." : "Für den Einstieg FLUX.3 Klein laden oder ein eigenes Modell hinzufügen.");
});
