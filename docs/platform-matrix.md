# 平台支持矩阵

状态定义：已验证 = 本机原生运行全部测试与生成 C 校验通过；受限 = 部分通过；未验证 = 无本机证据。

| 平台 | 架构 | 状态 | 最近验证 | C 工具链 | 最低系统 |
|---|---|---|---|---|---|
| Windows | x86-64 | 已验证 | 2026-11-10（v0.1.0 起） | MinGW-W64 GCC 16.2.0（UCRT） | 待定（实测 Windows 10.0.26300） |
| Linux | x86-64 | 已验证 | 2026-11-10 | GCC 14.2.0（Debian 13） | 待定（实测 Debian 13，内核 6.18） |
| Linux（净化器） | x86-64 | 已验证 | 2026-11-10 | GCC `-fsanitize=address,undefined` | 同上 |
| macOS / ARM64 / 交叉编译 | — | 未验证（不在首批承诺内） | — | — | — |

## 验证内容

- `cargo test --locked`：9 单元 + 129 集成测试（Unicode 路径与字符边界、受检算术边界、字符串/向量/映射/枚举的所有权与释放、循环与范围、`?` 传播（结果枚举与 `Option`）、哈希表、模块可见性、泛型枚举/泛型结构体/泛型函数实例化及预算、类型参数约束与布尔相等、泛型借用参数 `&T` 与只读借用转发、`find` 字节下标结果值、排序/时钟/环境变量/追加、`exit` 状态码、标准输入与工作目录（含 `set_cwd`）、JSON 路径查询、`vec[string]` 借用元素拷贝、子进程状态与输出、缓存命中/损坏/清理/大小上限）。
- `scripts/verify_generated_c.py [--sanitize] [--leak]`：生成 C 在两种优化模式输出一致；失败路径（越界、断言、UTF-8 边界、分配失败）按预期非零退出；`--leak` 用分配计数要求每个程序的退出块数等于空程序基线（**Windows 2 块，Linux 0 块**——glibc 在 `main` 之前不保留堆分配，UCRT 会保留 2 块）；Linux 净化器下无地址/未定义行为报告。
- 全部 `examples/*.ae`（含 `examples/modules/` 的多文件程序）输出在两平台**按行尾归一化后**逐字节一致：Linux 为 LF，Windows 因 stdout 处于文本模式而为 CRLF；这不是原始字节一致，见「已知平台差异」。此前写成「逐字节一致」不准确，已修正。集成测试与示例比对都先把 `\r\n` 归一化为 `\n`。

### 记录时效

2026-11-10 的 Linux 运行覆盖了**到当时为止**的全部能力：第一阶段（InstaBox `evaluate-2`）记录的是性能基准，第二阶段（`evaluate-1`，见 `bench/results/2026-11-10-linux-evaluate-1/`）在新增哈希表、UTF-8 边界、分配器接口、`pub` 可见性、循环移动规则、泛型枚举、排序/时钟/环境变量/追加写入、`exit`、`read_stdin`/`cwd`、JSON 路径查询、缓存校验与上限、`strvec_new`/`run_command`、泛型结构体与泛型函数、`set_cwd`、类型参数约束之后重跑，9 + 126 全部通过、20 个生成 C 用例全部 PASS 且分配计数为 0。之后（`node`，见 `bench/results/2026-11-10-linux-node-option/`）再加入 `?` 对 `Option` 的传播，9 + 127 全部通过、21 个生成 C 用例 PASS，净化器与分配计数同样干净。第一阶段暴露并修复了一个**只在 Linux 出现**的缺陷：`clock_gettime` 属于 POSIX，在 `-std=c11 -pedantic-errors` 下被 glibc 隐藏，导致 91 个测试无法编译；改用 ISO C11 的 `timespec_get` 后消失（见 ADR-25）。

此后任何新增能力在提交前都要求两平台各跑一次上述命令；只有一方通过时，本文不会把它标记为跨平台已验证。

2026-11-10 在 Windows 本地与 Linux（instabox-node）各跑一次全量验证，覆盖 `?` 对 `Option` 的传播：9 单元 + 127 集成测试全部通过，21 个生成 C 用例在两种优化模式下 PASS，Linux 净化器无地址/未定义行为报告、`--leak` 的退出分配计数为 0（Windows 为 2 块基线）。原始输出见 `bench/results/2026-11-10-linux-node-option/`。

