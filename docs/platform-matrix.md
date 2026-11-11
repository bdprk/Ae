# 平台支持矩阵

状态定义：已验证 = 本机原生运行全部测试与生成 C 校验通过；受限 = 部分通过；未验证 = 无本机证据。

| 平台 | 架构 | 状态 | 最近验证 | C 工具链 | 最低系统 |
|---|---|---|---|---|---|
| Windows | x86-64 | 已验证 | 2026-11-10（v0.1.0 起） | MinGW-W64 GCC 16.2.0（UCRT） | 待定（实测 Windows 10.0.26300） |
| Linux | x86-64 | 已验证 | 2026-11-11 | GCC 14.2.0（Debian 13） | 待定（实测 Debian 13，内核 6.18） |
| Linux（净化器） | x86-64 | 已验证 | 2026-11-11 | GCC `-fsanitize=address,undefined` | 同上 |
| macOS / ARM64 / 交叉编译 | — | 未验证（不在首批承诺内） | — | — | — |

## 验证内容

- `cargo test --locked`：35 单元 + 164 集成测试（含 map 跨多次增长与墓碑复用、删除后 `keys()` 只报幸存键的用例、控制台与管道的输出编码用例、命令行非 ASCII 参数的 UTF-8 往返、流式 token 游标与 `split` 的逐 token 等价、借用拒绝与分隔符 16/17 字节边界、结构体字符串字段视图的借用用例、运行时注释归属函数的单元测试、非 ASCII 绝对输出路径的构建用例、split/lines 融合 arena 的边界用例、`check --json` 的 JSON 转义单元测试与结构化诊断集成测试、trait/impl 的分派语义、八类拒绝与跨文件一致性用例、按文件共享 token 流的 `Rc::ptr_eq` 回归断言、80 个泛型声明与 80 个实例化的位置一致性用例、声明处能力覆盖预检查的能力表一致性、与逐实例化检查的差分测试、弱约束拒绝与零误报用例、跨文件 `pub fn` 声明处诊断，以及 Win32 头按需引入的裁剪行为与标记审计、三个示例的生成 C 头决策与严格 C11 语法检查、`ae deps` 的接口摘要不变性/变更检测与依赖图；`fmt` 的幂等与 token 保真、`test` 三路径退出码、`map_inc` 缺键从零、字面量拼接与整数格式化的融合输出、Unicode 路径与字符边界、受检算术边界、字符串/向量/映射/枚举的所有权与释放、循环与范围、`?` 传播（结果枚举与 `Option`）、哈希表、模块可见性、泛型枚举/泛型结构体/泛型函数实例化及预算、类型参数约束与布尔相等、泛型借用参数 `&T` 与只读借用转发、`find` 字节下标结果值、`f64` 规范文本/结构体字段/枚举负载/泛型实参与显式转换、排序/时钟/环境变量/追加、`exit` 状态码、标准输入与工作目录（含 `set_cwd`）、JSON 路径查询、`vec[string]` 借用元素拷贝、子进程状态与输出、缓存命中/损坏/清理/大小上限）。
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

**同日 ADR-41（泛型声明按源文件共享词法结果）也在两平台各跑一次**：Windows `bench/results/2026-11-10-windows-w5-front/`、Linux `bench/results/2026-11-10-linux-w5-front/`。两平台 15 单元 + 139 集成测试全通过；`--leak` 25 个用例 PASS（Windows 2 块 / Linux 0 块），Linux `--sanitize` 25 个用例 PASS、0 报告；全部 32 个 examples `check` 通过；`deps` 输出归一化后一致（233 字节）；`print(1)` 生成 C 同为 20,352 字节。

**ADR-47（trait/impl 静态分派）两平台各跑一次**：Windows 本机 31 单元 + 154 集成全通过（新增 3 个 trait 集成：分派语义、八类拒绝、跨文件一致性），`--leak` 27 个用例 PASS、退出分配块数等于 2 块基线，`examples/traits.ae` 进入生成 C 用例集（26 → 27），`fmt` 验收 37 个文件 0 改动（trait/impl 语法幂等）；Linux `bench/results/2026-11-11-linux-traits/`（instabox-node，Debian 13 / GCC 14.2.0，提交 `ed780a2`）31 单元 + 154 集成同数通过、54 个用例 PASS（27 `--leak` + 27 `--sanitize`）、泄漏退出分配块 0（glibc 基线）、无 ASan/UBSan 报告，`traits.ae` 输出 `area=49 / area=12 / 12` 与 Windows 逐字一致。

