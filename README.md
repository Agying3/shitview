[中文](README.md) | [English](README.en.md)

# shitview

动态项目文件夹地图。长期产品路线基于 Rust + Slint；Python + PySide6 实现作为迁移参考保留。

## Rust Phase 0（阶段零）

在 Windows 环境下，进入 `rust/` 目录并使用 MSYS2 MinGW：

```powershell
.\run_phase0.bat
```

当前 Slint 窗口是一个合成的渲染基准（分别渲染 1,000 / 5,000 / 10,000 个节点）。它尚未索引真实文件夹。

## Python 参考运行

```powershell
python -m shitview.main H:\some\project
```

或者安装包之后：

```powershell
shitview H:\some\project
```

## 作为插件复用

```python
from shitview import analyze_folder, open_shitview, summarize_folder

summary = summarize_folder(r"H:\some\project")
print(summary)

analysis = analyze_folder(r"H:\some\project")
print(analysis.file_count, analysis.directory_count, analysis.leaf_count)

open_shitview(r"H:\some\project")
```

分层结构：

- `core`：数据模型、差异比对、标签、图布局
- `services`：扫描、监听、编排
- `ui`：Qt 控件与渲染
- `plugin`：供脚本、工具及其他文件夹复用的入口
