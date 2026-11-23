use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};
use url::Url;

type Result<T> = std::result::Result<T, String>;
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn folder(app: &tauri::AppHandle, name: &str) -> Result<PathBuf> {
    let path = app.path().app_local_data_dir().map_err(error)?.join(name);
    fs::create_dir_all(&path).map_err(error)?;
    Ok(path)
}

/// Rollen entsprechen den Komponenten-Optionen von sd-cli.
const ROLES: [(&str, &str); 10] = [
    ("checkpoint", "-m"),
    ("diffusion", "--diffusion-model"),
    ("high_noise", "--high-noise-diffusion-model"),
    ("clip_vision", "--clip_vision"),
    ("vae", "--vae"),
    ("clip_l", "--clip_l"),
    ("clip_g", "--clip_g"),
    ("t5xxl", "--t5xxl"),
    ("llm", "--llm"),
    ("llm_vision", "--llm_vision"),
];
const EXTENSIONS: [&str; 4] = ["safetensors", "gguf", "ckpt", "sft"];

const NSFW_TERMS: &[&str] = &[
    "nsfw", "porno", "porn", "pornografie", "pornography", "xxx", "nude", "nudes", "naked", "nackt",
    "erotic", "erotik", "erotisch", "sex", "sexual", "sexuell", "hentai", "explicit", "topless",
    "oben ohne", "genitals", "genitalien", "penis", "vagina", "vulva", "boobs", "tits", "breasts",
    "brüste", "nipples", "nippel", "fetish", "fetisch", "bdsm", "hardcore", "softcore", "uncensored",
    "unzensiert", "blowjob", "handjob", "intercourse", "geschlechtsverkehr", "strip", "striptease",
    "lingerie", "unterwäsche", "bikini", "cum", "orgasm", "orgasmus", "anal", "lesbian", "gay sex",
];

const PEOPLE_TERMS: &[&str] = &[
    "mensch", "menschen", "person", "personen", "mann", "männer", "frau", "frauen", "kind", "kinder",
    "mädchen", "junge", "jungen", "baby", "babys", "kleinkind", "porträt", "portrait", "gesicht",
    "gesichter", "selfie", "model", "schauspieler", "schauspielerin", "human", "humans", "people",
    "man", "men", "woman", "women", "girl", "girls", "boy", "boys", "child", "children", "face",
    "faces", "lady", "gentleman", "couple", "paar", "crowd", "menschenmenge", "passant", "passanten",
    "fußgänger", "pedestrian", "pedestrians", "figure", "figur", "körper", "body",
];

const MINOR_TERMS: &[&str] = &[
    "kind", "kinder", "mädchen", "junge", "jungen", "baby", "babys", "kleinkind", "minderjährig",
    "minderjährige", "child", "children", "girl", "girls", "boy", "boys", "kid", "kids", "minor",
    "minors", "underage", "teen", "teenager", "toddler", "infant", "loli", "shota",
];

fn normalize_words(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push(' ');
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
        } else if !out.ends_with(' ') {
            out.push(' ');
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

fn has_term(normalized: &str, terms: &[&str]) -> bool {
    terms.iter().any(|t| normalized.contains(&format!(" {t} ")))
}

/// Eine Modelldatei in der Bibliothek: heruntergeladen (managed) oder importiert (Verweis).
#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    id: String,
    name: String,
    role: String,
    family: String,
    path: String,
    managed: bool,
    size: u64,
    /// Freigabe für den Whitelist-Modus.
    #[serde(default)]
    approved: bool,
}

fn library_file(app: &tauri::AppHandle) -> Result<PathBuf> {
    Ok(folder(app, "")?.join("library.json"))
}

fn load_library(app: &tauri::AppHandle) -> Result<Vec<Entry>> {
    let file = library_file(app)?;
    let mut entries: Vec<Entry> = match fs::read_to_string(&file) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    // Übernahme aus Version 0.1: lose Checkpoints im Modellordner.
    for item in fs::read_dir(folder(app, "models")?).map_err(error)? {
        let path = item.map_err(error)?.path();
        if !path.is_file() || !has_model_extension(&path) {
            continue;
        }
        let path_text = path.to_string_lossy().into_owned();
        if !entries.iter().any(|e| e.path == path_text) {
            let file_name = path.file_name().unwrap_or_default().to_string_lossy();
            entries.push(new_entry(&path, &display_name(&file_name), "checkpoint", "sd", true));
        }
    }
    Ok(entries)
}

fn save_library(app: &tauri::AppHandle, entries: &[Entry]) -> Result<()> {
    let file = library_file(app)?;
    let tmp = file.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(entries).map_err(error)?).map_err(error)?;
    fs::rename(tmp, file).map_err(error)
}

fn has_model_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

