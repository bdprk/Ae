# AE

AE 是自主研发的实验性原生编程语言，首批目标平台为 Windows 和 Linux x86-64。目标是以 Go 的编译速度、Rust 的运行性能和安全为参照，尽量降低 CPU、内存和磁盘占用；通用能力与生态是次一级但长期必须建设的目标（见 [设计路线](docs/design.md)）。

优先级：安全和正确性是底线；运行 CPU 和内存共同优化；编译速度和磁盘占用都必须满足未来基于基准设定的预算。功能可以为这些目标让路。

当前原型支持的子集：`fn main()` 与用户函数（`i64`/`bool`/`f64`/`string`/`vec` 参数与返回值，含 `&vec` 只读借用与 `&[i64; N]` 数组借用）、`let`/`var`、赋值、整数、浮点与布尔字面量、字符串字面量与转义、括号、`+ - * / %` 与一元 `-`/`!`、受检算术（溢出、除零报告位置并退出）、**`f64`（IEEE 754 binary64，无隐式转换、除法检查零、跨平台规范文本）**、比较（含字符串内容相等）与 `&&`/`||` 短路、`if`/`else if`/`else`、`while`、`for i in a..b`（半开区间，界只求值一次）、`break`/`continue`、`assert`、`print`、`len`、固定长度数组与**动态向量 `vec`**（`push`/`pop`/索引，全部边界检查）、**拥有型字符串与向量的移动语义与确定性释放**、**`exit(状态码)`（终止前刷新输出并释放存活拥有值）**、**模块（`use` 导入、`pub` 显式导出、环检测、跨文件重名拒绝）**、**结构体（拥有字段、字段借用、确定性释放）**、**枚举与穷尽 `match`**（变体负载 `i64`/`bool`/`string`，负载随枚举确定性释放）、**泛型函数**（`fn identity[T](x: T) -> T`，调用点写 `identity[i64](41)`，逐实参单态化、可跨文件实例化、泛型体内可再实例化其他泛型，循环实例化在闭合处报错）、**类型参数约束**（`fn add[T: Add](a: T, b: T)`，内置 `Add`/`Eq`/`Ord`/`Copy`，在调用点检查实参形状、在声明处检查约束与函数体的能力覆盖，未知或重复约束在声明处报错）、**trait 与 impl（静态分派）**（`trait Area { fn area(shape: T) -> i64; }` + `impl Area for Rect { ... }`，泛型体内直接调用 trait 方法，实例化时解析到 impl 的函数——无方法表；一致性规则：impl 与 trait/类型同文件，(trait, type) 全程序唯一）、**泛型结构体**（`struct Pair[K, V] { left: K, right: V }`，同一实参列表共用一份布局、不同实参是不同类型，字段可为任意命名类型，每程序 256 个结构体类型的预算）、**泛型枚举与 `Option[T]`**（`enum Either[T, U] { ... }` 按实参单态化，同一实参列表共用一份布局，预置 `Option[T]` 用于「可能没有」的返回值，每程序 256 个枚举类型的预算）、**错误与 `None` 传播 `?`**（`let x = call()?;` 与 `call()?;`，`Err`/`None` 时释放已绑定值并原样返回给调用者，`Ok`/`Some` 绑定或释放负载；结果枚举与 `Option[T]` 同规则）、**可恢复错误**（预置 `ReadResult`/`WriteResult`/`IntResult`/`DirResult`/`FileResult`/`StatusResult`/`BoolResult`/`CommandResult` 与 `try_read_file`/`try_write_file`/`try_list_dir`/`try_file_info`/`parse_int`/`to_string`，失败作为值返回而非终止进程）、**目录遍历与路径查询**（`try_list_dir`/`path_exists`/`path_is_dir`）、**路径代数**（`path_join`/`path_name`/`path_parent`，两平台统一 `/`）、**文件整理操作**（`try_copy_file`/`try_rename_path`/`try_remove_file`/`try_create_dir`/`try_remove_dir`，全部可恢复）、**文件元数据**（`try_file_info` 返回字节数、Unix 秒时间戳与目录标志）、**原地排序与反序**（`sort`/`reverse`，i64 按数值、`vec[string]` 按字节序）、**墙钟时间**（`now_s`/`now_ms`，与文件 mtime 同单位）、**环境变量**（`env` 返回 `ReadResult`，区分「空值」与「未设置」）、**追加写入**（`try_append_file`）、**标准输入与工作目录**（`read_stdin` 按二进制读入全部输入、`cwd` 返回 `/` 归一化的绝对路径，两者都返回结果值；`set_cwd` 改变进程工作目录并影响之后的相对路径与子进程）、**JSON 路径查询**（`json_get`/`json_int`/`json_bool`/`json_len`/`json_has`，路径 `key.sub[2]`，返回值而不是哨兵；只读、无文档类型、`\uXXXX` 与代理对还原）、**空字符串向量与子进程执行**（`strvec_new()` 造空 `vec[string]`；`run_command(program, args)` 合并捕获 stdout/stderr，退出码在 `Ok`、启动失败才是 `Err`，信号按 `128 + 信号号` 报告，不隐式经过 shell）、**哈希表 `map[string, i64]`**（`map_new`/`map_get`/`map_has`/`map_remove`/`keys`，缺失键返回 `Err` 而不是哨兵 0，`keys` 按字节序复制，**`map_inc` 单次查找计数、缺键从零开始**）、**只读借用参数**（`&string`/`&vec[string]`，不移动不复制）、**UTF-8 字符边界**（`slice` 不得切开字符，`utf8_len`/`utf8_prefix` 按码点计数与截断）、**`for x in vec` 元素迭代**、**文件读写 `read_file`/`write_file`**（UTF-8 路径；源码字面量与命令行传入的非 ASCII 路径都可用——`args()` 在 Windows 上把 ANSI 命令行经 UTF-16 转成 UTF-8，ADR-54；代码页表示不了的字符仍会在进入程序前丢失）、`bytes()`/`contains()`/`starts_with()`/`ends_with()`/`trim()`/`slice()`/`split()`/`join()`/`lines()`/`count_lines_with()`/`args()` 与 `vec[string]` 字符串向量与**借用字符串视图**（`slice_view` 零拷贝切片，借用跟踪）、**`find(text, needle) -> IntResult`**（首次匹配的字节下标，未命中返回 `Err` 而不是 -1 哨兵，成功路径零分配）、**结构体字符串字段的只读视图**（`slice_view(p.name, 0, 5)`，借用整个变量：移动/整体赋值/写任一字段都会被拒绝，ADR-52）、**`split`/`lines` 的单块 arena**（ADR-49：一次切分的分配次数与 token 数无关，text 负载同次对照 1.84 倍）、**流式 token 游标 `tokenize`**（ADR-53：与 `split` 同边界但按 token 零分配，2.5 MB 文本 9.24 → 3.56 ms、峰值内存 12.52 → 3.88 MB）、**控制台输出编码**（ADR-55：写控制台时把 UTF-8 转成 UTF-16 交给宽字符接口，中文在 Windows 终端里不再是乱码；写管道与文件仍是原始 UTF-8 字节，控制台表示不了的字符显示为 `U+FFFD`）、**泛型借用参数 `&T`**（在每个实例化处落到既有的 `string view`/`&vec`/`&vec[string]`，不引入新生命周期）与**只读借用转发**（`&vec` 参数可原样交给另一个只读参数）。表达式从左到右求值；字符串与向量赋值/传参/返回按移动处理，移动后使用被编译期拒绝，并有运行时空检查兜底；块退出、提前返回与跳转路径全部插入幂等释放。

