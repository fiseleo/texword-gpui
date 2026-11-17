@echo off
setlocal
set "REL_EXE=%LOCALAPPDATA%\..\.cache\texword-gpui\target\release\texword_app.exe"
if exist "%USERPROFILE%\.cache\texword-gpui\target\release\texword_app.exe" (
  start "" "%USERPROFILE%\.cache\texword-gpui\target\release\texword_app.exe" --project "z:\書\書稿"
) else (
  cd /d "%~dp0"
  cargo run --release -- --project "z:\書\書稿"
)
