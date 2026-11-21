# Mosaic

Lokales Bildstudio für Windows x64 mit Intel Arc (Vulkan) und macOS auf Apple Silicon (Metal). Die Installationspakete enthalten die Bild-Engine aus stable-diffusion.cpp. Modelle werden separat geladen. Die Pakete sind unsigniert.

1. Mosaic öffnen und das Profil für das eigene Gerät auswählen.
2. Eine öffentliche Hugging-Face-Modell-ID oder einen Dateilink einfügen. Unterstützt werden vollständige SD-1.5- und SDXL-Checkpoints als einzelne Safetensors-Datei. Bei mehreren Dateien den Link zum gewünschten Checkpoint verwenden. Mehrteilige FLUX-Modelle und zugangsbeschränkte Downloads werden in dieser Version nicht unterstützt.
3. Modell herunterladen, Prompt eingeben und **Bild erstellen** wählen. Das Studio zeigt das PNG und seinen lokalen Speicherort. Bereits geladene Modelle funktionieren offline.

## Bauen

Node 22, Rust, CMake, Git, Python 3 und die nativen Tauri-Buildwerkzeuge installieren. Unter Windows wird zusätzlich das Vulkan SDK benötigt. Auf dem Mac auf einem arm64-System bauen.

    npm ci
    python scripts/engine.py
    npm run tauri dev

Der Workflow im Repository baut bei einem Versions-Tag (z. B. v0.1.0) den Windows-Installer und das macOS-Disk-Image auf nativen Runnern und legt beide im selben GitHub Release ab. Ein manueller Workflow-Lauf liefert beide Pakete als Actions-Artefakte.
