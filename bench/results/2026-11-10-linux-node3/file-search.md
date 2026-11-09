# 工作负载 B：文件搜索（Linux / InstaBox node3）

日期：2026-11-10 ｜ 输入：bench-input.txt（20000 行，约 1.4 MB，固定种子生成）
语义：统计行数与包含子串 `ae` 的行数。四方输出完全一致：`20000` / `12140`。

版本固定：AE 提交 `8bf8000`+（本目录归档时的工作树）；Gojo `b460b17`（"Report where a segfault happened"，`git clone --depth 1`）；Go 1.27.1；rustc 1.99.0；GCC 14.2.0。机器：4 vCPU / 7.8 GiB / Debian 13。

## 结果（中位数，7 次执行 / 3 次构建）

| 语言 | 纯执行 | 冷构建 | 热构建 | 产物大小 |
|---|---|---|---|---|
| AE（-O2） | 5.81 ms | 43 ms | 2.5 ms（缓存命中） | **16,888 B** |
| Gojo（默认优化） | 3.74 ms | **28 ms** | 28 ms | 230,904 B |
| Go | 2.84 ms | 116 ms | ~116 ms | 2,490,891 B |
| Rust（-O） | **2.35 ms** | — | — | 4,546,568 B |

`gojo run`（编译+运行）35.5 ms；`ae run` 热缓存 25.0 ms；`go run` 37.0 ms。

## 公平性披露（方案 §17）

- **执行差距来自标准库而非编译器**：Gojo 的 `str.contains` 是优化过的原生实现；AE 版本用语言内朴素逐字节扫描（`bytes()` + 双层循环）。这是库成熟度差距，如实记录。
- Go/Rust 用各自标准库的 `strings.Contains` / `str::lines + contains`；语义一致（按 `\n` 分行、子串匹配）。
- AE 热构建走内容寻址缓存；冷构建为全新空缓存。Gojo/Go 无缓存分层，其"热"即重复构建（工具链自身缓存状态不受控）。
- 单机、单负载、小输入；不构成"全面超越"结论。按方案 §7.3，这是第一份 Linux 文件搜索公开数据：AE 在产物体积（13.7× 小于 Gojo）与热构建速度上领先，冷构建与执行落后于 Gojo。

## 复现

程序：`grep.ae`（examples/）、`grep.gojo`、`grep.go`、`grep.rs`（本目录归档副本）。
命令：`ae run examples/grep.ae`、`gojo run grep.gojo`、`go run grep.go`、`rustc -O grep.rs && ./grep_rs`；输入由 `python -c` 固定种子（42）生成。