fn display_name(file_name: &str) -> String {
    // Entfernt den Hash-Präfix heruntergeladener Dateien.
    let trimmed = match file_name.split_once('-') {
        Some((prefix, rest)) if prefix.len() == 16 && prefix.bytes().all(|b| b.is_ascii_hexdigit()) => rest,
        _ => file_name,
    };
    trimmed.to_string()
}

fn new_entry(path: &Path, name: &str, role: &str, family: &str, managed: bool) -> Entry {
    let path_text = path.to_string_lossy().into_owned();
    let mut hash: u64 = 14695981039346656037;
    for b in path_text.bytes() {
        hash = (hash ^ u64::from(b)).wrapping_mul(1099511628211);
    }
    Entry {
        id: format!("{hash:016x}"),
        name: name.to_string(),
        role: role.to_string(),
        family: family.to_string(),
        path: path_text,
        managed,
        size: fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        approved: false,
    }
}

fn check_kind(role: &str, family: &str) -> Result<()> {
    if !ROLES.iter().any(|(r, _)| *r == role) {
        return Err("Unbekannte Rolle für die Modelldatei.".into());
    }
    if !family.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') || family.is_empty() || family.len() > 32 {
        return Err("Unbekannte Modellfamilie.".into());
    }
    Ok(())
}

fn upsert(app: &tauri::AppHandle, entry: Entry) -> Result<Entry> {
    let mut entries = load_library(app)?;
    let mut entry = entry;
    if let Some(old) = entries.iter().find(|e| e.path == entry.path) {
        entry.approved = old.approved;
    }
    entries.retain(|e| e.path != entry.path);
    entries.push(entry.clone());
    save_library(app, &entries)?;
    Ok(entry)
}

/// Dauerhafte App-Einstellungen (settings.json im Datenordner).
#[derive(Serialize, Deserialize, Clone, Default)]
struct Settings {
    /// Nur freigegebene Modelle (Whitelist) verwenden.
    #[serde(default)]
    whitelist_only: bool,
}

fn load_settings(app: &tauri::AppHandle) -> Result<Settings> {
    match fs::read_to_string(folder(app, "")?.join("settings.json")) {
        Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
        Err(_) => Ok(Settings::default()),
    }
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Result<Settings> {
    load_settings(&app)
}

#[tauri::command]
fn set_settings(app: tauri::AppHandle, settings: Settings) -> Result<()> {
    let file = folder(&app, "")?.join("settings.json");
    fs::write(file, serde_json::to_vec_pretty(&settings).map_err(error)?).map_err(error)
}

#[tauri::command]
fn approve_entry(app: tauri::AppHandle, id: String, approved: bool) -> Result<()> {
    let mut entries = load_library(&app)?;
    let entry = entries.iter_mut().find(|e| e.id == id).ok_or("Modell nicht gefunden.")?;
    entry.approved = approved;
    save_library(&app, &entries)
}

#[tauri::command]
fn profile() -> &'static str {
    if cfg!(target_os = "windows") {
        "arc"
    } else {
        "metal"
    }
}

#[tauri::command]
fn library(app: tauri::AppHandle) -> Result<Vec<Entry>> {
    let mut entries = load_library(&app)?;
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(entries)
}

/// Bindet eine lokale Modelldatei ein, ohne sie zu kopieren.
#[tauri::command]
fn import_file(app: tauri::AppHandle, path: String, role: String, family: String) -> Result<Entry> {
    check_kind(&role, &family)?;
    let path = PathBuf::from(path.trim().trim_matches('"'));
    if !path.is_file() {
        return Err("Datei nicht gefunden. Bitte den vollständigen Pfad zu einer Modelldatei angeben.".into());
    }
    if !has_model_extension(&path) {
        return Err("Unterstützt werden .safetensors, .gguf, .ckpt und .sft.".into());
    }
    let path = path.canonicalize().map_err(error)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    upsert(&app, new_entry(&path, &name, &role, &family, false))
}

#[tauri::command]
fn update_entry(app: tauri::AppHandle, id: String, role: String, family: String) -> Result<()> {
    check_kind(&role, &family)?;
    let mut entries = load_library(&app)?;
    let entry = entries.iter_mut().find(|e| e.id == id).ok_or("Modell nicht gefunden.")?;
    entry.role = role;
    entry.family = family;
    save_library(&app, &entries)
}

/// Entfernt einen Eintrag. Heruntergeladene Dateien werden gelöscht, importierte bleiben liegen.
#[tauri::command]
fn remove_entry(app: tauri::AppHandle, id: String) -> Result<()> {
    let mut entries = load_library(&app)?;
    let models_dir = folder(&app, "models")?;
    if let Some(entry) = entries.iter().find(|e| e.id == id) {
        let path = Path::new(&entry.path);
        if entry.managed && path.parent() == Some(models_dir.as_path()) {
            let _ = fs::remove_file(path);
        }
    }
    entries.retain(|e| e.id != id);
    save_library(&app, &entries)
}

