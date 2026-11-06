@echo off
::==============================================================================
:: SoundShared — Windows build script (build.bat)
::==============================================================================
:: Usage:
::   build.bat                : debug build (Opus codec disabled)
::   build.bat release        : release build (Opus codec disabled)
::   build.bat opus           : debug build WITH Opus codec (pure Rust, no CMake/C compiler needed)
::   build.bat opus-release   : release build WITH Opus codec (pure Rust, no CMake/C compiler needed)
::   build.bat test           : run all tests
::   build.bat clean          : cargo clean
::   build.bat fmt            : cargo fmt --check
::   build.bat clippy         : cargo clippy --all-targets
::==============================================================================

setlocal enabledelayedexpansion

:: Project root (directory containing this script)
set "PROJECT_ROOT=%~dp0"
if not defined PROJECT_ROOT set "PROJECT_ROOT=."

:: Default mode: debug, Opus disabled
set "BUILD_MODE=debug"
set "OPUS_FEATURE="
if "%~1"=="release" set "BUILD_MODE=release"
if "%~1"=="rel" set "BUILD_MODE=release"
if "%~1"=="opus" set "BUILD_MODE=debug"&set "OPUS_FEATURE=--features opus-codec"
if "%~1"=="opus-release" set "BUILD_MODE=release"&set "OPUS_FEATURE=--features opus-codec"
if "%~1"=="opus-rel" set "BUILD_MODE=release"&set "OPUS_FEATURE=--features opus-codec"

::----------------------------------------------------------------------
:: 0. Prerequisites
::----------------------------------------------------------------------
:check_rust
where cargo >nul 2>&1
if errorlevel 1 (
    echo [ERROR] cargo not found in PATH. Install Rust from https://rustup.rs/
    echo         then re-run this script.
    exit /b 1
)

::----------------------------------------------------------------------
:: 1. Commands
::----------------------------------------------------------------------
if "%~1"=="clean" goto :do_clean
if "%~1"=="fmt"   goto :do_fmt
if "%~1"=="clippy" goto :do_clippy
if "%~1"=="test" goto :do_test

::----------------------------------------------------------------------
:: Default: build
::----------------------------------------------------------------------
:do_build
echo [INFO] Building SoundShared %BUILD_MODE%...
cd /d "%PROJECT_ROOT%"

if "%BUILD_MODE%"=="release" (
    cargo build --release %OPUS_FEATURE%
) else (
    cargo build %OPUS_FEATURE%
)

if errorlevel 1 (
    echo [ERROR] Build failed.
    exit /b 1
)

echo [INFO] Build complete.
if "%BUILD_MODE%"=="release" (
    echo [INFO] Binary: target\release\soundshared.exe
) else (
    echo [INFO] Binary: target\debug\soundshared.exe
)
exit /b 0

::----------------------------------------------------------------------
:do_clean
echo [INFO] Cleaning target directory...
cd /d "%PROJECT_ROOT%"
cargo clean
exit /b 0

::----------------------------------------------------------------------
:do_fmt
echo [INFO] Checking formatting...
cd /d "%PROJECT_ROOT%"
cargo fmt --check
if errorlevel 1 (
    echo [WARN] Formatting issues found. Run `cargo fmt` to fix.
    exit /b 1
)
echo [INFO] Formatting OK.
exit /b 0

::----------------------------------------------------------------------
:do_clippy
echo [INFO] Running clippy...
cd /d "%PROJECT_ROOT%"
cargo clippy --all-targets
if errorlevel 1 (
    echo [ERROR] Clippy found issues.
    exit /b 1
)
echo [INFO] Clippy OK.
exit /b 0

::----------------------------------------------------------------------
:do_test
echo [INFO] Running tests...
cd /d "%PROJECT_ROOT%"
cargo test --all-targets
if errorlevel  1 (
    echo [ERROR] Tests failed.
    exit /b 1
)
echo [INFO] All tests passed.
exit /b 0

endlocal