诊断显示行列号；泛型目前是逐实参单态化（枚举、结构体、函数）。trait/impl 已能做**静态分派**（ADR-47，双平台）：分派在实例化时解析成一次直接调用，没有方法表；但它是 v1 形状——没有方法调用语法、没有 `&T` trait 参数、没有泛型 impl、没有默认方法、没有动态分派，trait 方法也不能在泛型体外直接调用。声明处的能力覆盖检查（ADR-40）只是更早的拒绝，不是静态分派，也不能证明泛型体合法。并发与正式安全证明尚未实现，不能据此宣称完整的内存安全语言已完成。`&T` 只对三种已有借用形状可用，`find` 按字节计数且与 `contains` 在嵌入 NUL 时的行为不同，`f64` 没有隐式转换、没有数学库、容器里也还不能装（`vec` 与 `map` 仍是整数容器），规则与限制见 [当前语言语义](docs/current-language.md)。

## 使用

需要 Rust 和支持 C11 的 GCC；使用 `AE_CC` 指定其他兼容的编译器可执行文件。Windows（GCC/MinGW）与 Linux（GCC）均已原生验证：双平台全部测试、Linux 净化器检查通过，示例输出按行尾归一化后一致（Windows stdout 为 CRLF，Linux 为 LF；输出到 Windows 控制台时按控制台代码页转成 UTF-16，见 ADR-55，详见[平台支持矩阵](docs/platform-matrix.md)）。

