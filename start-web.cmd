@echo off
setlocal
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0web\start-web.ps1" %*
exit /b %errorlevel%
