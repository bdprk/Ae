# 平台支持矩阵

状态定义：已验证 = 本机原生运行全部测试与净化器验证通过；受限 = 部分通过；未验证 = 无本机证据。

| 平台 | 架构 | 状态 | 首个验证记录 | C 工具链 | 最低系统 |
|---|---|---|---|---|---|
| Windows | x86-64 | 已验证 | 2026-11-09（v0.1.0 起） | MinGW-W64 GCC 16.2.0（UCRT） | 待定（实测 Windows 10.0.26300） |
| Linux | x86-64 | 已验证 | 2026-11-10 | GCC 14.2.0（Debian 13） | 待定（实测 Debian 13，内核 6.18） |
| Linux（净化器） | x86-64 | 已验证 | 2026-11-10 | GCC `-fsanitize=address,undefined` | 同上 |
| macOS / ARM64 / 交叉编译 | — | 未验证（不在首批承诺内） | — | — | — |

## 验证内容

- `cargo test --locked`：9 单元 + 106 集成测试（Unicode 路径与字符边界、受检算术边界、字符串/向量/映射/枚举的所有权与释放、循环与范围、`?` 传播、哈希表、模块可见性、泛型枚举实例化与预算、缓存命中/损坏/清理）。
- `scripts/verify_generated_c.py [--sanitize]`：生成 C 在两种优化模式输出一致；失败路径（越界、断言、UTF-8 边界、分配失败）按预期非零退出；`--leak` 用分配计数要求每个程序的退出块数等于空程序基线（当前 2 块）；Linux 净化器下无地址/未定义行为报告。
- 全部 `examples/*.ae`（含 `examples/modules/` 的多文件程序）输出与 Windows 完全一致。

### 记录时效

表内的「已验证」记录分别来自 2026-11-09/10 的两次本机运行。此后新增的能力——哈希表 `map[string, i64]`、UTF-8 字符边界与 `utf8_len`/`utf8_prefix`、分配器接口与 `AE_ALLOC_LIMIT`、`pub` 可见性、循环移动规则、泛型枚举与 `Option[T]`——目前只有 **Windows 本机**的完整证据；Linux 侧的记录早于这些改动，需要重跑 `cargo test --locked`、`python3 scripts/verify_generated_c.py --sanitize --leak` 与示例对比后才能重新声明一致。在这之前，本文不把新能力标记为跨平台已验证。

## 已知平台差异（不掩盖）

- 可执行文件命名：Windows `.exe`；Linux 无后缀且需要可执行位（`build` 自动 0755）。
- 产物体积：同一程序 Windows 约 61–64 KB，Linux 约 16.8 KB（链接器与 CRT 差异）。
- 地址/未定义行为净化器：MinGW 无 ASan 运行时，仅 Linux 验证；Windows 依赖测试矩阵与生成 C 审查。
- 路径编码：Windows 内部使用 UTF-8 源码与非 ASCII 路径实测通过（`程序.ae`）；Linux 原生 UTF-8。

## 复现

Linux 结果与原始数据见 `bench/results/2026-11-10-linux-node3/`（Debian 13、GCC 14.2.0、rustc 1.99.0、4 vCPU / 7.8G），Windows 见 `bench/results/2026-11-10-windows-local/`。命令：`cargo test --locked`、`python3 scripts/verify_generated_c.py --sanitize`、`python3 bench/run.py [--release] [--cold]`。
