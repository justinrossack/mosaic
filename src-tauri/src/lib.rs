use std::{
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

#[tauri::command]
fn profile() -> &'static str {
    if cfg!(target_os = "windows") {
        "arc"
    } else {
        "metal"
    }
}

#[tauri::command]
fn models(app: tauri::AppHandle) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for item in fs::read_dir(folder(&app, "models")?).map_err(error)? {
        let entry = item.map_err(error)?;
        if entry.file_type().map_err(error)?.is_file()
            && entry.path().extension().and_then(|s| s.to_str()) == Some("safetensors")
        {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    Ok(names)
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
async fn download(app: tauri::AppHandle, source: String) -> Result<String> {
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
    let (revision, file) = if parts.len() == 2 {
        let response = client
            .get(format!("https://huggingface.co/api/models/{repo}"))
            .send()
            .await
            .map_err(error)?
            .error_for_status()
            .map_err(error)?;
        let info: serde_json::Value = response.json().await.map_err(error)?;
        let files = info["siblings"]
            .as_array()
            .ok_or("Keine Modelldateien gefunden.")?;
        let options: Vec<_> = files
            .iter()
            .filter_map(|v| v["rfilename"].as_str())
            .filter(|s| valid_part(s) && s.ends_with(".safetensors"))
            .collect();
        if options.len() != 1 {
            return Err("Bitte auf Hugging Face unter Files den Link zu einem vollständigen SD-1.5- oder SDXL-Checkpoint (.safetensors) kopieren.".into());
        }
        ("main".to_owned(), options[0].to_string())
    } else {
        if parts.len() < 5 || !matches!(parts[2], "blob" | "resolve") {
            return Err("Unbekannter Hugging-Face-Dateilink.".into());
        }
        (parts[3].to_owned(), parts[4..].join("/"))
    };
    if !file.ends_with(".safetensors") {
        return Err("Dieses Studio lädt vollständige .safetensors-Checkpoints.".into());
    }
    let url = Url::parse(&format!(
        "https://huggingface.co/{repo}/resolve/{revision}/{file}"
    ))
    .map_err(error)?;
    // A stable filename keeps revisions and similarly named repositories separate.
    let mut hash: u64 = 14695981039346656037;
    for b in url.as_str().bytes() {
        hash = (hash ^ u64::from(b)).wrapping_mul(1099511628211);
    }
    let name = format!(
        "{hash:016x}-{}",
        file.rsplit('/').next().unwrap_or("model.safetensors")
    );
    let dest = folder(&app, "models")?.join(&name);
    if dest.is_file() {
        return Ok(name);
    }
    let mut r = client
        .get(url)
        .send()
        .await
        .map_err(error)?
        .error_for_status()
        .map_err(error)?;
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
                let _ = app.emit("progress", (received, total));
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
    Ok(name)
}

#[tauri::command]
async fn generate(
    app: tauri::AppHandle,
    device: String,
    model: String,
    prompt: String,
) -> Result<String> {
    if device != profile() {
        return Err("Bitte das Profil für dieses Betriebssystem auswählen.".into());
    }
    if prompt.trim().is_empty() || prompt.len() > 8000 {
        return Err("Bitte einen Prompt mit höchstens 8000 Bytes eingeben.".into());
    }
    if !models(app.clone())?.contains(&model) {
        return Err("Bitte zuerst das Modell laden.".into());
    }
    let input = folder(&app, "models")?.join(model);
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(error)?
        .as_nanos();
    let out = folder(&app, "images")?.join(format!("mosaic-{id}.png"));
    let exe = app
        .path()
        .resource_dir()
        .map_err(error)?
        .join("engine")
        .join(if cfg!(windows) {
            "sd-cli.exe"
        } else {
            "sd-cli"
        });
    let guard = app.state::<Mutex<bool>>();
    {
        let mut busy = guard.lock().map_err(error)?;
        if *busy {
            return Err("Eine Generierung läuft bereits.".into());
        }
        *busy = true;
    }
    let result=tauri::async_runtime::spawn_blocking(move || {
        let mut cmd=Command::new(&exe);
        #[cfg(windows)]
        {use std::os::windows::process::CommandExt; cmd.creation_flags(0x08000000);}
        let result=cmd.current_dir(exe.parent().ok_or("Engine-Pfad ungültig.")?).arg("-m").arg(input).args(["-p",&prompt,"-W","512","-H","512","--steps","20","-o"]).arg(&out).output().map_err(|e| format!("Bild-Engine konnte nicht starten ({e})."))?;
        if !result.status.success() || !out.is_file(){
            let log=String::from_utf8_lossy(&result.stderr);
            return Err(format!("Generierung fehlgeschlagen. Modellformat, Grafiktreiber und freien Speicher prüfen. {}",log.lines().rev().take(2).collect::<Vec<_>>().join(" ")));
        }
        Ok(out.to_string_lossy().into_owned())
    }).await;
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
            profile, models, download, generate, preview
        ])
        .run(tauri::generate_context!())
        .expect("Mosaic konnte nicht starten");
}
