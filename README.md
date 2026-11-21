# Mosaic

Lokales Bildstudio für Windows x64 mit Intel Arc (Vulkan) und macOS auf Apple Silicon (Metal). Die Installationspakete enthalten die Bild-Engine aus [stable-diffusion.cpp](https://github.com/leejet/stable-diffusion.cpp). Modelle werden separat geladen oder importiert. Die Pakete sind unsigniert.

## Download

Unter **[Releases](../../releases/latest)**:

- **Windows:** `Mosaic_…_x64-setup.exe`
- **macOS (Apple Silicon):** `Mosaic_…_aarch64.dmg`. Beim ersten Start per Rechtsklick → „Öffnen“ starten, weil die App nicht signiert ist.

## Modelle

**Ein Klick:** „FLUX.3 Klein laden“ holt die kleinste FLUX-Variante (ca. 6,4 GB, frei zugänglich): Diffusionsmodell (GGUF Q4), Qwen3-VL-Text-Encoder und VAE.

**Eigene Modelle:** Dateien (`.safetensors`, `.gguf`, `.ckpt`, `.sft`) ins Fenster ziehen oder einen lokalen Pfad bzw. Hugging-Face-Dateilink eintragen. Importierte Dateien bleiben an ihrem Ort, Mosaic merkt sich nur den Pfad. Für jede Datei werden **Art** und **Familie** festgelegt; Mosaic rät beides aus dem Dateinamen, in der Bibliothek lässt es sich ändern.

| Familie | Hauptdatei | Zubehör |
| --- | --- | --- |
| SD 1.5 / SDXL | Checkpoint | VAE (optional) |
| FLUX.1 | Diffusionsmodell | VAE, CLIP-L, T5-XXL |
| FLUX.3 Klein | Diffusionsmodell | VAE, LLM (Qwen3-VL 4B) |
| Andere (z. B. Z-Image, Qwen Image, SD 3.5) | Checkpoint oder Diffusionsmodell | frei wählbar |

Welche Modelle die Engine kennt, steht in der [Modellübersicht von stable-diffusion.cpp](https://github.com/leejet/stable-diffusion.cpp/tree/master/docs/models). Zugangsbeschränkte Hugging-Face-Repos lassen sich nicht direkt laden: Datei im Browser herunterladen und dann importieren.

## Bauen

Node 22, Rust, CMake, Git, Python 3 und die nativen Tauri-Buildwerkzeuge installieren. Unter Windows wird zusätzlich das Vulkan SDK benötigt. Auf dem Mac auf einem arm64-System bauen.

    npm ci
    python scripts/engine.py
    npm run tauri dev

## Release

Ein Versions-Tag startet den Workflow `.github/workflows/release.yml`. Er baut den Windows-Installer und das macOS-Disk-Image auf nativen Runnern und legt beide im selben GitHub-Release ab:

    git tag v0.2.0
    git push origin v0.2.0
