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
- [x] 12. 四路差分模糊测试例行化并扩展语法（分支内 move 字符串的 Mixed 形状），40 seeds 0 failures
- [x] 13. 落地 borrow-checker-design §3 对抗矩阵（先探行为再写测试）：5 个 UI 拒绝例 + 1 个 run 正例，钉住 L0 基线供 L1 NLL 差分
- [x] 14. emit-mir 打印 releases 精化结果（Frees 专用渲染 + drops pass 双视图同步 + 字面量类型修正），证据链闭合
- [x] 15. emit-mir 具名 intrinsic 全量渲染：容器/文件/JSON/数学/路径等 ~60 种语句与表达式带操作数显示（push/store/sort/index/join/parse_int/math...），浮点与 bool 格式修正
- [x] 16. drops pass 精化 `?` 错误路径 release（propagate 先应用 move 再重写列表），probe 用例 6 个 release 减到 2 个
- [x] 17. 修复 movewalk `?` 截断 bug（二元/多操作数表达式第二个操作数起从未 walk）：recorder 完整记录 move，alloc workload 恢复 elision；Checked 与 parser agreement 经全套件确认
- [x] 18. M2.4 基准报告（bench/reports/2026-11-14-windows-drop-elision.md）：release 调用点结构性消除（alloc 2/3、straight 20000/20000），运行时差异在噪声内（诚实结论：价值在 C 形态而非秒数）
- [x] 19. fuzzer 语法补容器操作面（push/下标写/sort），40 seeds 0 failures
- [x] 20. M2.5 L1 第一步：borrowscope 拆分两类借用区域——loop/match 保持 scope 区域（union meet），view 借用改 NLL 区域（视图槽后向活跃性，最后使用即死），写检查用 suffix+live_out；3 个单元测试钉住区域数学，enforcement 仍留 parser（下一步）
- [x] 21. M2.5 L1 落地（ADR-0205）：BorrowKind 标签 + parser 延迟 string 写冲突 + 函数装配时 NLL 裁定（引入支配写点 ∧ 视图仍可读才拒绝）；基线两例翻转为允许（run 语料），新增拒绝例 ui 快照；current-language/resource-model 同步
- [x] 22. L1 move 放宽（§3.3）：move 也延迟裁定（verb 区分文案）；裁定目标 = 写点 ∪ move 效果；walk_statement 的 Declare/Assign/Return 臂改 walk_moving_expr 修复 recorder 长期漏记；borrow_move_after_last_use 翻转 run 允许例 + borrow_move_during_view 拒绝例

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
- 2026-11-14 12:45 四路差分（MIR/drops vs tree vs release vs interp）40 seeds 全过；fuzzer 语法新增"分支内按值调用 move 字符串"形状（M2.4 Mixed 站点，seed 5 实测 kept_mixed=1），ADR-0204 证据同步更新。
- 2026-11-14 13:15 落地 borrow-checker-design §3 对抗矩阵测试（M2.5 L0 基线）：别名对读写（§3.1）、last-use 后写入（§3.2）、last-use 后 move（§3.3）、视图逃逸（§3.6 反例）、元素借用后容器写入（§3.8）五个 UI 拒绝快照 + 别名/回边正例 run 测试；全部先经编译器实测探明行为与诊断文本再固化。cargo test 273 全绿。
- 2026-11-14 13:50 emit-mir 补上 releases 渲染：Frees 从 stmt() 占位符改为 releases(v0 (string), ...) 专用 intrinsic；drops pass 同步改写 statements 与 bindings 两个视图（此前只改 statements，打印图会显示 C 里不存在的 release）；expr_type 给字符串形状表达式标注真实类型。新增 emit_mir_shows_which_releases_survive_drop_elaboration，ADR-0204 证据更新，cargo test 274 全绿。
- 2026-11-14 14:20 emit-mir 具名渲染全量化：语句 catch-all 从裸 stmt() 改为带 slot+操作数的具名 intrinsic（push/pop/store/array/sort/reverse/put/assert/bind_payload/propagate/break/continue/print），表达式 catch-all 覆盖全部内建（index/join/split/slice_view/tokens/len/contains/parse_int/trim/math/文件与 JSON 族/map 族/时间/env/neg/not...），未处理形状保留裸占位符作为 tripwire；Float 操作数用 Debug 格式保住小数点；sort/reverse 的 bool 实为 strings 标志改编码进名字；math 调用打印源名与 f64 类型；Result 型内建 temp 标注真实枚举类型。cargo test 274 全绿，verify_generated_c 41 PASS。
- 2026-11-14 14:45 drops pass 覆盖 `?` 传播：rewrite 对 Propagate 先应用值表达式的 move 效果、再按该状态重写错误路径 release 列表（错误路径直接退出函数）、最后应用 Ok 绑定的 revive；语句本体恒保留。probe 用例（concat 消耗 s 后 `?`）错误路径与 scope-end 各消 1 个死 release，MIR 后端 2 个 release 对 tree 后端 6 个；新增 mir_drop_elision_thins_propagate_error_path_releases，ADR-0204 证据更新，cargo test 275 全绿。
- 2026-11-14 15:15 修复 movewalk 长期潜伏的 `?` 截断：普通读取返回 None 被当作提前退出，二元表达式/多参数 Call/Store/树形控制语句的后续子树从未被 walk。改为 if-let 模式（违例传播、正常继续）。recorder 现在完整记录 Plus 链与多参数调用中的 move（alloc workload main: kept=1 elided=2）；同时修复 propagate rewrite 污染继续路径状态的 bug（错误路径 release 用状态副本重写），该 bug 由 mir_drop_elision_thins_propagate_error_path_releases 测试当场抓住。cargo test 275 全绿，fuzz 40 seeds 0 failures，verify_generated_c 41 PASS。
- 2026-11-14 15:35 M2.4 基准报告落地：drop_elision_timings.py 双 workload（alloc/straight）× 双后端 11 轮交错计时，stdout 全程校验一致；结构性结果精确（alloc 1/3、straight 0/20000 个 release 调用点），运行时中位数差异 ~2%（alloc）与持平（straight）——如实记录为「价值在 C 形态而非秒数」，原始 JSON 存 bench/results/。ADR-0204 证据更新。
- 2026-11-14 15:50 fuzzer 语法新增向量操作面（构造+push+下标写+sort+读），四路差分覆盖容器 lowering 的索引检查与排序确定性；40 seeds 0 failures。
- 2026-11-14 16:05 M2.5 L1 地基：movewalk 新增 Reads tracker（statement_reads/expr_reads，按求值序收集读取）；borrowscope 拆分为 ScopeRegions（loop/match/for-tokens 借用，union meet 不变）与 NllRegions（slice_view/tokens 视图借用的后向活跃性——视图不可重赋值故无 kill），写检查对视图借用改用「写语句之后的后缀读取 ∪ 后继块入口活跃」判定；3 个手工构造 CFG 的单元测试钉住区域数学（冲突/最后使用后允许/循环容器 scope 保持）。纯计算切片，parser enforcement 未动，cargo test 278 全绿。
- 2026-11-14 17:30 L1 move 放宽：expression.rs 移动路径延迟裁定（verb 区分 assign/move），裁定目标 = 写点 ∪ statement_move_effects.moved（move 把缓冲转给新所有者、可能先死，故视图仍可读即拒绝）；walk_statement 的 Declare/Assign/Return 臂与 CFG 终结器改 walk_moving_expr，修复 recorder 长期漏记 declare/assign/return move 的盲区（drop elision 与 Checked 同步受益）；两处测试期望更新为 NLL 文案（拒绝保持）。cargo test 278 全绿，fuzz 40 seeds，verify 41 PASS，已推送 574113c。
- 2026-11-14 17:05 M2.5 L1 第一切片落地：parser 的 scalar 赋值在视图借用下不再当场拒绝，延迟到函数装配时由 borrowscope::adjudicate_deferred 裁定（NLL 区域 + 支配关系：引入支配写点且视图仍可读才拒绝，写后创建的视图永不冲突）；容器借用保持当场拒绝；BorrowKind{View,Container} 标签贯穿 13 处触碰点。borrow_write_after_last_use/alias_write 两例按基线翻转为 run 允许例，新增 borrow_write_during_view 拒绝例；current-language.md 与 resource-model.md 同步；3 个单元测试钉区域数学。期间抓到并修复 live_at_write 一次性计算的粒度混装 bug。cargo test 278 全绿，fuzz 40 seeds 0 failures，verify_generated_c 41 PASS。
- 2026-11-14 17:45 L1 字段写放宽：结构体字段赋值（FieldStore）在视图借用下也延迟裁定——字段写释放被视的字节，变量粒度的覆盖规则与整体写一致（ADR-52 视图借的是整个变量）；field_views/tokens 两处期望更新为 NLL 文案（拒绝保持）。cargo test 278 全绿，fuzz 40 seeds，verify 41 PASS。
