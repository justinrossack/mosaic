# Mosaic

Lokales Bildstudio für Windows x64 mit Intel Arc (Vulkan) und macOS auf Apple Silicon (Metal). Die Installationspakete enthalten die Bild-Engine aus [stable-diffusion.cpp](https://github.com/leejet/stable-diffusion.cpp). Modelle werden separat geladen oder importiert. Die Pakete sind unsigniert.

## Download

Unter **[Releases](../../releases/latest)**:

- **Windows:** `Mosaic_…_x64-setup.exe`
- **macOS (Apple Silicon):** `Mosaic_…_aarch64.dmg`. Beim ersten Start per Rechtsklick → „Öffnen“ starten, weil die App nicht signiert ist.

## Bild und Video erstellen

Oben gibt es zwei Bereiche: **Bild erstellen** und **Video erstellen**. Beide haben dieselben drei Eingaben:

- **Vorlage / Startbild:** ein eigenes Bild hochladen oder ins Fenster ziehen.
  - Bild: Mit FLUX.3 Klein (und anderen Modellen mit Referenzbildern) wird die Vorlage als **Referenz** genutzt (Inhalt, Stil, Komposition). Alternativ als **Ausgangsbild**, das mit einstellbarer Stärke abgewandelt wird – das funktioniert mit allen Bildmodellen.
  - Video: Das Video beginnt mit diesem Bild.
- **Beschreibung:** Prompt und optional ein negativer Prompt.
- **Gesichter:** Personen aus der eigenen Gesichter-Datenbank auswählen. Ihre Fotos gehen als Referenzbilder ins Modell (FLUX.3 Klein). Für Videos erzeugt Mosaic zuerst mit FLUX.3 Klein ein Startbild mit den Gesichtern und animiert es danach mit dem Videomodell.

Insgesamt sind bis zu 10 Referenzbilder möglich (Vorlage + bis zu 3 Fotos pro Person).

## Gesichter-Datenbank

Im Bereich **Gesichter** Personen mit Namen anlegen und eigene Fotos hochladen (PNG, JPG, WebP; am besten 1–3 gut beleuchtete Porträts). Die Fotos liegen nur im lokalen App-Datenordner und lassen sich einzeln oder samt Person löschen.

## Modelle

**Ein Klick:**

- **Bild: FLUX.3 Klein** (ca. 6,4 GB): Diffusionsmodell (GGUF Q4), Qwen3-VL-Text-Encoder und VAE. Kann Vorlagen und Gesichter als Referenz nutzen.
- **Video: Wan 2.2 5B** (ca. 8 GB): Wan 2.2 TI2V 5B (GGUF Q4), UMT5-Text-Encoder und Wan-2.2-VAE. Text-zu-Video und Bild-zu-Video, Ausgabe als WebM.

**Eigene Modelle:** Dateien (`.safetensors`, `.gguf`, `.ckpt`, `.sft`) ins Fenster ziehen oder einen lokalen Pfad bzw. Hugging-Face-Dateilink eintragen. Importierte Dateien bleiben an ihrem Ort, Mosaic merkt sich nur den Pfad. Für jede Datei werden **Art** und **Familie** festgelegt; Mosaic rät beides aus dem Dateinamen, in der Bibliothek lässt es sich ändern.

| Familie | Hauptdatei | Zubehör |
| --- | --- | --- |
| SD 1.5 / SDXL | Checkpoint | VAE (optional) |
| FLUX.1 | Diffusionsmodell | VAE, CLIP-L, T5-XXL |
| FLUX.3 Klein | Diffusionsmodell | VAE, LLM (Qwen3-VL 4B) |
| Andere (z. B. Z-Image, Qwen Image, SD 3.5) | Checkpoint oder Diffusionsmodell | frei wählbar |
| Wan 2.2 TI2V 5B (Video) | Diffusionsmodell | VAE (Wan 2.2), T5-XXL/UMT5 |
| Andere Videomodelle (Wan 2.1, Wan 2.2 14B) | Diffusionsmodell | VAE, UMT5, optional CLIP Vision / High-Noise-Modell |

Welche Modelle die Engine kennt, steht in der [Modellübersicht von stable-diffusion.cpp](https://github.com/leejet/stable-diffusion.cpp/tree/master/docs/models). Zugangsbeschränkte Hugging-Face-Repos lassen sich nicht direkt laden: Datei im Browser herunterladen und dann importieren.

## Bauen

Node 22, Rust, CMake, Git, Python 3 und die nativen Tauri-Buildwerkzeuge installieren. Unter Windows wird zusätzlich das Vulkan SDK benötigt. Auf dem Mac auf einem arm64-System bauen.

    npm ci
    python scripts/engine.py
    npm run tauri dev

## Release

Ein Versions-Tag startet den Workflow `.github/workflows/release.yml`. Er baut den Windows-Installer und das macOS-Disk-Image auf nativen Runnern und legt beide im selben GitHub-Release ab:

    git tag v0.3.0
    git push origin v0.3.0