分派在实例化时解析成 **impl 函数的一次直接调用**（`ae_fn_{id}`），没有方法表、没有函数指针、没有运行时分派，因此没有「分派开销」可测——差分测试断言经由 trait 的调用与手写函数输出一致，而不是给出加速比。生成 C、缓存、运行时数据结构与可执行文件大小不变；不新增缓存种类或常驻进程，没有关闭任何检查。

边界（不得越过）：trait 方法**不能在泛型体外**调用，impl 方法体之间也不能互调；trait 签名不支持 `&T`；无泛型 impl、默认方法、动态分派/trait 对象、关联类型与 `where` 子句；缺 impl 在实例化点报 `no impl of trait 'X' for 'Y'; required by 'Z'`，**不做声明处预检**（声明处预检仍只覆盖 ADR-40 的内建约束）。一致性：impl 只能写在 trait 或类型的声明文件里，第三文件拒绝；`(trait, type)` 全程序唯一；预算 64 trait / 256 impl。因为菱形导入本身被模块系统拒绝，「独立 impl 模块 + main 直连 trait 文件」的布局不可组合，这一点按现状记录，不宣称模块系统已支持。

**ADR-48（编辑器诊断 `check --json`）两平台各跑一次**：Windows `bench/results/2026-11-11-windows-checkjson/`、Linux `bench/results/2026-11-11-linux-checkjson/`。两平台 34 单元 + 155 集成全通过；`--leak` 27 用例 PASS（Windows 2 块 / Linux 0 块），Linux `--sanitize` 27 用例 PASS、0 报告；全部 34 个 examples 通过 `check`；`print(1)` 生成 C 同为 24,034 字节。`check --json` 的五种输入（成功、未知名字、缺失文件、反斜杠路径、参数顺序颠倒）在 Windows 输出 `file:line:column`，在 Linux 输出 `/tmp/...` 绝对路径，形状一致；默认 `check` 的单行输出与退出码未变。

首轮 Linux 运行暴露一处**平台相关断言**：集成测试无条件要求路径含反斜杠转义，Linux 上必然失败（154/155）；改为 `#[cfg(windows)]`/`#[cfg(unix)]` 分开断言后两平台全绿。这是测试自身的缺陷，不是实现差异——JSON 输出保留输入路径的分隔符原样。

`check --json` **不是** LSP：无增量、无错误恢复（数组当前至多 1 个元素）、只有起始位置；`message` 是展示文本、不保证逐字稳定，契约是 `file`/`line`/`column`。不引入 serde、不常驻、不关闭任何检查；`check` 与 `check --json` 走同一个 `load_program`。

**ADR-49（`split`/`lines` 单块 arena）两平台各跑一次**：Windows `bench/results/2026-11-11-windows-split-arena/`、Linux `bench/results/2026-11-11-linux-split-arena/`。两平台 34 单元 + 156 集成通过；`--leak` 28 个用例 PASS（Windows 2 块 / Linux 0 块），Linux `--sanitize` 28 个用例 PASS、0 报告；35 个 examples 通过 `check`；`print(1)` 生成 C 同为 25,134 字节且逐字节相同。

同次运行对照（两个真实二进制、7 次交替、中位，输入为 50,000 行 / 2.59 MB / 约 45 万 token 的确定性文本）：Windows `full` 44.78 → 24.37 ms（1.84 倍）、`count` 44.83 → 23.62 ms、`read` 16.73 → 16.61 ms；Linux `full` 25.08 → 10.22 ms（2.45 倍）、`count` 23.68 → 10.32 ms、`read` 1.40 → 1.46 ms。两平台的 `read` 差值都在噪声内，说明收益来自 `split` 而非 I/O；`split` 成本（`count − read`）Windows 28.10 → 7.01 ms、Linux 22.28 → 8.86 ms。**两平台绝对值不可比**，也不把 1.84/2.45 倍当普遍加速比；同一实现的历史运行出现过 1.85–1.92 倍（Windows）与 2.28–2.45 倍（Linux）的波动，只能当量级。

体积代价：`print(1)` 生成 C +1,100 字节（24,034 → 25,134，未被引用的单元仍被裁剪，可执行文件大小不变；ADR-50 之后同一探针为 11,105 字节），真正使用融合路径的 `split_arena.ae` 可执行文件 +2,341 字节（+3.2%）。元素仍是只读借用；`push`/`args`/`keys` 的向量仍逐元素自有；**不是零拷贝**。

