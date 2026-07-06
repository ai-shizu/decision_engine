@echo off
setlocal
cd /d "%~dp0"
echo.
echo 注意: target\debug\pkb-desktop.exe を直接ダブルクリックすると黒画面になります。
echo       開発中は dev.cmd を使ってください。
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\run-tauri-dev.ps1"
exit /b %ERRORLEVEL%
