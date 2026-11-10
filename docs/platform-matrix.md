# 平台支持矩阵

状态定义：已验证 = 本机原生运行全部测试与生成 C 校验通过；受限 = 部分通过；未验证 = 无本机证据。

| 平台 | 架构 | 状态 | 最近验证 | C 工具链 | 最低系统 |
|---|---|---|---|---|---|
| Windows | x86-64 | 已验证 | 2026-11-10（v0.1.0 起） | MinGW-W64 GCC 16.2.0（UCRT） | 待定（实测 Windows 10.0.26300） |
| Linux | x86-64 | 已验证 | 2026-11-10 | GCC 14.2.0（Debian 13） | 待定（实测 Debian 13，内核 6.18） |
| Linux（净化器） | x86-64 | 已验证 | 2026-11-10 | GCC `-fsanitize=address,undefined` | 同上 |
| macOS / ARM64 / 交叉编译 | — | 未验证（不在首批承诺内） | — | — | — |

## 验证内容

- `cargo test --locked`：14 单元 + 138 集成测试（含声明处能力覆盖预检查的能力表一致性、与逐实例化检查的差分测试、弱约束拒绝与零误报用例、跨文件 `pub fn` 声明处诊断，以及 Win32 头按需引入的裁剪行为与标记审计、三个示例的生成 C 头决策与严格 C11 语法检查、`ae deps` 的接口摘要不变性/变更检测与依赖图；Unicode 路径与字符边界、受检算术边界、字符串/向量/映射/枚举的所有权与释放、循环与范围、`?` 传播（结果枚举与 `Option`）、哈希表、模块可见性、泛型枚举/泛型结构体/泛型函数实例化及预算、类型参数约束与布尔相等、泛型借用参数 `&T` 与只读借用转发、`find` 字节下标结果值、`f64` 规范文本/结构体字段/枚举负载/泛型实参与显式转换、排序/时钟/环境变量/追加、`exit` 状态码、标准输入与工作目录（含 `set_cwd`）、JSON 路径查询、`vec[string]` 借用元素拷贝、子进程状态与输出、缓存命中/损坏/清理/大小上限）。
- `scripts/verify_generated_c.py [--sanitize] [--leak]`：生成 C 在两种优化模式输出一致；失败路径（越界、断言、UTF-8 边界、分配失败）按预期非零退出；`--leak` 用分配计数要求每个程序的退出块数等于空程序基线（**Windows 2 块，Linux 0 块**——glibc 在 `main` 之前不保留堆分配，UCRT 会保留 2 块）；Linux 净化器下无地址/未定义行为报告。
- 全部 `examples/*.ae`（含 `examples/modules/` 的多文件程序）输出在两平台**按行尾归一化后**逐字节一致：Linux 为 LF，Windows 因 stdout 处于文本模式而为 CRLF；这不是原始字节一致，见「已知平台差异」。此前写成「逐字节一致」不准确，已修正。集成测试与示例比对都先把 `\r\n` 归一化为 `\n`。

### 记录时效

2026-11-10 的 Linux 运行覆盖了**到当时为止**的全部能力：第一阶段（InstaBox `evaluate-2`）记录的是性能基准，第二阶段（`evaluate-1`，见 `bench/results/2026-11-10-linux-evaluate-1/`）在新增哈希表、UTF-8 边界、分配器接口、`pub` 可见性、循环移动规则、泛型枚举、排序/时钟/环境变量/追加写入、`exit`、`read_stdin`/`cwd`、JSON 路径查询、缓存校验与上限、`strvec_new`/`run_command`、泛型结构体与泛型函数、`set_cwd`、类型参数约束之后重跑，9 + 126 全部通过、20 个生成 C 用例全部 PASS 且分配计数为 0。之后（`node`，见 `bench/results/2026-11-10-linux-node-option/`）再加入 `?` 对 `Option` 的传播，9 + 127 全部通过、21 个生成 C 用例 PASS，净化器与分配计数同样干净。第一阶段暴露并修复了一个**只在 Linux 出现**的缺陷：`clock_gettime` 属于 POSIX，在 `-std=c11 -pedantic-errors` 下被 glibc 隐藏，导致 91 个测试无法编译；改用 ISO C11 的 `timespec_get` 后消失（见 ADR-25）。