fn valid_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 100
        && !part.starts_with('.')
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}

#[tauri::command]
async fn download(app: tauri::AppHandle, source: String, role: String, family: String) -> Result<Entry> {
    check_kind(&role, &family)?;
    let raw = source
        .trim()
        .trim_start_matches("https://huggingface.co/")
        .split('?')
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    let parts: Vec<_> = raw.split('/').collect();
    if parts.len() < 2 || !parts.iter().all(|p| valid_part(p)) {
        return Err("Bitte eine Hugging-Face-Modell-ID oder einen Dateilink eintragen.".into());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(90))
        .build()
        .map_err(error)?;
    let repo = format!("{}/{}", parts[0], parts[1]);
    let is_model = |s: &&str| EXTENSIONS.iter().any(|ext| s.ends_with(&format!(".{ext}")));
    let (revision, file) = if parts.len() == 2 {
        let response = client
            .get(format!("https://huggingface.co/api/models/{repo}"))
            .send()
            .await
            .map_err(error)?
            .error_for_status()
            .map_err(|_| "Repository nicht gefunden oder zugangsbeschränkt.".to_string())?;
        let info: serde_json::Value = response.json().await.map_err(error)?;
        let files = info["siblings"]
            .as_array()
            .ok_or("Keine Modelldateien gefunden.")?;
        let options: Vec<_> = files
            .iter()
            .filter_map(|v| v["rfilename"].as_str())
            .filter(|s| s.split('/').all(valid_part))
            .filter(is_model)
            .collect();
        if options.len() != 1 {
            return Err("Das Repository enthält mehrere Dateien. Bitte auf Hugging Face unter „Files“ den Link zur gewünschten Datei kopieren.".into());
        }
        ("main".to_owned(), options[0].to_string())
    } else {
        if parts.len() < 5 || !matches!(parts[2], "blob" | "resolve") {
            return Err("Unbekannter Hugging-Face-Dateilink.".into());
        }
        (parts[3].to_owned(), parts[4..].join("/"))
    };
    if !is_model(&file.as_str()) {
        return Err("Unterstützt werden .safetensors, .gguf, .ckpt und .sft.".into());
    }
    let url = Url::parse(&format!(
        "https://huggingface.co/{repo}/resolve/{revision}/{file}"
    ))
    .map_err(error)?;
    // Ein stabiler Dateiname hält Revisionen und gleichnamige Repositories auseinander.
    let mut hash: u64 = 14695981039346656037;
    for b in url.as_str().bytes() {
        hash = (hash ^ u64::from(b)).wrapping_mul(1099511628211);
    }
    let short = file.rsplit('/').next().unwrap_or("model.safetensors").to_string();
    let dest = folder(&app, "models")?.join(format!("{hash:016x}-{short}"));
    if dest.is_file() {
        return upsert(&app, new_entry(&dest, &short, &role, &family, true));
    }
    let mut r = client
        .get(url)
        .send()
        .await
        .map_err(error)?
        .error_for_status()
        .map_err(|_| "Download nicht möglich. Ist die Datei zugangsbeschränkt (Anmeldung auf Hugging Face nötig)?".to_string())?;
    let total = r.content_length();
    let part = dest.with_extension(format!("{}.part", std::process::id()));
    let write = async {
        let mut f = fs::File::create(&part).map_err(error)?;
        let mut received = 0u64;
        let mut next = 0;
        while let Some(bytes) = r.chunk().await.map_err(error)? {
            f.write_all(&bytes).map_err(error)?;
            received += bytes.len() as u64;
            if received >= next {
                let _ = app.emit("progress", (received, total, short.clone()));
                next = received + 4 * 1024 * 1024;
            }
        }
        if received < 8 || total.is_some_and(|n| n != received) {
            return Err("Download unvollständig. Bitte erneut laden.".to_owned());
        }
        f.sync_all().map_err(error)?;
        drop(f);
        fs::rename(&part, &dest).map_err(error)
    }
    .await;
    if let Err(e) = write {
        let _ = fs::remove_file(part);
        return Err(e);
    }
    upsert(&app, new_entry(&dest, &short, &role, &family, true))
}

