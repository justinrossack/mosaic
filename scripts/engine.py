"""Build the bundled native executable on the target operating system."""
from pathlib import Path
from shutil import copy2
import platform
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
TAG = "master-1106-532d2c1"
SPIRV_TAG = "vulkan-sdk-1.4.309.0"  # gleiche Version wie das Vulkan SDK im Workflow
windows = platform.system() == "Windows"
dest = ROOT / "src-tauri" / "engine"
dest.mkdir(exist_ok=True)
with tempfile.TemporaryDirectory() as tmp:
    src = Path(tmp) / "sd"
    build = Path(tmp) / "build"
    subprocess.run(["git", "clone", "--branch", TAG, "--depth", "1",
                    "--recursive", "--shallow-submodules",
                    "https://github.com/leejet/stable-diffusion.cpp.git", str(src)], check=True)
    backend = "-DSD_VULKAN=ON" if windows else "-DSD_METAL=ON"
    extra = []
    if windows:
        # ggml-vulkan verlangt SPIRV-Headers als CMake-Paket; das Vulkan SDK
        # der CI liefert es nicht mit. Passend zur SDK-Version nachinstallieren.
        spirv_src = Path(tmp) / "spirv-headers"
        spirv_prefix = Path(tmp) / "spirv-prefix"
        subprocess.run(["git", "clone", "--branch", SPIRV_TAG, "--depth", "1",
                        "https://github.com/KhronosGroup/SPIRV-Headers.git", str(spirv_src)], check=True)
        subprocess.run(["cmake", "-S", str(spirv_src), "-B", str(spirv_src / "build"),
                        "-DSPIRV_HEADERS_ENABLE_TESTS=OFF", "-DSPIRV_HEADERS_ENABLE_INSTALL=ON",
                        f"-DCMAKE_INSTALL_PREFIX={spirv_prefix}"], check=True)
        subprocess.run(["cmake", "--install", str(spirv_src / "build")], check=True)
        extra.append(f"-DCMAKE_PREFIX_PATH={spirv_prefix.as_posix()}")
    subprocess.run(["cmake", "-S", str(src), "-B", str(build), backend,
                    "-DCMAKE_BUILD_TYPE=Release", "-DBUILD_SHARED_LIBS=OFF",
                    "-DSD_BUILD_SHARED_LIBS=OFF", "-DGGML_NATIVE=OFF",
                    "-DGGML_METAL_EMBED_LIBRARY=ON", *extra],check=True)
    subprocess.run(["cmake", "--build", str(build), "--config", "Release",
                    "--parallel", "3", "--target", "sd-cli"],check=True)
    exe = next(build.rglob("sd-cli.exe" if windows else "sd-cli"))
    copy2(exe, dest / exe.name)
    for dll in exe.parent.glob("*.dll"):
        copy2(dll, dest / dll.name)
    copy2(src / "LICENSE", dest / "stable-diffusion-LICENSE.txt")
    for license_file in src.rglob("LICENSE"):
        if license_file != src / "LICENSE":
            copy2(license_file, dest / ("license-" + "-".join(license_file.relative_to(src).parts) + ".txt"))