此后任何新增能力在提交前都要求两平台各跑一次上述命令；只有一方通过时，本文不会把它标记为跨平台已验证。

2026-11-10 在 Windows 本地与 Linux（instabox-node）各跑一次全量验证，覆盖 `?` 对 `Option` 的传播：9 单元 + 127 集成测试全部通过，21 个生成 C 用例在两种优化模式下 PASS，Linux 净化器无地址/未定义行为报告、`--leak` 的退出分配计数为 0（Windows 为 2 块基线）。原始输出见 `bench/results/2026-11-10-linux-node-option/`。

2026-11-10 本轮（ADR-35/36）先在 Windows 本地验证：`&T` 泛型借用参数、只读借用参数转发、`find(text, needle) -> IntResult`。`cargo test --locked` 为 9 单元 + 129 集成测试全部通过；`python scripts/verify_generated_c.py --leak` 共 23 个用例全部 PASS，每个程序的退出分配块数都等于 Windows 空程序基线（2 块），含 `find` 的 `Err` 分支与 `generic_borrow.ae` 的借用路径。

**同日 W1 已在 Linux 回填完成**（`bench/results/2026-11-10-linux-borrow-find/`，Debian 13 / GCC 14.2.0 / rustc 1.99.0）：`cargo test --locked` 9 单元 + 129 集成全部通过；`--sanitize` 23 个用例 PASS 且无 ASan/UBSan 报告；`--leak` 23 个用例 PASS、退出分配块数均为 glibc 基线 0；全部 `examples/*.ae` 通过 `check`；两个新示例与 Windows 输出按行尾归一化后一致。因此这三项能力现为**双平台验证**，不再是单平台原型；净化器只在 Linux 跑过（MinGW 无 ASan 运行时），这一限制不变。

**同日 W2（`f64`，ADR-37）也在两平台各跑一次**：Windows `bench/results/2026-11-10-windows-float/`、Linux `bench/results/2026-11-10-linux-float/`。两平台均为 9 单元 + 133 集成测试全通过；Linux `--sanitize` 24 个用例 PASS、无 ASan/UBSan 报告；`--leak` 24 个用例 PASS，Linux 退出分配块数为 0、Windows 等于 2 块基线（`float_math.ae` 的 `to_string`/`to_i64` 分配全部归还）。`float_math.ae` 输出在两平台按行尾归一化后逐字节一致——因为文本由运行时 `ae_f64_text` 生成，不经过 `printf` 的 `%g`；实测 UCRT 对 NaN 输出 `-nan(ind)`、glibc 输出 `-nan`，这正是不能用 `%g` 当语言语义的证据。生成 C 文本在两平台完全相同（trivial 109,721 字节、`float_math.ae` 116,494 字节，+6,773 字节主要是格式化器源码）；空程序的 release 产物仍是 Windows 61,309 / Linux 16,176 字节，说明未被引用的格式化器会被优化掉。`float_math.ae` 用了格式化与转换，release 产物 Windows 70,467 / Linux 21,576 字节；它与 `find_text.ae`（66,904 字节）不是同一程序，差值只能作量级参考，不能当作 `f64` 的精确增量。

**资源成本（如实记录）**：`f64` 支持让每个程序多约 6.8 KB 生成 C；实际进入可执行文件的只有引用到的格式化和转换路径（`float_math.ae` 这类程序约多几 KB，空程序 0）。这是「跨平台规范文本」的价格，不以关闭检查或依赖 CRT 差异换取体积。**注意这些字节数是 ADR-38 之前测的**：现在生成 C 只含可达运行时函数，同一程序已变成 20,273 字节，不能直接与旧数字相减当作某个特性的体积增量。