fn default_strength() -> f32 {
    0.75
}
fn default_frames() -> u32 {
    33
}
fn default_fps() -> u32 {
    24
}
fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
struct GenerateRequest {
    #[serde(default)]
    mode: String,
    model: String,
    components: HashMap<String, String>,
    prompt: String,
    negative: String,
    width: u32,
    height: u32,
    steps: u32,
    cfg: f32,
    seed: i64,
    sampler: String,
    offload: bool,
    #[serde(default)]
    allow_nsfw: bool,
    #[serde(default = "default_true")]
    allow_people: bool,
    /// Vorlage als Ausgangsbild (img2img) bzw. Startbild eines Videos.
    #[serde(default)]
    init_image: Option<String>,
    #[serde(default = "default_strength")]
    strength: f32,
    /// Vorlagen als Referenzbilder (z. B. FLUX.3 Klein, Qwen Image Edit).
    #[serde(default)]
    refs: Vec<String>,
    /// Personen aus der Gesichter-Datenbank, deren Fotos als Referenz dienen.
    #[serde(default)]
    faces: Vec<String>,
    #[serde(default = "default_frames")]
    frames: u32,
    #[serde(default = "default_fps")]
    fps: u32,
    #[serde(default)]
    flow_shift: f32,
}

/// Liefert einen Pfad nur, wenn er innerhalb der Mosaic-Daten liegt.
fn inside_data(app: &tauri::AppHandle, path: &str) -> Result<PathBuf> {
    let base = folder(app, "")?.canonicalize().map_err(error)?;
    let path = Path::new(path).canonicalize().map_err(|_| "Datei nicht gefunden.".to_string())?;
    if !path.starts_with(&base) {
        return Err("Ungültiger Dateipfad.".into());
    }
    Ok(path)
}

const IMAGE_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];
const MAX_REFS: usize = 10;

