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