**同日 W3 阶段 1（ADR-38，运行时按需裁剪 + `WIN32_LEAN_AND_MEAN` + `ae deps`）在两平台各跑一次**：Windows `bench/results/2026-11-10-windows-w3/`、Linux `bench/results/2026-11-10-linux-w3/`。两平台 9 单元 + 135 集成测试全通过；Linux `--sanitize` 24 个用例 PASS、无 ASan/UBSan 报告；`--leak` 24 个用例 PASS，Linux 退出分配块数 0、Windows 等于 2 块基线；全部 `examples/*.ae` 通过 `check`。`deps examples/modules/main.ae` 的两份输出在行尾归一化后完全一致，说明接口摘要与路径标签与平台无关。

生成 C 文本在两平台完全相同：`fn main() { print(1); }` 从改动前的 109,721 字节降到 20,273 字节。同一份基线 C 已归档为 `baseline-full-runtime.c`，用 `bench/build_timings.py --full-c` 可复跑对比：Windows gcc 编译 1,030 → 690 ms、Linux 121.8 → 30.6 ms（各为 3 次中位值，本机多次测量波动明显，只报告方向与量级）。四类场景整进程耗时见各自 `timings.json`：Windows 冷 772 / 无修改 99 / 改实现 769 / 改接口 779 ms，Linux 37.4 / 5.6 / 34.3 / 37.1 ms。**PCH（69 MB 缓存）与分离翻译单元（冷构建回归约 68%）都实测否决并回退**，详见 ADR-38；本轮没有新增缓存种类，也没有关闭任何检查。

**同日 ADR-39（`<windows.h>` 按可达性引入）也在两平台各跑一次**：Windows `bench/results/2026-11-10-windows-winhdr/`、Linux `bench/results/2026-11-10-linux-winhdr/`。两平台 11 单元 + 136 集成测试全通过；Linux `--sanitize` 24 个用例 PASS、无 ASan/UBSan 报告；`--leak` 24 个用例 PASS，Linux 退出分配块数 0、Windows 等于 2 块基线；全部 `examples/*.ae` 通过 `check`；`deps` 输出归一化后完全一致（233 字节）。两平台的头决策一致：`float_math.ae` 去掉 windows.h、`file_tidy.ae` 保留，`print(1)` 生成 C 同为 20,352 字节。

`bench/build_timings.py --header-baseline` 把**同一份**生成 C 用字节级替换还原 `<windows.h>`，在同一次运行内交替计时。另外两平台都对全部 31 个 `examples/*.ae` 的生成 C 逐份做了严格 C11 语法检查，头决策完全一致（11 个含头、20 个去头），见各自的 `generated-c-syntax.log`——这是「漏标会让删头后的 C 缺声明」这条失败模式的最终兜底。Windows 三次独立运行（每次内部 3 次取中位）：含头 980.9 / 945.6 / 1,296.4 ms，去头 252.6 / 301.6 / 319.6 ms，同次运行内省 644.0–976.8 ms（3.14–4.06 倍）；Linux 含头 32.0 ms、去头 31.4 ms，差异在噪声内——因为 Linux 本来就不预处理 `#ifdef _WIN32`。**这条差异本身就是平台证据**：收益只属于 Windows，不能拿 Linux 数字代表 Windows 体验，也不能把它说成生成 C 体积变化带来的普遍加速。本机负载波动明显，四类场景的绝对值不与 W3 归档的单次数字直接比较；三次原始值保存在 `timings-runs.json`。

本轮**没有手写任何 Win32 声明**，方案文档 §21 的 W6 仍未实现，也没有引入 ABI 假设；用到文件、目录、时钟、环境变量、工作目录或子进程的程序仍会保留 `<windows.h>` 并付解析成本。漏加标记的后果是编译期缺声明报错（显式失败），多加只是多解析一次头；没有新增缓存种类、常驻进程，也没有关闭任何检查。可执行文件大小不变（Windows 64,195 / Linux 17,168 字节，`-O0`）。

