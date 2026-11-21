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
const ROLES: [(&str, &str); 7] = [
    ("checkpoint", "-m"),
    ("diffusion", "--diffusion-model"),
    ("vae", "--vae"),
    ("clip_l", "--clip_l"),
    ("clip_g", "--clip_g"),
    ("t5xxl", "--t5xxl"),
    ("llm", "--llm"),
];
const EXTENSIONS: [&str; 4] = ["safetensors", "gguf", "ckpt", "sft"];

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
    entries.retain(|e| e.path != entry.path);
    entries.push(entry.clone());
    save_library(app, &entries)?;
    Ok(entry)
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

#[derive(Deserialize)]
struct GenerateRequest {
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
}

#[tauri::command]
async fn generate(app: tauri::AppHandle, device: String, request: GenerateRequest) -> Result<String> {
    if device != profile() {
        return Err("Bitte das Profil für dieses Betriebssystem auswählen.".into());
    }
    let r = request;
    if r.prompt.trim().is_empty() || r.prompt.len() > 8000 || r.negative.len() > 8000 {
        return Err("Bitte einen Prompt mit höchstens 8000 Bytes eingeben.".into());
    }
    let sizes_ok = |v: u32| (256..=2048).contains(&v) && v % 64 == 0;
    if !sizes_ok(r.width) || !sizes_ok(r.height) || !(1..=150).contains(&r.steps) || !(0.0..=30.0).contains(&r.cfg) {
        return Err("Ungültige Einstellungen für Größe, Schritte oder CFG.".into());
    }
    if !["euler", "euler_a", "dpm++2m", "dpm++2s_a", "lcm"].contains(&r.sampler.as_str()) {
        return Err("Unbekanntes Sampling-Verfahren.".into());
    }
    let entries = load_library(&app)?;
    let find = |id: &str| -> Result<&Entry> {
        let entry = entries.iter().find(|e| e.id == id).ok_or("Modell nicht gefunden. Bitte erneut auswählen.")?;
        if !Path::new(&entry.path).is_file() {
            return Err(format!("Datei fehlt: {}. Wurde sie verschoben oder gelöscht?", entry.path));
        }
        Ok(entry)
    };
    let mut args: Vec<String> = Vec::new();
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
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(error)?
        .as_nanos();
    let out = folder(&app, "images")?.join(format!("mosaic-{id}.png"));
    args.extend([
        "-p".into(), r.prompt.clone(),
        "-W".into(), r.width.to_string(),
        "-H".into(), r.height.to_string(),
        "--steps".into(), r.steps.to_string(),
        "--cfg-scale".into(), r.cfg.to_string(),
        "--sampling-method".into(), r.sampler.clone(),
        "-s".into(), r.seed.to_string(),
    ]);
    if !r.negative.trim().is_empty() {
        args.extend(["-n".into(), r.negative.clone()]);
    }
    if r.offload {
        args.push("--offload-to-cpu".into());
    }
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

#[tauri::command]
async fn preview(app: tauri::AppHandle, name: String) -> Result<tauri::ipc::Response> {
    let path = Path::new(&name);
    if path.parent() != Some(folder(&app, "images")?.as_path())
        || path.extension().and_then(|v| v.to_str()) != Some("png")
    {
        return Err("Ungültiger Bildpfad.".into());
    }
    Ok(tauri::ipc::Response::new(fs::read(path).map_err(error)?))
}

pub fn run() {
    tauri::Builder::default()
        .manage(Mutex::new(false))
        .invoke_handler(tauri::generate_handler![
            profile, library, import_file, update_entry, remove_entry, download, generate, preview
        ])
        .run(tauri::generate_context!())
        .expect("Mosaic konnte nicht starten");
}
