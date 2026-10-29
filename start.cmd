@echo off
cd /d "%~dp0"
if not exist "%~dp0speeching.exe" (
  echo speeching.exe is missing. Build it with cargo build --release --locked.
  pause
  exit /b 1
)
start "" "%~dp0speeching.exe"