#[tauri::command]
async fn generate(app: tauri::AppHandle, device: String, request: GenerateRequest) -> Result<String> {
    if device != profile() {
        return Err("Bitte das Profil für dieses Betriebssystem auswählen.".into());
    }
    let r = request;
    let video = match r.mode.as_str() {
        "" | "image" => false,
        "video" => true,
        _ => return Err("Unbekannter Modus.".into()),
    };
    if r.prompt.trim().is_empty() || r.prompt.len() > 8000 || r.negative.len() > 8000 {
        return Err("Bitte einen Prompt mit höchstens 8000 Bytes eingeben.".into());
    }

    // Inhaltsprüfung für NSFW/Porno und Menschen
    let norm_prompt = normalize_words(&r.prompt);
    let prompt_has_nsfw = has_term(&norm_prompt, NSFW_TERMS);
    let prompt_has_minor = has_term(&norm_prompt, MINOR_TERMS);
    if prompt_has_nsfw && prompt_has_minor {
        return Err("Darstellungen von Minderjährigen in NSFW-Kontexten sind grundsätzlich unzulässig.".into());
    }
    // Mit Gesichtsfotos oder Vorlagen (z. B. Fotos echter Personen) bleibt NSFW immer gesperrt.
    let uses_real_images = !r.faces.is_empty() || !r.refs.is_empty() || r.init_image.as_deref().is_some_and(|p| !p.is_empty());
    let allow_nsfw = r.allow_nsfw && !uses_real_images;
    if prompt_has_nsfw && uses_real_images {
        return Err("NSFW ist mit Gesichtern oder Vorlagen grundsätzlich gesperrt, damit keine sexuellen Bilder echter Personen entstehen.".into());
    }
    if !allow_nsfw && prompt_has_nsfw {
        return Err("NSFW / Porno ist ausgeschaltet. Bitte den Schalter „NSFW & Porno erlauben“ aktivieren oder den Prompt anpassen.".into());
    }
    if !r.allow_people {
        if !r.faces.is_empty() {
            return Err("„Menschen generieren“ ist ausgeschaltet. Bitte zuerst aktivieren oder ausgewählte Gesichter abwählen.".into());
        }
        if has_term(&norm_prompt, PEOPLE_TERMS) {
            return Err("„Menschen generieren“ ist ausgeschaltet, aber der Prompt beschreibt Personen. Bitte aktivieren oder den Prompt anpassen.".into());
        }
    }

    let sizes_ok = |v: u32| (256..=2048).contains(&v) && v % 16 == 0;
    if !sizes_ok(r.width) || !sizes_ok(r.height) || !(1..=150).contains(&r.steps) || !(0.0..=30.0).contains(&r.cfg) {
        return Err("Ungültige Einstellungen für Größe, Schritte oder CFG.".into());
    }
    if !(0.0..=1.0).contains(&r.strength) || !(0.0..=20.0).contains(&r.flow_shift) {
        return Err("Ungültige Stärke oder Flow-Shift.".into());
    }
    if video && (!(5..=241).contains(&r.frames) || r.frames % 4 != 1 || !(1..=60).contains(&r.fps)) {
        return Err("Ungültige Videolänge oder Bildrate.".into());
    }
    if !["euler", "euler_a", "dpm++2m", "dpm++2s_a", "lcm"].contains(&r.sampler.as_str()) {
        return Err("Unbekanntes Sampling-Verfahren.".into());
    }
    let entries = load_library(&app)?;
    let whitelist_only = load_settings(&app)?.whitelist_only;
    let find = |id: &str| -> Result<&Entry> {
        let entry = entries.iter().find(|e| e.id == id).ok_or("Modell nicht gefunden. Bitte erneut auswählen.")?;
        if whitelist_only && !entry.approved {
            return Err(format!("„{}“ ist nicht in der Whitelist freigegeben. In der Bibliothek freigeben oder den Whitelist-Modus ausschalten.", entry.name));
        }
        if !Path::new(&entry.path).is_file() {
            return Err(format!("Datei fehlt: {}. Wurde sie verschoben oder gelöscht?", entry.path));
        }
        Ok(entry)
    };
    let mut args: Vec<String> = Vec::new();
    if video {
        args.extend(["-M".into(), "vid_gen".into()]);
    }
    let main = find(&r.model)?;
    let flag = |role: &str| ROLES.iter().find(|(name, _)| *name == role).map(|(_, f)| *f);
    match main.role.as_str() {
        "checkpoint" | "diffusion" => {
            args.push(flag(&main.role).unwrap_or("-m").into());
            args.push(main.path.clone());
        }
        _ => return Err("Bitte ein Hauptmodell (Checkpoint oder Diffusionsmodell) auswählen.".into()),
    }
    for (role, id) in &r.components {
        if id.is_empty() {
            continue;
        }
        let entry = find(id)?;
        let option = flag(role).filter(|_| !matches!(role.as_str(), "checkpoint" | "diffusion")).ok_or("Ungültige Komponente.")?;
        args.push(option.into());
        args.push(entry.path.clone());
    }
    if let Some(init) = r.init_image.as_deref().filter(|p| !p.is_empty()) {
        args.push("-i".into());
        args.push(inside_data(&app, init)?.to_string_lossy().into_owned());
        if !video {
            args.extend(["--strength".into(), r.strength.to_string()]);
        }
    }
    let mut refs: Vec<PathBuf> = Vec::new();
    for path in &r.refs {
        refs.push(inside_data(&app, path)?);
    }
    let mut face_names: Vec<String> = Vec::new();
    if !r.faces.is_empty() {
        let faces = load_faces(&app)?;
        let dir = folder(&app, "faces")?;
        for id in &r.faces {
            let face = faces.iter().find(|f| &f.id == id).ok_or("Person nicht gefunden.")?;
            if !face.consent {
                return Err(format!("Für „{}“ fehlt die Bestätigung der Einwilligung. Bitte im Bereich „Gesichter“ bestätigen.", face.name));
            }
            if face.images.is_empty() {
                return Err(format!("Für „{}“ gibt es noch keine Fotos.", face.name));
            }
            // Bis zu drei Fotos pro Person reichen für die Identität.
            refs.extend(face.images.iter().take(3).map(|img| dir.join(img)));
            face_names.push(face.name.clone());
        }
    }
    if refs.len() > MAX_REFS {
        return Err(format!("Höchstens {MAX_REFS} Referenzbilder (Vorlage + Gesichtsfotos) gleichzeitig."));
    }
    for path in &refs {
        args.push("-r".into());
        args.push(path.to_string_lossy().into_owned());
    }
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(error)?
        .as_nanos();
    let out = if video {
        folder(&app, "videos")?.join(format!("mosaic-{id}.webm"))
    } else {
        folder(&app, "images")?.join(format!("mosaic-{id}.png"))
    };
    args.extend([
        "-p".into(), r.prompt.clone(),
        "-W".into(), r.width.to_string(),
        "-H".into(), r.height.to_string(),
        "--steps".into(), r.steps.to_string(),
        "--cfg-scale".into(), r.cfg.to_string(),
        "--sampling-method".into(), r.sampler.clone(),
        "-s".into(), r.seed.to_string(),
    ]);
    if video {
        args.extend(["--video-frames".into(), r.frames.to_string(), "--fps".into(), r.fps.to_string()]);
    }
    if r.flow_shift > 0.0 {
        args.extend(["--flow-shift".into(), r.flow_shift.to_string()]);
    }
    let mut neg_parts: Vec<String> = Vec::new();
    if !r.negative.trim().is_empty() {
        neg_parts.push(r.negative.trim().to_string());
    }
    if !allow_nsfw {
        neg_parts.push("nsfw, nude, naked, porn, explicit, erotic, sexual, topless".into());
    }
    if !r.allow_people {
        neg_parts.push("person, people, human, man, woman, face, portrait, crowd, pedestrian".into());
    }
    let effective_negative = neg_parts.join(", ");
    if !effective_negative.is_empty() {
        args.extend(["-n".into(), effective_negative]);
    }
    if r.offload {
        args.push("--offload-to-cpu".into());
    }
    let model_name = main.name.clone();
    let result = run_engine(&app, args, out).await?;
    let _ = append_history(&app, serde_json::json!({
        "time": SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        "mode": if video { "video" } else { "image" },
        "prompt": r.prompt,
        "negative": r.negative,
        "model": model_name,
        "faces": face_names,
        "width": r.width,
        "height": r.height,
        "steps": r.steps,
        "cfg": r.cfg,
        "seed": r.seed,
        "output": result,
    }));
    Ok(result)
}