**ADR-50（运行时注释随函数裁剪）两平台各跑一次**：Windows `bench/results/2026-11-11-windows-prune-comments/`、Linux `bench/results/2026-11-11-linux-prune-comments/`。两平台 35 单元 + 157 集成通过、28 个 `--leak` 用例 PASS、Linux `--sanitize` 28 用例 0 报告、35 个 examples 通过 `check`；`print(1)` 生成 C 两平台同为 11,105 字节。Windows 对照（7 次交替中位）：生成 C print 25,134 → 11,105（0.442x）、strings 25,813 → 12,042（0.467x）、map 32,947 → 22,481（0.682x）；同一份 C 的 `gcc -O0 -c` 差值 3.6 / 13.8 / −0.4 ms（小且不稳定，只当方向）；端到端 `build --release`（每次新缓存）390.66 → 346.12 ms（省 44.5 ms，11.4%），该差值大于单文件 gcc 差值，**机制未完全归因，只报实测值**。

发现过程：`print(1)` 的生成 C 在 ADR-49 之后变大，追查发现 `runtime_units` 把函数上方的 `//` 注释留在永远保留的「非函数文本」里，而 `ae_symbols` 会在这段文本里扫描 `ae_*` 名字——注释里提到的函数因此被当成依赖根，整个函数被拉进每个程序。现在注释随函数单元裁剪（`/* */` 与文件头保留）。字符串程序仍保留所需的 `ae_str_from`/`ae_str_eq`，输出不变。**不宣称「编译器普遍提速 11%」**：这是每程序的常量收益，程序越大比例越小；可执行文件大小不变。

**ADR-51（非 ASCII 绝对路径构建）两平台各跑一次**：Windows `bench/results/2026-11-11-windows-nonascii-build/`、Linux `bench/results/2026-11-11-linux-nonascii-build/`。缺陷形态：输出路径含中文且缓存未命中时，`ld` 收到按 ANSI 代码页替换过的路径，报 `cannot open output file ...: No such file or directory`；缓存命中不运行 gcc，因此长期只在首次构建时出现。修法是让编译器在临时目录内以相对文件名（`program.c`、`-o program.exe`）运行，命令行不含非 ASCII。新增集成测试 `build_accepts_non_ascii_absolute_output_paths`，**回滚修复后用 `git stash` 验证该测试确实失败**；修复后两平台通过（Linux 本来可用，作为无回归证据）。边界：只覆盖编译器调用，`AE_CC` 自身路径含非 ASCII 仍可能失败。

**ADR-52（结构体字符串字段的只读视图）两平台各跑一次**：Windows `bench/results/2026-11-11-windows-field-view/`、Linux `bench/results/2026-11-11-linux-field-view/`。两平台 35 单元 + 158 集成通过、29 个 `--leak` 用例 PASS（Windows 2 块 / Linux 0 块）、Linux `--sanitize` 29 用例 0 报告、36 个 examples 通过 `check`；`print(1)` 生成 C 仍为 11,105 字节（本改动不影响其它程序），`field_view.ae` 的生成 C 含 `AE_VIEW(v0.name)` 并通过严格 C11 语法检查，输出 `Grace / Hopper / 12` 两平台一致。

**ADR-53（流式 token 游标 `tokenize`）两平台各跑一次**：Windows `bench/results/2026-11-11-windows-tokens/`、Linux `bench/results/2026-11-11-linux-tokens/`。两平台 35 单元 + 160 集成通过、30 个 `--leak` 用例 PASS（Windows 2 块 / Linux 0 块）、Linux `--sanitize` 30 用例 0 报告、37 个 examples 通过 `check`；`print(1)` 生成 C 为 11,274 字节（未使用游标的程序只多 169 字节），`tokens.ae` 的生成 C 通过严格 C11 语法检查。

同一程序两种写法的同次对照（`bench/tokens_timings.py`，9 次交替中位，输入为 50,000 行 / 2.59 MB / 约 45 万 token）：Linux `read` 1.69 ms、物化 9.30 ms、惰性 3.56 ms（**2.61 倍**），超出 I/O 的 token 化成本 7.61 → 1.87 ms，峰值内存 12.52 → 3.90 MB；Windows `read` 15.09、物化 22.11、惰性 17.97（1.23 倍），token 化 7.02 → 2.88 ms，峰值内存 19.55 → 10.98 MB。两平台绝对值不可比；**Windows 的绝对差值尤其不可用**——这台机器每次新建进程要付约 37–40 ms（同一台机器 `sum` 负载 40 ms，Linux 同程序 0.88 ms），只有减掉共同底噪的 token 化差值与峰值内存可读。