```sh
cargo test
cargo run -- check examples/hello.ae
cargo run -- check --json examples/hello.ae   # 编辑器用：stdout 恰为一个 JSON 数组
cargo run -- emit-c examples/hello.ae
cargo run -- fmt examples/hello.ae
cargo run -- test tests_demo/strings_test.ae   # 约定：fn test_<名称>() -> i64，0 为通过
cargo run -- run examples/hello.ae
cargo run -- run examples/enums.ae
cargo run -- run examples/concat.ae
cargo run -- run examples/sum_numbers.ae -- 文件路径
cargo run -- run examples/find_files.ae -- . txt
cargo run -- run examples/file_tidy.ae
cargo run -- run examples/copy_notes.ae
cargo run -- run examples/word_count.ae
cargo run -- run examples/maybe.ae
cargo run -- run examples/option_propagate.ae
cargo run -- run examples/generic_borrow.ae
cargo run -- run examples/find_text.ae
cargo run -- run examples/float_math.ae
cargo run -- run examples/dir_report.ae
cargo run -- run examples/json_report.ae
cargo run -- run examples/pairs.ae
cargo run -- run examples/generic_fn.ae
cargo run -- run examples/tokens.ae   # 流式 token 游标：按 token 零分配
cargo run -- run examples/constraints.ae
cargo run -- run examples/generic_constraints.ae
cargo run -- run examples/run_tool.ae -- git --version   # 把参数转发给子进程
cargo run -- run examples/text_preview.ae
cargo run -- run examples/modules/main.ae
cargo run -- run examples/strings.ae
cargo run -- run examples/vectors.ae
cargo run -- run examples/grep.ae
cargo run -- run examples/loops.ae
cargo run -- run examples/arrays.ae
cargo run -- run examples/functions.ae
cargo run -- run examples/borrow.ae
cargo run -- build examples/arithmetic.ae -o arithmetic.exe
cargo run -- run examples/arithmetic.ae --release
cargo run -- cache stats            # 条目数、字节数、上限
cargo run -- deps examples/modules/main.ae   # 模块依赖图与公开接口摘要
cargo run -- version
```

默认构建使用 `-O0`，`run` 或 `build` 末尾加 `--release` 使用 `-O2`；两者均保留全部检查。`build` 与 `run` 的产物进入内容寻址缓存（键覆盖生成的 C、模式、C 编译器标识、编译器版本与平台），相同输入直接复用可执行文件；`AE_CACHE` 覆盖缓存位置，`cache dir|clear|stats` 管理。`run` 在系统临时目录中编译并执行；`build` 写入指定路径，拒绝覆盖已有文件。Linux 上可将输出命名为 `arithmetic`。

生成的 C 只保留本程序可达的运行时函数（按符号引用做保守的传递闭包），所以 `emit-c` 的字节数随所用内建变化，不要再把它当作固定值；`deps` 输出模块依赖图与公开接口摘要哈希，改函数体不改变摘要、改 `pub` 签名会改变，但它**尚未参与缓存失效判定**，用户模块仍是同一个翻译单元。生成 C 还会在程序完全不触及 Win32 层时省掉 `<windows.h>`（ADR-39）：用到文件、目录、时钟、环境变量、工作目录或子进程时照常保留，纯计算程序不再解析它；这不改动任何声明、不引入 ABI 假设，两平台生成的 C 文本相同，收益只体现在 Windows。规则与实测取舍见 [当前语言语义](docs/current-language.md) 与 ADR-38/39。套件有四个负载：`sum`、`text`（物化 token：`split`/`strings.Split`）、`textlazy`（惰性扫描：`tokenize`/手写索引扫描/Rust 的 `split(' ')`）与 `map`；`bench/tokens_timings.py` 单独对照同一程序的两种 token 形态并同时给出峰值内存。计时与峰值内存分两趟测量（采样器曾在 Windows 上把每个数字拉长约 15 ms）。`bench/build_timings.py` 可复跑冷构建、无修改、改实现、改接口四类场景的耗时；加 `--header-baseline` 会在同一次运行内把同一份生成 C 还原 `<windows.h>` 后交替计时，避免跨会话的墙钟数字不可比。

