@echo off
setlocal
echo === Mosaic Windows-Installer (.exe) erstellen ===
echo.
where node >nul 2>nul || (echo FEHLER: Node.js 22 wurde nicht gefunden. Bitte installieren. & pause & exit /b 1)
where cargo >nul 2>nul || (echo FEHLER: Rust / Cargo wurde nicht gefunden. Bitte rustup installieren. & pause & exit /b 1)
where cmake >nul 2>nul || (echo FEHLER: CMake wurde nicht gefunden. Bitte installieren. & pause & exit /b 1)
where python >nul 2>nul || (echo FEHLER: Python 3 wurde nicht gefunden. Bitte installieren. & pause & exit /b 1)
if not defined VULKAN_SDK (echo FEHLER: Vulkan SDK wurde nicht gefunden. Bitte von https://vulkan.lunarg.com installieren. & pause & exit /b 1)

echo [1/3] NPM-Abhaengigkeiten installieren...
call npm ci || exit /b 1

echo [2/3] Bild-Engine (sd-cli.exe mit Vulkan) kompilieren...
python scripts\engine.py || exit /b 1

echo [3/3] Windows-Installer (.exe) mit Tauri bauen...
call npm run tauri build -- --bundles nsis || exit /b 1

echo.
echo FERTIG! Der fertige Windows-Installer liegt unter:
echo src-tauri\target\release\bundle\nsis\
pause
