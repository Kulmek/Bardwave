@echo off
::==============================================================================
:: SoundShared — Windows build script (build.bat)
::==============================================================================
:: Usage:
::   build.bat                : debug build
::   build.bat release        : release build
::   build.bat test           : run all tests
::   build.bat clean          : cargo clean
::   build.bat fmt            : cargo fmt --check
::   build.bat clippy         : cargo clippy --all-targets
::==============================================================================

setlocal enabledelayedexpansion

:: Project root (directory containing this script)
set "PROJECT_ROOT=%~dp0"
if not defined PROJECT_ROOT set "PROJECT_ROOT=."

:: Default mode: debug
set "BUILD_MODE=debug"
if "%~1"=="release" set "BUILD_MODE=release"
if "%~1"=="rel" set "BUILD_MODE=release"

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

where cmake >nul 2>&1
if errorlevel 1 (
    echo [WARN] cmake not found in PATH. opusic-sys libopus needs CMake to build.
    echo        Trying scoop...
    where scoop >nul 2>&1
    if errorlevel 1 (
        echo [ERROR] cmake is required but neither cmake nor scoop is available.
        echo        Install cmake manually or run: scoop install cmake
        exit /b 1
    ) else (
        echo [INFO] Installing cmake via scoop...
        scoop install cmake
        if errorlevel 1 (
            echo [ERROR] scoop install cmake failed.
            exit /b 1
        )
    )
)

where ninja >nul 2>&1
if errorlevel 1 (
    echo [WARN] ninja not found in PATH. CMake needs Ninja to avoid MSBuild temp-file sandbox blocks.
    echo        Trying scoop...
    where scoop >nul 2>&1
    if errorlevel 1 (
        echo [ERROR] ninja is required but neither ninja nor scoop is available.
        echo        Install ninja manually or run: scoop install ninja
        exit /b 1
    ) else (
        echo [INFO] Installing ninja via scoop...
        scoop install ninja
        if errorlevel 1 (
            echo [ERROR] scoop install ninja failed.
            exit /b 1
        )
    )
)

:: Use Ninja generator so opusic-sys does not spawn MSBuild/cmd.exe,
:: which is blocked in restricted Windows sandboxes.
set "CMAKE_GENERATOR=Ninja"

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
    cargo build --release
) else (
    cargo build
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