跨语言：套件新增 `textlazy` 负载（三语言各自的本命惰性写法），Linux release 两次运行给出 AE 3.67–3.98 ms、Go 3.62–3.63、Rust 3.45–3.71——三者同档，排序在噪声内不可分辨；峰值内存 AE 最低（3.85–3.90 vs 4.54–4.60 / 4.48–4.49 MB）。同一输入的物化版 `text` 为 AE 9.26–9.48 / Go 11.83–11.86 / Rust 3.66（Rust 那一栏本来就是惰性写法，不可比）。Windows 的套件 JSON 原样归档但不作跨语言结论。

**测量口径变了**：`bench/run.py` 与 `bench/tokens_timings.py` 现在把墙钟时间与峰值内存分成两趟。旧实现把采样放在计时循环里，而 Windows 的 `time.sleep(0.001)` 实际睡满 15.6 ms 时钟中断，导致每个数字被拉长十几毫秒且「越快越被拉长」。因此本轮归档的墙钟数字与更早的 Windows 归档不可跨会话比较，更早的 Windows 数字偏高；Linux 采样走 `/proc`，影响 ≤1 ms。

`tokenize` **不是通用迭代器**：只有「字符串 + 分隔符」一种形态，没有 `len`（计数必须消费整条流），游标不能 `print`/进容器/作返回值、分隔符上限 16 字节。`split` 一行未改——变快的是用另一种写法写的同一个程序。

**ADR-54（`args()` 在 Windows 上转 UTF-8）两平台各跑一次**：Windows `bench/results/2026-11-11-windows-argv-utf8/`、Linux `bench/results/2026-11-11-linux-argv-utf8/`。缺陷形态：Windows 的 CRT 按 ANSI 代码页把命令行交给 `main`，含中文的路径因此以 GBK 字节进入程序（`len(args()[1])` = 8），而运行时按 UTF-8 解路径，`_wfopen` 打开一个不存在的名字——源码里的中文字面量却一直正常。修复：`args()` 把每个实参 CP_ACP → UTF-16 → UTF-8，失败时回退原始字节。两平台 35 单元 + 161 集成通过（新增用例把中文绝对路径当实参，`write_file` + `read_file` 走通，并断言长度为 12 而非 8；**回滚修复后该用例失败**）、30 个 `--leak` 用例 PASS、37 个 examples 通过 `check`。

代价：调用 `args()` 的程序在 Windows 上会保留 `<windows.h>`（转换用到 Win32），只读参数的探测程序生成 C 16,317 字节；不用 `args()` 的程序不受影响——`print(1)` 仍 11,299 字节且仍带 `/* windows.h omitted */`，`float_math.ae` 同样去头（ADR-55 之后 `print(1)` 为 15,243 字节）。边界：命令行进入 `main` 时已是 ANSI 字节，代码页表示不了的字符（部分 emoji）在 CRT 那一步就丢失，本修复无法恢复；`__wgetmainargs` 取原始 UTF-16 命令行在这套 UCRT 工具链上段错误，已记录不再重试。

**ADR-55（`print` 在 Windows 控制台上按 UTF-16 写出）**：Windows `bench/results/2026-11-11-windows-console-utf8/`、Linux `bench/results/2026-11-11-linux-console-utf8/`。缺陷形态：运行时一直写的是正确 UTF-8 字节，但控制台按**自己的输出代码页**（中文 Windows 为 936）解码接收到的字节，于是 `print("中文测试")` 在屏幕上显示成 `涓枃娴嬭瘯`。取证方法是自建一个 CP-936 控制台、把 `CONOUT$` 句柄交给被测程序、再用 `ReadConsoleOutputCharacterW` 读回屏幕缓冲区（存的就是用户看到的字符）：同一段字节窄写得到 `U+6D93 U+E15F U+6783 U+5A34 U+5B2D U+762F`，`WriteConsoleW` 得到 `U+4E2D U+6587 U+6D4B U+8BD5`。修复：`print` 的四种形态统一走一个汇点，运行期判断 stdout 是否为控制台——是则分块转成 UTF-16（1 KiB 块、栈上 2 KiB、块边界不切字符）并**把换行并进同一次写入**，否则原样写字节；四个 Win32 入口在用到它们的函数体内手写声明，不引入 `<windows.h>`（顶层文本永远保留，函数内声明随函数裁剪）。

