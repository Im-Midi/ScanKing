@echo off
rem ScanKing one-shot APK build (double-click me)
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "scripts\build_apk.ps1" %*
echo.
echo Log: scripts\build.log
pause