**同日 ADR-40（泛型声明处能力覆盖预检查）也在两平台各跑一次**：Windows `bench/results/2026-11-10-windows-w4/`、Linux `bench/results/2026-11-10-linux-w4/`。两平台 14 单元 + 138 集成测试全通过；`--leak` 25 个用例 PASS，Linux 退出分配块数 0、Windows 等于 2 块基线；Linux `--sanitize` 25 个用例 PASS、0 报告；全部 32 个 `examples/*.ae` 通过 `check`；`deps` 输出归一化后完全一致（233 字节）。`print(1)` 生成 C 两平台同为 20,352 字节，头决策与 ADR-39 一致（`float_math.ae` 去头、`file_tidy.ae` 留头）。声明处诊断样例见 Windows 归档的 `declaration-audit.txt`：弱 `T: Copy` 配 `+` 在 `1:5` 被拒，收紧到 `T: Add` 后正常运行。

`bench/constraint_audit_timings.py` 用两份**字节数完全相同**、仅差 `[T: Add]` 与补齐注释的源码，在同一次运行内交替测 `check`，并断言两份 `emit-c` 输出逐字节相同（20,352 字节）——这是「纯前端检查不改变生成 C」的直接证据。归档（`--runs 7`，中位值，单位 ms）：Windows 200 个声明 113.4 / 103.1（差 10.3）、800 个声明 1433.0 / 1338.8（差 94.2）；Linux 200 个声明 67.6 / 62.1（差 5.5）、800 个声明 1012.6 / 892.6（差 119.9）。**差值是上界而非纯审计成本**：带约束版本每行多 2 个 token，`GenericFn` 按 ADR-31 为每个声明复制整份 token 流的 O(N²) 行为会放大这一点；本机负载波动明显（Linux 800 声明某次达 2,910 ms），绝对值不与 W3 / ADR-39 归档跨会话比较。四类场景中位：Windows 冷 412.6 / 无修改 118.2 / 改实现 370.1 / 改接口 391.7 ms，Linux 36.9 / 4.9 / 35.4 / 36.6 ms。

本轮**没有实现 trait/impl**（无方法表、无静态分派、无孤儿规则），约束仍只拒绝不放行，逐实例化完整类型检查保留。声明处检查是一次**收紧**：弱约束现在即使只打算用合法类型调用也会在声明处报错，迁移方式是收紧约束或去掉约束。没有新增缓存种类、常驻进程，运行 CPU/内存与可执行文件大小不变；不宣称「零编译时间回归」，只给出上界、归因与生成 C 不变的证据。

## 已知平台差异（不掩盖）

- 可执行文件命名：Windows `.exe`；Linux 无后缀且需要可执行位（`build` 自动 0755）。
- 产物体积：同一程序 Windows 约 61–64 KB，Linux 约 16.8 KB（链接器与 CRT 差异）。
- 地址/未定义行为净化器：MinGW 无 ASan 运行时，仅 Linux 验证；Windows 依赖测试矩阵与生成 C 审查。
- 分配计数基线：Windows 2 块、Linux 0 块，比较时各自减去自己的基线。
- 路径编码：Windows 内部使用 UTF-8 源码与非 ASCII 路径实测通过（`程序.ae`）；Linux 原生 UTF-8。
- 严格 ISO C：生成文件只用 C11 标准接口，不注入 `_POSIX_C_SOURCE` 之类的特性宏，因此同一份 C 在 glibc、musl 与 `-std=c11 -pedantic-errors` 下都能编译；代价是不能使用 POSIX 专有接口（时间只能走 `timespec_get`，拿不到单调时钟）。
- 标准输入的换行：两平台都按二进制读入，`\r\n` 不会被翻译；差异只存在于用户自己传入的文本里。
- 标准输出的换行：生成的 C 用 `fputs`/`printf`/`fputc('\n')` 写 stdout，Windows 文本模式把每个 `\n` 翻成 `\r\n`，Linux 保持 `\n`。因此跨平台比较必须先归一化行尾；Rust 集成测试与示例比对都这么做。这是 CRT 差异，不是生成代码里的分支。
- C 编译耗时：Windows 分支在**用到 Win32 能力时**要解析 `<windows.h>`（单独 `-fsyntax-only` 约 690 ms，`WIN32_LEAN_AND_MEAN` 后约 520 ms）；ADR-39 之后，不触及该层的程序不再引入这个头（同次运行对照：含头 945.6–1,296.4 ms、去头 252.6–319.6 ms）。Linux 始终不预处理这个分支，含头 32.0 ms、去头 31.4 ms 的差异在测量噪声内。两平台编译耗时仍差一个数量级，且 Windows 侧会随程序是否使用文件/目录/时钟/环境/子进程能力而变化；`bench/build_timings.py` 的两平台结果必须分别看，不能混成一个「编译速度」数字，也不能用 Linux 的数字代表 Windows 体验，更不能把跨会话的绝对值当作精确加速比。
- 子进程：`run_command` 在两平台都**不**经过 shell（Windows 直接 `CreateProcessW`，POSIX 直接 `execvp`），所以 `echo`、`dir` 这类内建命令要显式写成 `cmd` 加 `/c` 的形式。被信号杀死时 POSIX 报 `128 + 信号号`，Windows 直接给退出码——这是唯一有意保留的语义差异。

