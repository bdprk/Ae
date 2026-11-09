# AE

AE 是自主研发的实验性原生编程语言，首批目标平台为 Windows 和 Linux x86-64。目标是以 Go 的编译速度、Rust 的运行性能和安全为参照，尽量降低 CPU、内存和磁盘占用；通用能力与生态是次一级但长期必须建设的目标（见 [设计路线](docs/design.md)）。

优先级：安全和正确性是底线；运行 CPU 和内存共同优化；编译速度和磁盘占用都必须满足未来基于基准设定的预算。功能可以为这些目标让路。

当前原型支持的子集：`fn main()` 与用户函数（`i64`/`bool`/`string` 参数与返回值）、`let`/`var`、赋值、整数与布尔字面量、字符串字面量与转义、括号、`+ - * / %` 与一元 `-`/`!`、受检算术（溢出、除零报告位置并退出）、比较与 `&&`/`||` 短路、`if`/`else if`/`else`、`while`、`for i in a..b`（半开区间，界只求值一次）、`break`/`continue`、`assert`、`print`（整数/布尔/字符串/字面量）、`len`（数组编译期常量、字符串字节数）、固定长度整数数组与边界检查、只读数组借用参数、**拥有型字符串的移动语义与确定性释放**。表达式从左到右求值；字符串赋值/传参/返回按移动处理，移动后使用被编译期拒绝，并有运行时空检查兜底；块退出、提前返回与跳转路径全部插入幂等释放。

诊断显示行列号；尚无模块、泛型、动态容器、并发或正式安全证明，不能据此宣称完整的内存安全语言已完成。

## 使用

需要 Rust 和支持 C11 的 GCC；使用 `AE_CC` 指定其他兼容的编译器可执行文件。当前 Windows 使用 GCC/MinGW 验证；Linux 支持尚待原生测试。

```sh
cargo test
cargo run -- check examples/hello.ae
cargo run -- emit-c examples/hello.ae
cargo run -- run examples/hello.ae
cargo run -- run examples/strings.ae
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

设置 `AE_TIMINGS=1` 输出前端、C 编译与链接耗时及缓存命中情况；`check` 只做词法、语法与语义检查，不需要 C 编译器。

验证生成的 C：`cargo build` 后运行 `python scripts/verify_generated_c.py`；Linux 可加 `--sanitize` 启用地址和未定义行为检测。检测覆盖有限案例，不能证明实现完全安全。

现有子集的完整规则见 [当前语言语义](docs/current-language.md)；设计决策记录见 [docs/decisions.md](docs/decisions.md)；探索性测量工具和限制见 [基准说明](bench/README.md)。目前不能据此宣称性能优于 Go、Rust、Gojo 或 Python。

## 许可证

MIT，见 [LICENSE](LICENSE)。