两平台 35 单元 + 162 集成通过（新增用例覆盖四种形态加中文字面量的逐字节输出）、30 个 `--leak` 用例 PASS（Windows 2 块 / Linux 0 块）、Linux `--sanitize` 30 用例 0 报告、37 个 examples 通过 `check`；`print(1)` 与 `args`+`print` 两种程序的生成 C 通过严格 C11 语法检查（后者同时保留 `windows.h` 与自声明入口，两者并存不冲突）。控制台边界与分块：1022 个 ASCII + 一个正好跨 1023 字节边界的 3 字节字符逐字符复原，3000 个汉字（多次分块）逐字符复原，非法 UTF-8 字节在控制台显示为 U+FFFD 而在管道里原样通过。

代价：生成 C `print(1)` 11,341 → 15,243 字节（+34%）、不打印的程序 11,286 → 11,811（+525）、`args`+`print` 15,178 → 20,742；构建时间 74.6 → 75.7 ms（9 次交替，噪声内）；控制台吞吐 2 万行 860.3 → 832.0 ms（一行一次 `WriteConsoleW`，无回归）、重定向到文件 17.6 → 18.2 ms（管道路径只多一次调用与一次判断）；转换不新增分配。边界：控制台表示不了的字符（emoji）显示为 U+FFFD，这是控制台的行为（直接调用 `WriteConsoleW` 的探针在 936 与 65001 两种代码页下同样如此），不宣称「emoji 也能显示」；只处理 stdout，运行时错误消息仍是 ASCII 的 `fprintf`；不改控制台代码页，退出后父 shell 环境不变。

**ADR-56（map 装载因子 0.75 → 0.5）**：Windows `bench/results/2026-11-11-windows-map-loadfactor/`、Linux `bench/results/2026-11-11-linux-map-loadfactor/`。先归因再改：四个探针把 map 负载拆成 `arith`（3.76 / 3.57 ms）、`key`（32.55 / 30.82）、`hit`（60.71 / 55.67）、`put`（58.27 / 53.76，均为 AE / Rust），得出「算术同级、键构造占一半、差距在探测次数」。改动两处：增长阈值 3/4 → 1/2（有效装载率 0.61 → 0.31，平均探测约 1.3 次）与 `ae_str_eq` 对 ≤8 字节走字节循环（省掉 `memcmp` 调用）。

两平台 35 单元 + 163 集成通过（新增 `map_grows_reuses_removed_slots_and_keeps_every_survivor`）、30 个 `--leak` 用例 PASS（Windows 2 块 / Linux 0 块，槽表扩容释放旧表这条路径被覆盖）、Linux `--sanitize` 30 用例 0 报告、37 个 examples 通过 `check`。

Linux 11 次交替配对：map 负载 67.92 → 58.08 ms（**1.172×**，全部比值 ≥1.12），`hit` 1.159×、`put` 1.178×；四负载套件（5 次交错）map 为 **57.72 ms / 3.14 MB**，Go 58.29 / 7.48、Rust 55.37 / 2.98、Python 236.88 / 9.36——从「落后 Rust 27%、与 Go 平手」变成「快过 Go、落后 Rust 4%」。代价是峰值内存 2.44 → 3.43 MB（+41%，每元素约 64 字节槽开销）；5/8 装载因子只给 1.03× 却同样多占内存，所以没采用。

边界：收益是负载相关的（探测密集的表明显、小表几乎无差别），内存代价对所有表成立；`keys` 按字节序排序，迭代顺序与可观察行为不变；仍不改哈希函数，也不宣称「map 比 Rust 快」。顺带量到但未修的一项：`map_get` 未命中时分配理由字符串，1M 次未命中约 6.9 ms（Rust 的 `Option<i64>` 为 0.5 ms），记为下一轮候选。

**ADR-57（map 状态位搬出槽，32 → 24 字节）**：Windows `bench/results/2026-11-11-windows-map-states/`、Linux `bench/results/2026-11-11-linux-map-states/`。ADR-56 的代价写在案：每元素约 64 字节槽开销、峰值 3.43 MB。这一步把 `unsigned char state` 从槽里搬到平行的 `unsigned char *states`（每槽 1 字节），槽只剩 `{ key, value }` 共 24 字节，探测链先读 32 KB 的状态数组（常驻 L1/L2）再碰槽。

