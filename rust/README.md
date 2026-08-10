# shitview Rust prototype

这是 shitview Rust 重构工作区。Python 原型保留为迁移参考，产品主线是 Rust + Slint。

## 工作区

- `crates/shitview-core`：目录模型和基线扫描器；
- `crates/shitview-index`：无 GUI 快照 CLI；
- `crates/shitview-storage`：SQLite/WAL 模式与合成数据基准；
- `apps/shitview-slint`：Slint 原生界面和大规模节点 Phase 0 PoC。

## 运行

在 `H:\shitview\rust` 目录执行：

```powershell
cargo run --package shitview-index -- H:\some\project --output snapshot.json
```

也可以限制本次快照：

```powershell
cargo run --package shitview-index -- H:\some\project --max-nodes 1600 --max-children 180
```

CLI 会输出 JSON 快照，并在标准错误中报告：

- 扫描节点数；
- 目录、文件、符号链接数量；
- 被省略节点数量；
- 扫描异常数量；
- 总耗时。

快照会明确标记 `stats.is_truncated`，不会把部分扫描结果伪装成完整目录。

## 当前边界

- `shitview-core` 仍保持无第三方依赖的稳定 DFS 基线，不跟随符号链接目录；
- 默认忽略 Rust、Python 和常见前端生成目录，包括 `target`；
- `shitview-storage` 已加入 SQLite/WAL schema 和合成数据基准，但还没有接入真实索引器；
- `shitview-slint` 已加入原生窗口和合成节点画布，但还没有接入真实目录数据；
- JSON 序列化边界已经独立，后续可替换为 `serde_json`；
- 增量监听、取消、进度和数据库提交仍属于 Phase 1。

## 测试

```powershell
cargo check --workspace
cargo test --workspace
```

Windows MinGW 环境如果没有把 MSYS2 加入全局 PATH，可以使用：

```powershell
.\run_phase0.bat
```

## Phase 0

运行 Slint 合成图谱：

```powershell
cargo run --package shitview-slint
```

窗口可以切换 1,000、5,000 和 10,000 个合成节点，用于验证 Slint 组件基线、缩放和电路板视觉。它不是最终批量画布，也不能替代真实帧率基准。

运行 SQLite 合成索引基准：

```powershell
cargo run --release --package shitview-storage --bin storage_bench -- 100000
```

## Phase 1 indexer

The native Slint app now opens a real project directory and runs the background indexer.

```powershell
$env:PATH='C:\msys64\mingw64\bin;' + $env:PATH
cargo run --offline --package shitview-slint -- H:\some\project
```

The optional folder argument skips the picker. Without it, use `BROWSE` or enter a path in
the window. The index database is stored outside the project under the platform application
data directory. Project-local `.shitview/ignore` is supported in addition to `.gitignore`.

Phase 1 provides bounded parallel traversal, a serialized SQLite/WAL writer, generation
switching, pause/resume/cancel, resumable queue state, stable file identity, native file
watching, and incremental create/modify/remove updates. Symlinks and junctions are recorded
but never traversed.

The Phase 2 canvas now rasterizes nodes, modules, orthogonal connectors, and the background grid
in Rust into one RGBA image. Slint displays that image as one clipped, zoomable item; module
labels are hidden below 42 percent zoom to keep dense scenes readable. The rasterizer benchmark
currently covers 1K, 5K, and 10K synthetic scenes. Scene preparation runs off the UI thread;
the Windows GUI reported 60 FPS at 1K and 59 FPS at 10K during the real GUI check.

Current debug-build scene preparation times on the development machine are approximately 134 ms
for 1K, 494 ms for 5K, and 1,046 ms for 10K. The 10K GUI working set was approximately 160 MB.
Zoom and scroll render one image item; FPS is measured through Slint's rendering notifier.

Phase 3 adds a uniform-grid hit index over rendered nodes. Click a node to show its path and
selection outline. File nodes expose `OPEN SOURCE`; the app uses `SHITVIEW_EDITOR`, then
`VISUAL`/`EDITOR`, and finally the platform default application.
