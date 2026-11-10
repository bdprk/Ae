# AE

AE 是自主研发的实验性原生编程语言，首批目标平台为 Windows 和 Linux x86-64。目标是以 Go 的编译速度、Rust 的运行性能和安全为参照，尽量降低 CPU、内存和磁盘占用；通用能力与生态是次一级但长期必须建设的目标（见 [设计路线](docs/design.md)）。

优先级：安全和正确性是底线；运行 CPU 和内存共同优化；编译速度和磁盘占用都必须满足未来基于基准设定的预算。功能可以为这些目标让路。

当前原型支持的子集：`fn main()` 与用户函数（`i64`/`bool`/`string`/`vec` 参数与返回值，含 `&vec` 只读借用与 `&[i64; N]` 数组借用）、`let`/`var`、赋值、整数与布尔字面量、字符串字面量与转义、括号、`+ - * / %` 与一元 `-`/`!`、受检算术（溢出、除零报告位置并退出）、比较（含字符串内容相等）与 `&&`/`||` 短路、`if`/`else if`/`else`、`while`、`for i in a..b`（半开区间，界只求值一次）、`break`/`continue`、`assert`、`print`、`len`、固定长度数组与**动态向量 `vec`**（`push`/`pop`/索引，全部边界检查）、**拥有型字符串与向量的移动语义与确定性释放**、**模块（`use` 导入、`pub` 显式导出、环检测、跨文件重名拒绝）**、**结构体（拥有字段、字段借用、确定性释放）**、**枚举与穷尽 `match`**（变体负载 `i64`/`bool`/`string`，负载随枚举确定性释放）、**错误传播 `?`**（`let x = call()?;` 与 `call()?;`，失败时释放已绑定值并原样返回给调用者）、**可恢复错误**（预置 `ReadResult`/`WriteResult`/`IntResult`/`DirResult`/`FileResult`/`StatusResult` 与 `try_read_file`/`try_write_file`/`try_list_dir`/`try_file_info`/`parse_int`/`to_string`，失败作为值返回而非终止进程）、**目录遍历与路径查询**（`try_list_dir`/`path_exists`/`path_is_dir`）、**路径代数**（`path_join`/`path_name`/`path_parent`，两平台统一 `/`）、**文件整理操作**（`try_copy_file`/`try_rename_path`/`try_remove_file`/`try_create_dir`/`try_remove_dir`，全部可恢复）、**文件元数据**（`try_file_info` 返回字节数、Unix 秒时间戳与目录标志）、**哈希表 `map[string, i64]`**（`map_new`/`map_get`/`map_has`/`map_remove`/`keys`，缺失键返回 `Err` 而不是哨兵 0，`keys` 按字节序复制）、**只读借用参数**（`&string`/`&vec[string]`，不移动不复制）、**UTF-8 字符边界**（`slice` 不得切开字符，`utf8_len`/`utf8_prefix` 按码点计数与截断）、**`for x in vec` 元素迭代**、**文件读写 `read_file`/`write_file`（Unicode 路径）**、`bytes()`/`contains()`/`slice()`/`split()`/`join()`/`lines()`/`count_lines_with()`/`args()` 与 `vec[string]` 字符串向量与**借用字符串视图**（`slice_view` 零拷贝切片，借用跟踪）。表达式从左到右求值；字符串与向量赋值/传参/返回按移动处理，移动后使用被编译期拒绝，并有运行时空检查兜底；块退出、提前返回与跳转路径全部插入幂等释放。

诊断显示行列号；尚无泛型、并发或正式安全证明，不能据此宣称完整的内存安全语言已完成。

## 使用

需要 Rust 和支持 C11 的 GCC；使用 `AE_CC` 指定其他兼容的编译器可执行文件。Windows（GCC/MinGW）与 Linux（GCC）均已原生验证：双平台全部测试、净化器检查与示例输出一致（见[平台支持矩阵](docs/platform-matrix.md)）。

```sh
cargo test
cargo run -- check examples/hello.ae
cargo run -- emit-c examples/hello.ae
cargo run -- run examples/hello.ae
cargo run -- run examples/enums.ae
cargo run -- run examples/concat.ae
cargo run -- run examples/sum_numbers.ae -- 文件路径
cargo run -- run examples/find_files.ae -- . txt
cargo run -- run examples/file_tidy.ae
cargo run -- run examples/copy_notes.ae
cargo run -- run examples/word_count.ae
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
cargo run -- version
cargo run -- cache stats
```

默认构建使用 `-O0`，`run` 或 `build` 末尾加 `--release` 使用 `-O2`；两者均保留全部检查。`build` 与 `run` 的产物进入内容寻址缓存（键覆盖生成的 C、模式、C 编译器标识、编译器版本与平台），相同输入直接复用可执行文件；`AE_CACHE` 覆盖缓存位置，`cache dir|clear|stats` 管理。`run` 在系统临时目录中编译并执行；`build` 写入指定路径，拒绝覆盖已有文件。Linux 上可将输出命名为 `arithmetic`。

`run` 支持把程序参数放在 `--` 之后转发给被运行的程序（`args()` 才能看到它们）：`cargo run -- run examples/find_files.ae -- examples txt`。设置 `AE_TIMINGS=1` 输出前端、C 编译与链接耗时及缓存命中情况；设置 `AE_ALLOC_LIMIT=N` 把运行时的累计分配字节数限制在 N，用于确定性地验证分配失败诊断（超出后按「out of memory」终止并给出位置，不是内存配额）；`check` 只做词法、语法与语义检查，不需要 C 编译器。

验证生成的 C：`cargo build` 后运行 `python scripts/verify_generated_c.py`；Linux 可加 `--sanitize` 启用地址和未定义行为检测，MinGW 无 libasan 时用 `--leak` 走 `-Wl,--wrap` 的分配计数（要求每个程序的退出分配块数等于空程序基线）。检测覆盖有限案例，不能证明实现完全安全。

现有子集的完整规则见 [当前语言语义](docs/current-language.md)；设计决策记录见 [docs/decisions.md](docs/decisions.md)；探索性测量工具和限制见 [基准说明](bench/README.md)。目前不能据此宣称性能优于 Go、Rust、Gojo 或 Python。

## 许可证

MIT，见 [LICENSE](LICENSE)。