两平台 35 单元 + 164 集成通过（新增 `keys_after_removal_skips_tombstones_and_sees_reused_slots`）、30 个 `--leak` 用例 PASS、Linux `--sanitize` 30 用例 0 报告、37 个 examples 通过 `check`。

Linux 11 次交替配对：map 负载 1.012×、`hit` 1.007×、`put` 1.027×（持平）；峰值内存 **3.43 → 2.90 MB（−15%）**，表本体 1,048,576 → 819,200 字节（−22%）。四负载套件 map 59.88 ms / 2.86 MB，Rust 56.99 / 2.91、Go 69.71 / 7.49——**内存已低于 Rust，时间仍慢约 5%**。生成 C `print(1)` 15,243 → 15,302（+59）。边界：这是内部布局，`keys()` 顺序与哈希不变；时间是「持平」不是「更快」。

设计要点：`slice_view` 的 base 现在可以是「变量.字符串字段」，但借用对象是**整个变量**——视图存活期内移动、整体赋值、写该变量任一字段都被拒绝（字段写入路径原本**不查** `borrows`，本次一并补上，属于收紧）。嵌套字段、`v[0].name`、非字符串字段仍分别报错。**不是字段级借用，也不是通用借用检查器**；视图仍只读、不能作返回值或字段。生成 C 与运行时零新增开销。

`bench/shared_tokens_timings.py` 用**两个真实二进制**（改动前用 `git archive 55f6b14` 导出源码重建，改动后为当前树）在同一脚本、同一份源码、同一时刻交替测 `check`，并断言两者 `emit-c` 逐字节相同、声明处诊断一致。基线树的 `runtime.c`/`runtime_ex.c` 已换成与工作区相同行尾，避免把 `git archive` 带出的 CRLF 差异误判为语义变化；脚本仍同时记录原始与归一化后的一致性。归档（`--runs 7`，中位值，单位 ms）：

| 声明数 | Windows 旧 → 新 | Linux 旧 → 新 |
| --- | --- | --- |
| 200 | 132.5 → 29.1（省 103.4，4.55 倍） | 64.1 → 3.4（省 60.7，18.80 倍） |
| 800 | 1575.9 → 32.8（省 1543.1，48.03 倍） | 987.2 → 9.3（省 978.0，106.73 倍） |

收益随声明数超线性增长，是 O(N²) → O(N) 的形状；ADR-40 的能力审计对照差值也随之从 10.3 / 94.2 ms 降到 1.2 / 4.5 ms（Windows 200/800），印证之前的差值主要来自复制而非审计。四类场景中位：Windows 冷 352.4 / 无修改 139.3 / 改实现 457.2 / 改接口 431.3 ms，Linux 38.2 / 5.4 / 40.0 / 40.2 ms。本机负载波动明显（Linux 800 声明某次达 1,629 ms），只报同次运行内的比值与量级，不与 W3 / ADR-39 / ADR-40 归档的绝对值跨会话相减。

本轮**没有切分翻译单元、没有新增缓存或常驻进程，也没有关闭任何检查**；生成 C 与可执行文件完全不变（Windows 64,195 / Linux 17,168 字节，`-O0`）。不宣称「所有程序编译都变快」——收益集中在泛型声明多的文件，单函数程序差异在噪声内；也不宣称峰值内存下降的具体数字（未做峰值采样）。W3 阶段 2 的用户模块级增量仍被 ADR-38 的实测结论阻塞。

2026-11-11 把 `fmt`（ADR-42）与 `test`（ADR-43）回填到 Linux：同一提交 `5d153d4` 在 instabox-node（Debian 13 / GCC 14.2.0 / rustc 1.99.0，证书用 `instabox ssh-cert node` 刷新）各跑一次全套命令——31 单元 + 148 集成全通过、净化器与 `--leak` 全干净、`fmt_acceptance.py` 35 文件 changed=0 且幂等、test 三路径退出码逐字一致、32 个独立示例 + 模块根 check OK。两项能力升级为**双平台验证**；日志见 `bench/results/2026-11-11-linux-fmt-test/`。fmt 的总耗时两平台绝对值不可比（Linux 23 ms / Windows ~300 ms，进程模型不同），只记量级。

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

