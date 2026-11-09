# AE

AE 是自主研发的实验性原生编程语言，首批目标平台为 Windows 和 Linux x86-64。目标是以 Go 的编译速度、Rust 的运行性能和安全为参照，尽量降低 CPU、内存和磁盘占用。

优先级：安全和正确性是底线；运行 CPU 和内存共同优化；编译速度和磁盘占用都必须满足未来基于基准设定的预算。功能可以为这些目标让路。

当前只是最小原型：支持 `fn main()`、`let` 不可变变量、`var` 可变变量、赋值、非负 i64 字面量、括号、整数四则运算、固定长度整数数组、受限只读数组参数及边界检查、整数函数、递归、比较条件、`if` / `else`、`while`、`print` 和 `//` 注释。表达式从左到右求值，乘除优先，溢出和除零终止程序。当前只有 i64 一种值类型，没有通用所有权和借用检查、并发或正式安全保证，尚未做性能比较。诊断显示行列号。

## 使用

需要 Rust 和支持 C11 的 GCC；使用 `AE_CC` 指定其他兼容的编译器可执行文件。当前 Windows 使用 GCC/MinGW 验证；Linux 支持尚待原生测试。

```sh
cargo test
cargo run -- check examples/hello.ae
cargo run -- emit-c examples/hello.ae
cargo run -- run examples/hello.ae
cargo run -- run examples/arithmetic.ae
cargo run -- run examples/control-flow.ae
cargo run -- run examples/arrays.ae
cargo run -- run examples/functions.ae
cargo run -- run examples/borrow.ae
cargo run -- build examples/arithmetic.ae -o arithmetic.exe
cargo run -- run examples/arithmetic.ae --release
```

默认构建使用 `-O0`，`run` 或 `build` 末尾加 `--release` 使用 `-O2`；两者均保留整数检查。`run` 在系统临时目录中编译并执行，完成后尝试清理；`build` 写入指定路径，拒绝覆盖已有文件。Linux 上可将输出命名为 `arithmetic`。缓存和发布工具链属于后续工作。

现有子集的规则见 [当前语言语义](docs/current-language.md)。双平台自动测试配置已经添加，但 Linux 原生运行结果仍待验证。

参见 [项目目标与路线](docs/design.md)。

探索性测量工具和限制见 [基准说明](bench/README.md)。目前不能据此宣称性能优于 Go、Rust 或 Gojo。

设置环境变量 `AE_TIMINGS=1` 可将前端与代码生成、C 编译与链接的合并耗时输出到标准错误；它不会计入文件读取和程序执行，也不单独拆分链接。

`check` 只执行词法、语法和当前语义检查，不生成 C，也不需要安装 C 编译器；尚未实现的所有权和借用规则不在检查范围内。

验证生成的 C：运行 `cargo build`，然后执行 `python scripts/verify_generated_c.py`；Linux 可加 `--sanitize` 启用地址和未定义行为检测。检测覆盖有限案例，不能证明实现完全安全。