// ---------- Verlauf ----------

fn append_history(app: &tauri::AppHandle, item: serde_json::Value) -> Result<()> {
    let file = folder(app, "")?.join("history.jsonl");
    let mut f = fs::OpenOptions::new().create(true).append(true).open(file).map_err(error)?;
    writeln!(f, "{item}").map_err(error)
}

/// Die letzten Einträge des automatisch gespeicherten Verlaufs, neueste zuerst.
#[tauri::command]
fn history(app: tauri::AppHandle) -> Result<Vec<serde_json::Value>> {
    let text = fs::read_to_string(folder(&app, "")?.join("history.jsonl")).unwrap_or_default();
    let mut items: Vec<serde_json::Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    items.reverse();
    items.truncate(200);
    Ok(items)
}

#[tauri::command]
fn history_clear(app: tauri::AppHandle) -> Result<()> {
    let file = folder(&app, "")?.join("history.jsonl");
    if file.exists() {
        fs::remove_file(file).map_err(error)?;
    }
    Ok(())
}

async fn run_engine(app: &tauri::AppHandle, args: Vec<String>, out: PathBuf) -> Result<String> {
    let log_file = folder(app, "logs")?.join("last-run.log");
    let exe = app
        .path()
        .resource_dir()
        .map_err(error)?
        .join("engine")
        .join(if cfg!(windows) { "sd-cli.exe" } else { "sd-cli" });
    let guard = app.state::<Mutex<bool>>();
    {
        let mut busy = guard.lock().map_err(error)?;
        if *busy {
            return Err("Eine Generierung läuft bereits.".into());
        }
        *busy = true;
    }
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = Command::new(&exe);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        let result = cmd
            .current_dir(exe.parent().ok_or("Engine-Pfad ungültig.")?)
            .args(&args)
            .arg("-o")
            .arg(&out)
            .output()
            .map_err(|e| format!("Bild-Engine konnte nicht starten ({e})."))?;
        // Protokoll des letzten Laufs für den Bug-Report.
        let log = format!(
            "Befehl: sd-cli {}\nStatus: {}\n\n--- stdout ---\n{}\n--- stderr ---\n{}",
            args.join(" "),
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let _ = fs::write(&log_file, log);
        if !result.status.success() || !out.is_file() {
            let log = String::from_utf8_lossy(&result.stderr);
            let tail: Vec<_> = log.lines().rev().filter(|l| !l.trim().is_empty()).take(3).collect();
            return Err(format!(
                "Generierung fehlgeschlagen. Passen Modell und Zubehör (VAE, Text-Encoder) zusammen? Genug freier Speicher? {}",
                tail.into_iter().rev().collect::<Vec<_>>().join(" ")
            ));
        }
        Ok(out.to_string_lossy().into_owned())
    })
    .await;
    *guard.lock().map_err(error)? = false;
    result.map_err(error)?
}

/// Liefert Bilder und Videos aus den Mosaic-Daten an die Oberfläche.
#[tauri::command]
async fn media(app: tauri::AppHandle, path: String) -> Result<tauri::ipc::Response> {
    let path = inside_data(&app, &path)?;
    let ext = path.extension().and_then(|v| v.to_str()).unwrap_or("").to_ascii_lowercase();
    if !IMAGE_EXTENSIONS.contains(&ext.as_str()) && ext != "webm" {
        return Err("Ungültiger Dateityp.".into());
    }
    Ok(tauri::ipc::Response::new(fs::read(path).map_err(error)?))
}

// ---------- Vorlagen und Gesichter ----------

#[derive(Serialize, Deserialize, Clone)]
struct Face {
    id: String,
    name: String,
    images: Vec<String>,
    /// Bestätigung: Die Person hat eingewilligt (oder es ist die eigene Person).
    #[serde(default)]
    consent: bool,
}

fn faces_file(app: &tauri::AppHandle) -> Result<PathBuf> {
    Ok(folder(app, "faces")?.join("faces.json"))
}
fn load_faces(app: &tauri::AppHandle) -> Result<Vec<Face>> {
    match fs::read_to_string(faces_file(app)?) {
        Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
        Err(_) => Ok(Vec::new()),
    }
}
fn save_faces(app: &tauri::AppHandle, faces: &[Face]) -> Result<()> {
    let file = faces_file(app)?;
    let tmp = file.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(faces).map_err(error)?).map_err(error)?;
    fs::rename(tmp, file).map_err(error)
}
fn unique_id() -> Result<String> {
    Ok(format!("{:x}", SystemTime::now().duration_since(UNIX_EPOCH).map_err(error)?.as_nanos()))
}

