@echo off
setlocal
set "PATH=C:\msys64\mingw64\bin;%PATH%"

echo Starting shitview Slint Phase 0...
cargo run --package shitview-slint

endlocal
