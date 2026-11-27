@echo off
setlocal
set "ROOT=%~dp0"
if exist "%ROOT%target\release\VIctory-RTX.exe" (
    start "" "%ROOT%target\release\VIctory-RTX.exe"
    exit /b 0
)
if exist "%ROOT%VIctory-RTX.exe" (
    start "" "%ROOT%VIctory-RTX.exe"
    exit /b 0
)
echo VIctory-RTX.exe is not built yet.
echo Build both the runtime and the desktop launcher once:
echo   cargo build --locked --release -p victory -p victory-launcher
pause
exit /b 1