#[tauri::command]
fn faces(app: tauri::AppHandle) -> Result<Vec<Face>> {
    load_faces(&app)
}

/// Ordner der Gesichtsfotos (mit abschließendem Trennzeichen) für die Vorschaubilder.
#[tauri::command]
fn faces_dir(app: tauri::AppHandle) -> Result<String> {
    let mut path = folder(&app, "faces")?.to_string_lossy().into_owned();
    path.push(std::path::MAIN_SEPARATOR);
    Ok(path)
}

#[tauri::command]
fn face_create(app: tauri::AppHandle, name: String, consent: bool) -> Result<Face> {
    if !consent {
        return Err("Bitte bestätigen, dass die Person eingewilligt hat oder du es selbst bist.".into());
    }
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err("Bitte einen Namen mit höchstens 60 Zeichen eingeben.".into());
    }
    let mut faces = load_faces(&app)?;
    let face = Face { id: unique_id()?, name: name.to_string(), images: Vec::new(), consent: true };
    faces.push(face.clone());
    save_faces(&app, &faces)?;
    Ok(face)
}

#[tauri::command]
fn face_confirm(app: tauri::AppHandle, id: String) -> Result<()> {
    let mut faces = load_faces(&app)?;
    faces.iter_mut().find(|f| f.id == id).ok_or("Person nicht gefunden.")?.consent = true;
    save_faces(&app, &faces)
}

#[tauri::command]
fn face_delete(app: tauri::AppHandle, id: String) -> Result<()> {
    let mut faces = load_faces(&app)?;
    let dir = folder(&app, "faces")?;
    if let Some(face) = faces.iter().find(|f| f.id == id) {
        for img in &face.images {
            let _ = fs::remove_file(dir.join(img));
        }
    }
    faces.retain(|f| f.id != id);
    save_faces(&app, &faces)
}

#[tauri::command]
fn face_remove_image(app: tauri::AppHandle, id: String, image: String) -> Result<()> {
    let mut faces = load_faces(&app)?;
    let face = faces.iter_mut().find(|f| f.id == id).ok_or("Person nicht gefunden.")?;
    if face.images.contains(&image) {
        let _ = fs::remove_file(folder(&app, "faces")?.join(&image));
        face.images.retain(|i| i != &image);
    }
    save_faces(&app, &faces)
}

/// Speichert ein hochgeladenes Bild (Rohdaten) als Vorlage oder als Foto einer Person.
#[tauri::command]
fn upload(app: tauri::AppHandle, request: tauri::ipc::Request<'_>) -> Result<String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("Ungültige Bilddaten.".into());
    };
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let ext = header("x-ext").to_ascii_lowercase();
    if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Err("Bitte ein PNG-, JPG- oder WebP-Bild wählen.".into());
    }
    if bytes.len() < 16 || bytes.len() > 40 * 1024 * 1024 {
        return Err("Das Bild ist leer oder größer als 40 MB.".into());
    }
    let id = unique_id()?;
    match header("x-kind").as_str() {
        "template" => {
            let path = folder(&app, "templates")?.join(format!("vorlage-{id}.{ext}"));
            fs::write(&path, bytes).map_err(error)?;
            Ok(path.to_string_lossy().into_owned())
        }
        "face" => {
            let face_id = header("x-face");
            let mut faces = load_faces(&app)?;
            let face = faces.iter_mut().find(|f| f.id == face_id).ok_or("Person nicht gefunden.")?;
            if face.images.len() >= 20 {
                return Err("Höchstens 20 Fotos pro Person.".into());
            }
            let file = format!("{face_id}-{id}.{ext}");
            let path = folder(&app, "faces")?.join(&file);
            fs::write(&path, bytes).map_err(error)?;
            face.images.push(file);
            save_faces(&app, &faces)?;
            Ok(path.to_string_lossy().into_owned())
        }
        _ => Err("Unbekannter Upload.".into()),
    }
}

/// Kopiert ein per Drag & Drop abgelegtes Bild als Vorlage in die Mosaic-Daten.
#[tauri::command]
fn import_template(app: tauri::AppHandle, path: String) -> Result<String> {
    let src = PathBuf::from(path);
    let ext = src.extension().and_then(|v| v.to_str()).unwrap_or("").to_ascii_lowercase();
    if !IMAGE_EXTENSIONS.contains(&ext.as_str()) || !src.is_file() {
        return Err("Bitte ein PNG-, JPG- oder WebP-Bild ablegen.".into());
    }
    let dest = folder(&app, "templates")?.join(format!("vorlage-{}.{ext}", unique_id()?));
    fs::copy(&src, &dest).map_err(error)?;
    Ok(dest.to_string_lossy().into_owned())
}

