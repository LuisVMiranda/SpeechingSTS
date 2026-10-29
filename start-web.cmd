@echo off
setlocal
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0web\start-web.ps1" %*
if errorlevel 1 (
    echo.
    if not "%SPEECHING_LAUNCHER_NO_PAUSE%"=="1" pause
    exit /b 1
)
exit /b 0