2026-11-10 本轮（ADR-35/36）先在 Windows 本地验证：`&T` 泛型借用参数、只读借用参数转发、`find(text, needle) -> IntResult`。`cargo test --locked` 为 9 单元 + 129 集成测试全部通过；`python scripts/verify_generated_c.py --leak` 共 23 个用例全部 PASS，每个程序的退出分配块数都等于 Windows 空程序基线（2 块），含 `find` 的 `Err` 分支与 `generic_borrow.ae` 的借用路径。

**同日 W1 已在 Linux 回填完成**（`bench/results/2026-11-10-linux-borrow-find/`，Debian 13 / GCC 14.2.0 / rustc 1.99.0）：`cargo test --locked` 9 单元 + 129 集成全部通过；`--sanitize` 23 个用例 PASS 且无 ASan/UBSan 报告；`--leak` 23 个用例 PASS、退出分配块数均为 glibc 基线 0；全部 `examples/*.ae` 通过 `check`；两个新示例与 Windows 输出按行尾归一化后一致。因此这三项能力现为**双平台验证**，不再是单平台原型；净化器只在 Linux 跑过（MinGW 无 ASan 运行时），这一限制不变。

## 已知平台差异（不掩盖）

- 可执行文件命名：Windows `.exe`；Linux 无后缀且需要可执行位（`build` 自动 0755）。
- 产物体积：同一程序 Windows 约 61–64 KB，Linux 约 16.8 KB（链接器与 CRT 差异）。
- 地址/未定义行为净化器：MinGW 无 ASan 运行时，仅 Linux 验证；Windows 依赖测试矩阵与生成 C 审查。
- 分配计数基线：Windows 2 块、Linux 0 块，比较时各自减去自己的基线。
- 路径编码：Windows 内部使用 UTF-8 源码与非 ASCII 路径实测通过（`程序.ae`）；Linux 原生 UTF-8。
- 严格 ISO C：生成文件只用 C11 标准接口，不注入 `_POSIX_C_SOURCE` 之类的特性宏，因此同一份 C 在 glibc、musl 与 `-std=c11 -pedantic-errors` 下都能编译；代价是不能使用 POSIX 专有接口（时间只能走 `timespec_get`，拿不到单调时钟）。
- 标准输入的换行：两平台都按二进制读入，`\r\n` 不会被翻译；差异只存在于用户自己传入的文本里。
- 标准输出的换行：生成的 C 用 `fputs`/`printf`/`fputc('\n')` 写 stdout，Windows 文本模式把每个 `\n` 翻成 `\r\n`，Linux 保持 `\n`。因此跨平台比较必须先归一化行尾；Rust 集成测试与示例比对都这么做。这是 CRT 差异，不是生成代码里的分支。
- 子进程：`run_command` 在两平台都**不**经过 shell（Windows 直接 `CreateProcessW`，POSIX 直接 `execvp`），所以 `echo`、`dir` 这类内建命令要显式写成 `cmd` 加 `/c` 的形式。被信号杀死时 POSIX 报 `128 + 信号号`，Windows 直接给退出码——这是唯一有意保留的语义差异。

## 复现

Linux 结果与原始数据见 `bench/results/2026-11-10-linux-evaluate-1/`（Debian 13、GCC 14.2.0、rustc 1.99.0、4 vCPU / 7.8 GB：`PLATFORM.txt`、`cargo-test.log`、`verify-c.log`）与 `bench/results/2026-11-10-linux-node-option/`（同机型的 `instabox-node`，含 `verify-c-sanitize.log` 与 `verify-c-leak.log`），性能基准见 `bench/results/2026-11-10-linux-node3/`（InstaBox `evaluate-2`），Windows 见 `bench/results/2026-11-10-windows-local/`，本轮 `&T`/`find` 的 Windows 验证日志见 `bench/results/2026-11-10-windows-borrow-find/`，同一提交树的 Linux 回填见 `bench/results/2026-11-10-linux-borrow-find/`（两处均有 `PLATFORM.txt`、`cargo-test.log`，Windows 另有 `verify-c-leak.log`，Linux 另有 `verify-c-sanitize.log` 与 `verify-c-leak.log`）。命令：`cargo test --locked`、`python3 scripts/verify_generated_c.py --sanitize`、`python3 scripts/verify_generated_c.py --leak`、`python3 bench/run.py [--release] [--cold]`。