// ---------- Speichern, Ordner öffnen, Bug-Report ----------

fn export_dir(app: &tauri::AppHandle, video: bool) -> Result<PathBuf> {
    let base = if video { app.path().video_dir() } else { app.path().picture_dir() }
        .or_else(|_| app.path().download_dir())
        .map_err(error)?;
    let dir = base.join("Mosaic");
    fs::create_dir_all(&dir).map_err(error)?;
    Ok(dir)
}

/// Kopiert ein Ergebnis in den Bilder- bzw. Filme-Ordner (Unterordner „Mosaic“).
#[tauri::command]
fn export_media(app: tauri::AppHandle, path: String) -> Result<String> {
    let src = inside_data(&app, &path)?;
    let video = src.extension().and_then(|e| e.to_str()) == Some("webm");
    let name = src.file_name().ok_or("Ungültiger Dateiname.")?;
    let dest = export_dir(&app, video)?.join(name);
    fs::copy(&src, &dest).map_err(error)?;
    Ok(dest.to_string_lossy().into_owned())
}

/// Zeigt eine Datei aus den Mosaic-Daten oder den Export-Ordnern im Finder/Explorer.
#[tauri::command]
fn reveal(app: tauri::AppHandle, path: String) -> Result<()> {
    let target = Path::new(&path).canonicalize().map_err(|_| "Datei nicht gefunden.".to_string())?;
    let mut allowed = vec![folder(&app, "")?.canonicalize().map_err(error)?];
    for video in [false, true] {
        if let Ok(dir) = export_dir(&app, video).and_then(|d| d.canonicalize().map_err(error)) {
            allowed.push(dir);
        }
    }
    if !allowed.iter().any(|base| target.starts_with(base)) {
        return Err("Dieser Ort kann nicht geöffnet werden.".into());
    }
    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg("-R").arg(&target).status();
    #[cfg(target_os = "windows")]
    let status = Command::new("explorer").arg(format!("/select,{}", target.display())).status();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let status = Command::new("xdg-open").arg(target.parent().unwrap_or(&target)).status();
    status.map(|_| ()).map_err(error)
}

/// Erstellt einen Fehlerbericht (Textdatei) mit Systeminfo, Modellen und Engine-Protokoll.
#[tauri::command]
fn bug_report(app: tauri::AppHandle, description: String) -> Result<String> {
    let mut text = String::new();
    text.push_str("Mosaic Fehlerbericht\n====================\n\n");
    text.push_str(&format!("Version: {}\n", app.package_info().version));
    text.push_str(&format!("System: {} {}\n", std::env::consts::OS, std::env::consts::ARCH));
    text.push_str(&format!("Profil: {}\n", profile()));
    text.push_str(&format!("Whitelist-Modus: {}\n\n", load_settings(&app)?.whitelist_only));
    text.push_str(&format!("Beschreibung:\n{}\n\n", description.trim()));
    text.push_str("Modelle:\n");
    for e in load_library(&app)? {
        text.push_str(&format!(
            "- {} | {} | {} | {:.2} GB | {} | {}\n",
            e.name, e.role, e.family, e.size as f64 / 1e9,
            if Path::new(&e.path).is_file() { "vorhanden" } else { "FEHLT" },
            if e.approved { "freigegeben" } else { "nicht freigegeben" }
        ));
    }
    text.push_str(&format!("\nPersonen in der Gesichter-Datenbank: {}\n", load_faces(&app)?.len()));
    let log = fs::read_to_string(folder(&app, "logs")?.join("last-run.log")).unwrap_or_else(|_| "(noch kein Lauf)".into());
    let tail: String = log.chars().rev().take(20_000).collect::<Vec<_>>().into_iter().rev().collect();
    text.push_str("\nLetzter Engine-Lauf:\n");
    text.push_str(&tail);
    let path = folder(&app, "reports")?.join(format!("mosaic-fehlerbericht-{}.txt", unique_id()?));
    fs::write(&path, text).map_err(error)?;
    Ok(path.to_string_lossy().into_owned())
}

pub fn run() {
    tauri::Builder::default()
        .manage(Mutex::new(false))
        .invoke_handler(tauri::generate_handler![
            profile, library, import_file, update_entry, remove_entry, download, generate, media,
            faces, faces_dir, face_create, face_confirm, face_delete, face_remove_image, upload, import_template,
            get_settings, set_settings, approve_entry, history, history_clear, export_media, reveal, bug_report
        ])
        .run(tauri::generate_context!())
        .expect("Mosaic konnte nicht starten");
}