Linux 结果与原始数据见 `bench/results/2026-11-10-linux-evaluate-1/`（Debian 13、GCC 14.2.0、rustc 1.99.0、4 vCPU / 7.8 GB：`PLATFORM.txt`、`cargo-test.log`、`verify-c.log`）与 `bench/results/2026-11-10-linux-node-option/`（同机型的 `instabox-node`，含 `verify-c-sanitize.log` 与 `verify-c-leak.log`），性能基准见 `bench/results/2026-11-10-linux-node3/`（InstaBox `evaluate-2`），Windows 见 `bench/results/2026-11-10-windows-local/`，本轮 `&T`/`find` 的 Windows 验证日志见 `bench/results/2026-11-10-windows-borrow-find/`，同一提交树的 Linux 回填见 `bench/results/2026-11-10-linux-borrow-find/`，`f64` 的两平台日志见 `bench/results/2026-11-10-windows-float/` 与 `bench/results/2026-11-10-linux-float/`（两处均有 `PLATFORM.txt`、`cargo-test.log`，Windows 另有 `verify-c-leak.log` 与 `example-output.txt`，Linux 另有 `verify-c-sanitize.log` 与 `verify-c-leak.log`）。W3 阶段 1 的两平台日志与四类场景计时见 `bench/results/2026-11-10-windows-w3/` 与 `bench/results/2026-11-10-linux-w3/`（`PLATFORM.txt`、`cargo-test.log`、`verify-c-*.log`、`timings.json`、`deps-output.txt`；Windows 另存 `baseline-full-runtime.c`）。ADR-39 的两平台日志见 `bench/results/2026-11-10-windows-winhdr/` 与 `bench/results/2026-11-10-linux-winhdr/`（各含 `PLATFORM.txt`、`cargo-test.log`、`verify-c-*.log`、`timings.json`、`generated-c-header-check.txt`、`deps-output.txt`；Windows 另有 `timings-runs.json`、`timings.txt`、`baseline-float_math-with-windows-header.c`）。ADR-40 的两平台日志见 `bench/results/2026-11-10-windows-w4/` 与 `bench/results/2026-11-10-linux-w4/`（各含 `PLATFORM.txt`、`cargo-test.log`、`verify-c-*.log`、`timings.json`、`audit-timings-200.json`、`audit-timings-800.json`、`examples-check.log`、`generated-c-header-check.txt`、`deps-output.txt`；Windows 另有 `declaration-audit.txt`）。ADR-41 的两平台日志见 `bench/results/2026-11-10-windows-w5-front/` 与 `bench/results/2026-11-10-linux-w5-front/`（各含 `PLATFORM.txt`（含基线二进制重建命令）、`cargo-test.log`、`verify-c-*.log`、`timings.json`、`shared-200.json`、`shared-800.json`、`audit-timings-200.json`、`examples-check.log`、`generated-c-header-check.txt`、`deps-output.txt`）。ADR-42/43（`fmt`/`test`）见 `bench/results/2026-11-10-windows-fmt/`、`bench/results/2026-11-10-windows-test/` 与 Linux 回填 `bench/results/2026-11-11-linux-fmt-test/`；ADR-44/45（字面量拼接融合、整数格式化栈缓冲）见 `bench/results/2026-11-11-{windows,linux}-litcat/` 与 `bench/results/2026-11-11-{windows,linux}-intbuf/`；ADR-46（`map_inc`）见 `bench/results/2026-11-11-{windows,linux}-map-inc/`，热点定位数据见 `bench/results/2026-11-11-windows-map-hotspots/`；ADR-47（trait/impl 静态分派）的 Linux 记录见 `bench/results/2026-11-11-linux-traits/`；ADR-48（编辑器诊断）见 `bench/results/2026-11-11-{windows,linux}-checkjson/`；ADR-49（split/lines 融合 arena）见 `bench/results/2026-11-11-{windows,linux}-split-arena/`（含旧/新二进制同次对照的 7 次原始值）；ADR-50（运行时注释随函数裁剪）见 `bench/results/2026-11-11-{windows,linux}-prune-comments/`；ADR-51（非 ASCII 绝对路径构建）见 `bench/results/2026-11-11-{windows,linux}-nonascii-build/`；ADR-52（结构体字段的只读视图）见 `bench/results/2026-11-11-{windows,linux}-field-view/`；ADR-53（流式 token 游标）见 `bench/results/2026-11-11-{windows,linux}-tokens/`（含两种写法的 9 次原始值与四负载套件 JSON）；ADR-54（命令行 UTF-8）见 `bench/results/2026-11-11-{windows,linux}-argv-utf8/`；ADR-55（控制台输出编码）见 `bench/results/2026-11-11-{windows,linux}-console-utf8/`（Windows 侧另含 CP-936 控制台启动器与探针源码）；ADR-56（map 装载因子）见 `bench/results/2026-11-11-{windows,linux}-map-loadfactor/`（Linux 侧含四个归因探针、`pair.py` 与 `measure.py`）；ADR-57（槽 32 → 24 字节）见 `bench/results/2026-11-11-{windows,linux}-map-states/`。命令：`cargo test --locked`、`python3 scripts/verify_generated_c.py --sanitize`、`python3 scripts/verify_generated_c.py --leak`、`python3 bench/run.py [--release] [--cold]`、`python3 bench/build_timings.py --runs 3 [--full-c PATH] [--header-baseline]`、`python3 bench/constraint_audit_timings.py --runs 7 --functions 200|800`、`python3 bench/shared_tokens_timings.py --old-exe <pre-ADR-41-binary> --runs 7 --functions 200|800`、`python3 scripts/fmt_acceptance.py`、`ae fmt <source.ae>`、`ae test <source.ae>`、`ae deps <source.ae>`、`ae check <source.ae>`、`ae check --json <source.ae>`、`python3 bench/split_timings.py --old-exe <pre-ADR-49-binary> --runs 7`、`python3 bench/prune_timings.py --old-exe <pre-ADR-50-binary> --runs 7`、`python3 bench/tokens_timings.py --runs 9`（同一二进制的两种写法，含峰值内存）。ADR-58（未命中理由文本改用只读常量）见 `bench/results/2026-11-11-{windows,linux}-map-missing/`（Linux 侧含 miss/hit 探针与 Rust 孪生程序、`pair.py`、`measure.py`、21 次交替配对的原始值）。ADR-59（build 默认优化构建、--debug 快速构建）见 `bench/results/2026-11-11-{windows,linux}-buildlevels/`（各含 `PLATFORM.txt` 与 `build-levels.json`；命令 `python3 bench/build_levels_timings.py --runs 5 --run-runs 9 --json <out>`；Linux 是定量基准：运行 1.34×–4.05×、构建 +12.8–87.4 ms）。ADR-60（数学内建 v1）见 `bench/results/2026-11-11-{windows,linux}-math/`（各含 `PLATFORM.txt`、`cargo-test.log`、`verify-c-leak.log`、`fmt-acceptance.{log,json}`、`examples-check.log`；命令 `python3 scripts/fmt_acceptance.py --json <out>`，`--leak` 用例表含 math_basics.ae；Linux 另有 `verify-c-sanitize.log`，math_basics 输出两平台逐字节一致）。ADR-61（vec[f64]）见 `bench/results/2026-11-12-{windows,linux}-vecoff64/`（各含 `PLATFORM.txt`、`cargo-test.log`、`verify-c-leak.log` 33 用例、`fmt-acceptance.{log,json}`、`examples-check.log`；Linux 另有 `verify-c-sanitize.log` 与 `float-stats-output.txt`，输出两平台逐字节一致——行尾归一化后；`print(1)` 13,909 字节两平台相同）。ADR-62（test 名字过滤）见 `bench/results/2026-11-12-{windows,linux}-testfilter/`（各含 `PLATFORM.txt`、`cargo-test.log` 36 单元 + 174 集成、`verify-c-leak.log` 33 用例、`fmt-acceptance.{log,json}`；Linux 另有 `verify-c-sanitize.log` 与 CLI 过滤演示 `filter-cli-demo.txt`，行为两平台逐字一致）。ADR-63（通用借用检查器设计稿，状态：已设计未实现）见 `docs/borrow-checker-design.md` 与 `docs/resource-model.md` §10 指针，无可运行证据，实现按其 §6 分级推进。ADR-64（min/max 运行时实现）见 `bench/results/2026-11-12-windows-minmax/`（`PLATFORM.txt`、`cargo-test.log` 36 单元 + 176 集成、`verify-c-leak.log` 33 用例、`fmt-acceptance.{log,json}`、`math-basics-output.txt` 含 min/max 五行）；Linux 回填待做，回填前保持单平台验证。