## 复现

Linux 结果与原始数据见 `bench/results/2026-11-10-linux-evaluate-1/`（Debian 13、GCC 14.2.0、rustc 1.99.0、4 vCPU / 7.8 GB：`PLATFORM.txt`、`cargo-test.log`、`verify-c.log`）与 `bench/results/2026-11-10-linux-node-option/`（同机型的 `instabox-node`，含 `verify-c-sanitize.log` 与 `verify-c-leak.log`），性能基准见 `bench/results/2026-11-10-linux-node3/`（InstaBox `evaluate-2`），Windows 见 `bench/results/2026-11-10-windows-local/`，本轮 `&T`/`find` 的 Windows 验证日志见 `bench/results/2026-11-10-windows-borrow-find/`，同一提交树的 Linux 回填见 `bench/results/2026-11-10-linux-borrow-find/`，`f64` 的两平台日志见 `bench/results/2026-11-10-windows-float/` 与 `bench/results/2026-11-10-linux-float/`（两处均有 `PLATFORM.txt`、`cargo-test.log`，Windows 另有 `verify-c-leak.log` 与 `example-output.txt`，Linux 另有 `verify-c-sanitize.log` 与 `verify-c-leak.log`）。W3 阶段 1 的两平台日志与四类场景计时见 `bench/results/2026-11-10-windows-w3/` 与 `bench/results/2026-11-10-linux-w3/`（`PLATFORM.txt`、`cargo-test.log`、`verify-c-*.log`、`timings.json`、`deps-output.txt`；Windows 另存 `baseline-full-runtime.c`）。ADR-39 的两平台日志见 `bench/results/2026-11-10-windows-winhdr/` 与 `bench/results/2026-11-10-linux-winhdr/`（各含 `PLATFORM.txt`、`cargo-test.log`、`verify-c-*.log`、`timings.json`、`generated-c-header-check.txt`、`deps-output.txt`；Windows 另有 `timings-runs.json`、`timings.txt`、`baseline-float_math-with-windows-header.c`）。ADR-40 的两平台日志见 `bench/results/2026-11-10-windows-w4/` 与 `bench/results/2026-11-10-linux-w4/`（各含 `PLATFORM.txt`、`cargo-test.log`、`verify-c-*.log`、`timings.json`、`audit-timings-200.json`、`audit-timings-800.json`、`examples-check.log`、`generated-c-header-check.txt`、`deps-output.txt`；Windows 另有 `declaration-audit.txt`）。命令：`cargo test --locked`、`python3 scripts/verify_generated_c.py --sanitize`、`python3 scripts/verify_generated_c.py --leak`、`python3 bench/run.py [--release] [--cold]`、`python3 bench/build_timings.py --runs 3 [--full-c PATH] [--header-baseline]`、`python3 bench/constraint_audit_timings.py --runs 7 --functions 200|800`、`ae deps <source.ae>`、`ae check <source.ae>`。