带约束的泛型函数还会在**声明处**接受能力覆盖检查（ADR-40）：把约束白名单的交集作为有效类型集，若函数体对 `T` 用了某个有效类型不支持的运算，就在类型参数声明处报错。例如 `fn f[T: Copy](a: T, b: T) -> T { return a + b; }` 会被拒绝（`Copy` 允许 `bool`），改成 `T: Add` 即可。这是**收紧**：过去只在实例化时才报错；它只拒绝、不放行，逐实例化完整类型检查保持不变，无约束泛型行为不变，**不是 trait/impl**。`bench/constraint_audit_timings.py` 用两份同字节数的源码交替测 `check`，并断言两者 `emit-c` 输出逐字节相同——本轮不改变任何生成 C、可执行文件或缓存。

编译器前端按源文件共享词法结果（ADR-41）：`GenericFn` 不再为每个泛型声明复制整份 token 流与源码，含大量泛型声明的文件 `check` 明显变快，生成 C 与诊断完全不变。`bench/shared_tokens_timings.py` 用改动前后的两个二进制在同一脚本内交替测 `check`，并断言两者 `emit-c` 逐字节相同、声明处诊断一致；`bench/constraint_audit_timings.py` 则用于度量能力审计本身的上界。

`run` 支持把程序参数放在 `--` 之后转发给被运行的程序（`args()` 才能看到它们）：`cargo run -- run examples/find_files.ae -- examples txt`。设置 `AE_TIMINGS=1` 输出前端、C 编译与链接耗时及缓存命中情况；设置 `AE_ALLOC_LIMIT=N` 把运行时的累计分配字节数限制在 N，用于确定性地验证分配失败诊断（超出后按「out of memory」终止并给出位置，不是内存配额）；`check` 只做词法、语法与语义检查，不需要 C 编译器。

`check --json` 是给编辑器的稳定接口（ADR-48）：stdout 恰为一个 JSON 数组，成功是 `[]` 并以 0 退出，失败是一个含 `file`/`line`/`column`/`message` 的对象并以 1 退出；无法定位的失败把整条消息放进 `message`、位置为 `null`，不猜位置。数组的**形状**是契约、长度不是（前端目前 fail-fast，所以至多一个元素）。`message` 是展示文本，不保证逐字稳定，请按 `file`/`line`/`column` 分支。默认 `check` 的单行人类可读输出与退出码不变。

生成的 C 只包含程序真正用到的运行时函数**及其注释**（ADR-50）：函数上方的 `//` 说明随函数一起被裁剪，`print(1)` 的生成 C 因此从 25,134 降到 11,105 字节，端到端冷构建实测 390.66 → 346.12 ms；`/* */` 分节注释与 `win32-header` 标记不受影响。Windows 上 `ae build` 也能处理绝对路径含非 ASCII 的输出（ADR-51）：C 编译器在临时目录内以相对文件名运行，绕开 MinGW 命令行按 ANSI 代码页转换导致的 `cannot open output file`。

验证生成的 C：`cargo build` 后运行 `python scripts/verify_generated_c.py`；Linux 可加 `--sanitize` 启用地址和未定义行为检测，MinGW 无 libasan 时用 `--leak` 走 `-Wl,--wrap` 的分配计数（要求每个程序的退出分配块数等于空程序基线）。检测覆盖有限案例，不能证明实现完全安全。

现有子集的完整规则见 [当前语言语义](docs/current-language.md)；所有权、借用与释放的正式规范见 [资源模型](docs/resource-model.md)；设计决策记录见 [docs/decisions.md](docs/decisions.md)；探索性测量工具和限制见 [基准说明](bench/README.md)。目前不能据此宣称性能优于 Go、Rust、Gojo 或 Python。

## 许可证

MIT，见 [LICENSE](LICENSE)。
