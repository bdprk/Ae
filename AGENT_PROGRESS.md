# AGENT_PROGRESS.md

## 目标（用户原话）
@[d:\用户\trevv\桌面\ae] @[d:\用户\trevv\桌面\自主编程语言完整方案.md] 继续

## 验收标准
1. `tests/run/enum_generic_impls.ae` 语义与借用规则一致（修复 value receiver move 问题，如单独绑定或克隆/重新构造），使测试套件中所有 207 个 execution 测试全部通过。
2. `cargo test` 0 失败，全绿通过。
3. `agent-failure-log/2026-11-13.md` 记录本次排查原因与教训。
4. 依据规则 1，将修复与更新 commit 并 push 到 GitHub 远端 `origin/master`。

## Checklist
- [x] 1. 修复 `tests/run/enum_generic_impls.ae` 中 string payload 枚举 receiver move 冲突并清理 `declarations.rs` 调试打印
- [x] 2. 补齐 ADR-0196 文档与泛型枚举 `&self` 借用拒绝检查、接入 `borrowscope` 返回值验证
- [x] 3. 完善 M2.2 MIR 降级与 `emit-mir` 命令验证（消除编译警告，支持 `ClosureLit` 降级，编写 ADR-0197 与回归测试）
- [x] 4. 推进 M2.10 MIR 常量传播与死代码消除（Constant Propagation / Folding pass over `MirFunction` + ADR-0198）
- [x] 5. 推进 M2.11 MIR 解释器（`ae test --interp` / `ae run --interp` 基础执行引擎与 UB/除零/溢出检测 + ADR-0199）
- [x] 6. 推进 M2.10/M2.11 三方差分模糊测试（扩展 `scripts/diff_fuzz.py` 对齐 `--debug` / `--release` / `--interp` 三路输出 + 集成测试与 ADR-0200）
- [x] 7. 推进 M2.10 MIR 不可达基本块消除（Unreachable Block Elimination / CFG 简化 pass + 回归测试与 ADR-0201）
- [x] 8. 推进 M2.10 MIR 线性基本块合并（单前驱融合 `mir::merge_linear_blocks` + 回归测试与 ADR-0202）
- [x] 9. 完成 M2.3 C 后端从 MIR 块图发射（match/for 完全下放 + 四路差分 + ADR-0203，提交 4d259a8/8d6dc58）
- [x] 10. 完成 M2.4 MIR drop 精化（`mir::elaborate_drops` 数据流消除已证死的 release，Mixed 保守保留 + MoveTracker 共享 move 语义 + 2 个回归测试 + ADR-0204）
- [x] 11. 修复 ADR-0196 引入的 `Self::` 普通枚且回归（examples/enum_methods.ae 编译失败），verify_generated_c.py 41 例全过

## 日志
- 2026-11-13 15:57 初始化 AGENT_PROGRESS.md，定位到 4 个失败的 execution 测试均由 enum_generic_impls.ae 的 receiver move 引起。
- 2026-11-13 16:45 清理 declarations.rs 调试输出，更新 enum_generic_impls.ae/.stdout，cargo test --workspace --locked 全部通过（207+3+21+2+20=253 tests green），已写入 agent-failure-log/2026-11-13.md 第 8 条。
- 2026-11-13 17:01 补齐 docs/adr/0196-generic-enum-impls.md，修复泛型枚举 impl 中 `&self`/`&mut self` 诊断检查，将 loader.rs 中 borrowscope::verify_program_borrows 返回值正式接入校验，提交 35afbdb 并推送到 origin/master。
- 2026-11-13 17:15 完善 M2.2 `mir.rs`/`cfg.rs`/`mod.rs`，消除全部 8 个编译器警告，新增 `emit_mir_lowers_closures_and_aggregates` 测试与 ADR-0197，提交 420862d 并推送到 origin/master。
- 2026-11-13 17:25 完成 M2.10 `mir::optimize_function`（块内常量传播、带溢出/除零保护的受检常量折叠、常量分支折叠为 Goto、无用临时变量消除），新增 `mir_constant_propagation_folds_checked_arithmetic_and_branches` 测试与 ADR-0198。
- 2026-11-13 17:58 完成 M2.11 进程内解释器 `crates/ae-compiler/src/frontend/interp.rs` 并接入 `ae run --interp` 与 `ae test --interp`，新增 `mir_interpreter_executes_programs_and_tests_with_ub_detection` 测试、ADR-0199 及失败日志第 9 条，提交 8c9f033 并推送到 origin/master。
- 2026-11-13 18:02 完成 M2.10/M2.11 三方差分测试（扩展 `scripts/diff_fuzz.py` 同时校验 `--debug`/`--release`/`--interp` 15 seeds 全过，新增 `three_way_differential_agreement_between_debug_release_and_interp` 测试与 ADR-0200）。
- 2026-11-13 22:35 完成 M2.10 线性基本块合并：`mir::merge_linear_blocks`（仅融合"前驱以 `Goto` 跳转、后继只有这一个前驱、目标非入口块"的边，保留前驱块 id、按序拼接绑定并继承后继终结符，循环至不动点），接入 `simplify_cfg` 的 thread→prune→merge 固定点；常量传播用例的 main 由多块 `goto` 链降为 1 块，循环形状不变，新增 `mir_linear_block_merging_fuses_single_predecessor_successors` 测试与 ADR-0202、失败日志第 10 条；`cargo test --workspace --locked` 269 项全绿。
- 2026-11-13 22:03 完成 M2.10 不可达基本块消除：`mir::simplify_cfg`（入口可达性裁剪 + 空 `Goto` 跳板线程化含环保护 + 同目标分支折叠且要求条件无副作用 + 块 id 不重编号，`validate` 改为按 id 集合校验），新增 `mir_unreachable_block_elimination_prunes_abandoned_continuations` 测试与 ADR-0201；两臂 return 的函数从 6 块降到 3 块，`cargo test --workspace --locked` 268 项全绿。
- 2026-11-14 11:50 完成 M2.4 drop 精化：`drops.rs` 对优化后的 MIR 块图做 per-block/per-slot 数据流（Live/Uninit/Dropped/Mixed），有证明才消除 release（Uninit/Dropped 移除、Mixed 保守保留依赖幂等 release，修复 WIP 中"拒绝合法程序"的 internal error）；`movewalk` 重构为 MoveTracker trait（verifier/recorder 共享一份 move 语义），emit-mir 打印精化后的图；新增 2 个回归测试与 ADR-0204，失败日志新增 1 条。修复后 `cargo test --workspace --locked` 272 项全绿。
- 2026-11-14 12:25 修复 `Self::` 普通枚举关联函数解析回归：expression.rs 的 Self 枚举臂在无 enum_origin 时回退 associated_call（与裸名路径共用 inherent 查找），examples/enum_methods.ae 恢复编译运行；verify_generated_c.py 41 PASS / 0 FAIL（此前从未跑过，由本次例行化引入并立刻抓到该回归）；新增正例+拒绝例回归测试，失败日志新增 1 条。`cargo test --workspace --locked` 273 项全绿。
