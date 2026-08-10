@echo off
setlocal
set "PATH=C:\msys64\mingw64\bin;%PATH%"

echo Starting shitview Slint indexer...
cargo run --package shitview-slint -- %*

endlocal
