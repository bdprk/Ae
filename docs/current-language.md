# AE 当前原型语义（未稳定）

本文只描述已经实现的子集，不代表未来语言最终设计。

## 词法与结构

源文件采用 UTF-8。标识符由 ASCII 字母或下划线开头，后续可包含数字；关键字保留（`fn let var print if else while for in return break continue assert true false use struct enum match`）。文件顶部可写 `use "相对路径.ae";` 引入其他文件：按导入文件所在目录解析、深度优先加载、环导入报错；函数名跨文件唯一，`main` 全程序唯一（库文件无需 main）。跨文件可见的顶层项必须写 `pub`（`pub fn`/`pub struct`/`pub enum`），未标记的项只在本文件内可见，见下文「模块与可见性」。空白分隔词元，`//` 注释延伸至行尾。字符串字面量用双引号，支持 `\n`、`\t`、`\r`、`\\`、`\"` 转义，不允许原始换行或 NUL。程序有且仅有一个 `fn main() { ... }`，也可定义其他函数。语句以分号结束。

## 值与计算

标量类型为有符号 64 位整数 `i64` 与布尔 `bool`；另有拥有型 `string` 与固定长度 `i64` 数组。整数字面量目前仅支持 0 至 9223372036854775807；一元负号与减法可产生负数。支持 `+ - * / %`，乘除模高于加减，同优先级左结合，一元 `-` 和 `!` 比所有二元运算结合更紧，操作数从左到右求值。除法与求余向零截断，余数符号同被除数。溢出、除零使程序非零退出并在标准错误输出运算符的行列位置；开发与优化构建均不关闭这些检查。

比较 `== != < <= > >=` 作用于两个 `i64`；`==` 与 `!=` 也可比较两个 `string`（按内容）；产生 `bool`；`&&` 与 `||` 短路求值（右操作数仅在左操作数不能决定结果时求值），`!` 取反布尔。比较不可直接链接（`a < b < c` 因类型错误被拒绝）。

`let x = 表达式;` 声明不可变变量；`var x = 表达式;` 声明可变变量。声明前不能使用变量，不能重复声明；赋值要求类型一致。花括号引入词法作用域，内层可读符合可变性规则的外层变量；内层变量离开块后不可见，不能遮蔽当前可见的名称。`print(x);` 以十进制（整数）、`true`/`false`（布尔）或字符串内容加换行写入标准输出；`print("文本");` 直接输出字面量；输出错误使程序非零退出。`assert(条件);` 在条件为假时以非零状态报告断言行列位置。

## 拥有型字符串与移动语义

`string` 值拥有堆上的字节缓冲。初始化、按值传参、赋值与 `return` **移动**所有权；移动后原变量在编译期不可再使用（分支内移动后，分支合并处保守视为已移动）。对已移动变量赋值是合法的重新初始化；同一语句内右侧不能移动赋值目标自己。`+` 连接两个字符串并消耗两个操作数；`print` 与 `len` 借用不消耗。`len(s)` 返回字符串的 UTF-8 字节数。

释放在编译期插入：块结束、提前返回、`break`/`continue`（仅循环体内部声明的字符串）、函数返回路径全部覆盖；释放操作幂等（释放后指针置空），即使分析器与生成代码出现分歧也不会双重释放或使用已释放内存——每次读取字符串变量前有运行时空检查作为最后一道防线。内存分配失败使程序报告位置后非零退出。循环体内部**不能把循环外的拥有值移出去**（`let t = s;`、`f(s)`、`return s`）：循环体会重复执行，下一轮将读到已移动的值，因此编译期直接拒绝，除非在循环体结束前重新赋值给它（见「控制流程」）。

## 控制流程

`if 条件 { ... } else { ... }`：条件必须是 `bool` 表达式；支持 `else if` 链，末尾 `else` 可省略。`while 条件 { ... }` 每次循环重新计算条件。`for i in 开始..结束 { ... }` 迭代半开区间：界只求值一次且自左向右，`i` 是循环作用域内的不可变 `i64`，隐藏的自增是受检算术（到达 `i64` 最大值会安全失败）。`for x in 向量变量 { ... }` 按值迭代元素：`i64` 向量里 `x` 是不可变拷贝，`vec[string]` 里 `x` 是元素视图；长度在循环开始时捕获一次。被迭代的向量在整个循环体内被借用，因此循环体内**移动或原地修改**它（`push`/`pop`/索引写）都在编译期拒绝——尽管长度快照让 `push` 在语义上可行，当前实现选择保守。`break` 立即退出最内层循环；`continue` 跳过剩余主体——在 `for` 中仍执行自增。`break`/`continue` 仅允许出现在循环内。三种循环都适用同一条移动规则：**循环体不得把循环外声明的拥有型值移出去**（`let t = s;`、`f(s)`、`return s`），除非在本次循环体结束前重新赋值给它——否则下一次迭代会读到已移动的值；诊断指向循环关键字。`while` 与两种 `for` 的循环体内部声明的拥有值在每轮结束时释放。

`exit(状态码);` 立刻终止进程，状态码必须是 `i64`（进程退出码按 `int` 截断，POSIX 上 shell 只看低 8 位）。终止前先刷新标准输出、再释放在场的全部拥有值——所以「打印后退出」不会丢最后一行，分配账目也保持平衡。`exit` 之后同块内的语句不可达但仍需通过解析。只经 `exit` 结束、从不返回值的函数可以写成任意返回类型而无需 `return`。

## 动态向量

`var v = vec[1, 2];` 创建拥有型可增长向量（`vec[]` 为空向量）。`push(v, x);` 与 `pop(v);` 是语句：要求可变局部变量、原地修改、不转移所有权；空向量 `pop` 报错。`v[i]` 读、`v[i] = x;` 写都按当前长度做边界检查（写入先检查下标再计算右侧）。`len(v)` 是运行时长度。向量与字符串一样拥有堆资源：赋值、按值传参、`return` 移动所有权；移动后使用编译期拒绝并有运行时空检查兜底；作用域退出与跳转路径幂等释放。`&vec` 参数是只读借用：可 `len`、可索引、可转借，不可写入、push/pop、解移动或越过被借值存活；调用点用 `f(&v)` 传入。向量元素是 `i64`；内置名 `vec push pop len read_file write_file bytes` 为保留名。

## 文件与字符串工具

`read_file(path) -> string` 读取整个文件为拥有字符串；`write_file(path, content) -> i64` 写入并返回字节数；两者借用参数，路径为 UTF-8（Windows 经 UTF-16 宽字符 API，非 ASCII 路径可用），失败以行列位置报告运行时错误。`bytes(s) -> vec` 把字符串按字节转为向量，用于字节级处理。`contains(s, sub) -> bool` 子串搜索（平台优化实现，借用两参数，空子串为真）；`starts_with(s, prefix) -> bool` 与 `ends_with(s, suffix) -> bool` 按字节比较前后缀（空串为真），因此判断扩展名要用 `ends_with(name, ".txt")`，`contains` 会把 `a.txt.bak` 也算命中；`trim(s) -> string` 返回去掉首尾 ASCII 空白（空格、`\t`、`\n`、`\r`、`\v`、`\f`）的拥有副本，它只在字符边界处裁剪，不会切坏多字节字符；`slice(s, start, end) -> string` 返回拥有副本，越界以位置报告。字符串现在携带长度（`len`/`slice` 为 O(1)），缓冲区仍以 NUL 结尾供 C 互操作；`len`/`bytes` 按长度精确计数（含嵌入 NUL）。`read_file`/`write_file`/`bytes`/`contains`/`starts_with`/`ends_with`/`trim`/`slice` 为保留名。

**目录与路径**：`try_list_dir(path) -> DirResult` 返回目录下的条目名（不含 `.` 与 `..`，按字节序排序），失败返回 `Err(描述)`；`path_exists(path) -> bool`、`path_is_dir(path) -> bool` 只做查询。三者都接受拥有字符串或视图。

## Unicode 与字符边界

`len` 与 `slice` 按**字节**工作（字符串是 UTF-8 字节序列，两者都是 O(1)）。为避免切出非法 UTF-8，`slice`/`slice_view` 在索引落在字符中间时终止并报告 `string index splits a UTF-8 character`——判定只看一个字节，仍是 O(1)；`start`/`end` 等于 `len` 视为合法边界。

按字符处理用两个函数：`utf8_len(s) -> i64` 返回字符数（码点数，即非「后续字节」的字节数），`utf8_prefix(s, n) -> string` 返回前 `n` 个字符的拥有副本（`n` 超过字符数则复制全部，负数为错误）。`utf8_len("中文 😀 abc")` 是 8 而 `len` 是 15；`utf8_prefix(..., 2)` 得到 `中文`。

规则只保证**不新引入**断裂序列：来自文件或 argv 的非法字节允许存在并被原样处理（`bytes`/`slice` 皆可）。字符按码点计，不处理组合字符与字素簇；`split`/`contains`/`lines`/`path_*` 仍按字节匹配分隔符（ASCII 分隔符不受影响）。`utf8_len`/`utf8_prefix` 为保留名。

## 固定长度数组

`let a = [1, 2];` 声明不可变数组，`var a = [1, 2];` 声明可变数组。数组元素是整数表达式，自左向右求值；数组长度为 1 至 4096，只允许声明时使用数组字面量，不支持数组整体赋值、按值传参、返回或嵌套数组。`a[i]` 读取，`a[i] = value;` 需要可变数组。下标须为整数，负数或大于等于长度时程序报告下标位置并以非零状态退出；写入先计算并检查下标，再计算右侧。`len(a)` 是编译期常量（要求裸数组变量）。优化构建保持相同检查。

## 函数

`fn add(a: i64, b: i64) -> i64 { return a + b; }` 定义函数；返回类型可为 `i64`、`bool`、`string` 或 `vec`；参数类型可为 `i64`、`bool`、`string`（按值，移动）、`vec`（按值，移动）、`&vec`（只读借用）、`&vec[string]`（只读借用字符串向量）、`&string`（只读借用字符串，绑定为视图）、枚举或结构体（按值，移动）或 `&[i64; N]`（只读借用数组），最多 64 个。`&string` 调用点直接写实参（`f(name)`、`f("literal")`、`f(view)`），不移动也不复制，函数体内按视图规则使用（含 `+` 拼接，会复制）。借用值没有独立存储：`let q = 借来的参数;` 被拒绝，借用只能作为参数或 `match` 绑定存在。实参从左到右求值；字符串实参移动所有权。函数可以在调用点之前或之后定义，支持自递归和相互递归。所有路径必须明确返回值；编译器目前保守地只认可直接 `return` 或 `if`/`else` 两个分支均返回。`main` 不接受参数或 `return`。`len` 是内置函数，不能被重新定义。递归深度受系统栈限制。

## 只读数组参数

函数参数可写为 `a: &[i64; 3]`，调用时用 `f(&values)` 显式传入长度匹配的局部数组或只读数组参数。借用只在调用期间存在，可转借给其他函数；被调用函数不能写入、保存或返回借用。它是一种受限的只读参数，不是完整的借用检查或所有权系统。下标仍执行边界检查。

## 工具链

`ae check|emit-c|run|build|cache|version`。`build` 与 `run` 的产物基于内容寻址缓存：键覆盖生成的 C、优化模式、C 编译器标识与其路径、AE 版本与目标平台；条目带 `字节数 + SHA-256` 的 manifest，命中前校验，损坏条目会被删除并重建；编译器身份无法查询时本次禁用缓存（`AE_TIMINGS=1` 会说明原因）。缓存命中与全量构建语义一致。更换 libc/SDK 等键外因素后应 `ae cache clear`。`AE_CACHE` 覆盖缓存位置，`AE_CACHE_LIMIT` 以字节为单位限制缓存总量（未设置或 `0` 表示不限制，超出后按最近使用时间删除整条条目），`cache dir|clear|stats` 查看与清理（`stats` 会显示当前上限）。`AE_CC` 指定 C 编译器；默认开发模式 `-O0`，`--release` 使用 `-O2`；两种模式保持相同语言语义与检查。`run` 把 `--` 之后的参数转发给被运行的程序（`args()` 可见，含 argv[0]）；其他子命令不接受程序参数，传入即报错。`AE_TIMINGS=1` 输出阶段耗时与缓存命中情况；`AE_ALLOC_LIMIT=N` 把运行时累计分配字节数限制在 N，用于确定性地验证分配失败诊断（超出后以「out of memory」终止并给出位置，不是内存配额，非数字值被忽略）。

## 字符串向量

`split(s, sep) -> vec[string]` 按分隔符切分（保留空段，Python 语义；空分隔符为运行时错误）；`join(parts, sep) -> string` 以分隔符连接（借用向量，总长先溢出检查）；`args() -> vec[string]` 返回程序参数（含 argv[0]；Windows 的 argv 为 ANSI 编码，如实标注）。类型书写为 `vec[string]`（`vec` 单写是 `i64` 向量）。`vec[string]` 是拥有型容器：赋值/传参/返回移动；`push` 移入字符串，`pop` 释放并移除末元素；`v[i]` 读与 `v[i] = s;` 写按当前长度做边界检查（写入先检查下标、释放旧元素、再移入新值）。**元素是只读借用**：不能把元素移出向量（拼接或赋给新变量被拒绝）；`len(v)` 为 O(1)。`split`/`args`/`join`/`lines`/`count_lines_with` 为保留名。`lines(s) -> vec[string]` 按换行切分：尾部换行不产生末尾空行，内部空行保留，空输入为零行（换行扫描为运行时字长优化实现）。 `count_lines_with(text, pattern) -> i64` 统计包含模式的行数：单遍零分配的融合实现，空文本为零、空模式匹配每一行。

## 借用字符串视图

`slice_view(s, start, end) -> string view` 创建零拷贝借用视图：基必须是**裸字符串变量**，创建后该变量在视图存活期（当前作用域）内不可移动、不可重新赋值（借用跟踪器拒绝）。视图是非拥有类型：赋值/传参即复制借用，永不释放。视图支持 `len`（O(1)）、`contains`、`==`/`!=`（按内容）、`print`、`bytes`、`slice`（得到拥有副本）、`slice_view`（再借用），**可与字符串拼接**（`+` 会把视图复制进新字符串，视图本身仍然可用）；不可作为函数返回类型（无跨函数生命周期跟踪），也不能当作 `string` 传给按值接收的函数。越界视图以位置报告运行时错误。`for x in vec[string]` 按借用视图迭代元素（不可移出元素，容器在循环内不可修改/移动）。

## 模块与可见性

`use "lib.ae";` 只能写在文件顶部（`use` 之前只能有注释），路径相对**当前文件所在目录**解析，因此导入一个目录里的模块时它自己的 `use` 也按它自己的位置解析。加载是深度优先、同一文件只加载一次，环导入报错；`main` 全程序唯一，库文件不需要 `main`。

顶层项默认私有：只有带 `pub` 的 `fn`/`struct`/`enum` 才能被其他文件使用（`pub` 也可写在 `enum`/`struct` 之前）。文件总能看见自己的私有项，预置结果枚举对所有文件公开。跨文件重名仍然一律报错（尚未实现按文件隔离的私有命名空间，见限制），因此私有项也不能与其他文件里的名字冲突。

两条相关的检查：使用未导出的名称时报 `function 'x' is private to <文件> (mark it 'pub' to export it)`（结构体、枚举同理）；`pub` 函数的参数或返回类型若是本文件的私有类型，则在**定义处**报 `public function 'x' exposes private struct 'Y'`，而不是留给每个调用点去撞墙。

## 结构体

`struct Point { x: i64, y: i64 }` 定义命名字段序列；字段类型可为任意已注册类型（标量、string、vec、其他结构体）。自身引用在构造上即不可能（结构体名在其体内尚未注册），因此不存在无限大小布局。构造 `Point{x: 1, y: 2}`：字段可按任意顺序书写但各恰好一次，缺失/重复/未知字段被拒绝；字段表达式按声明序求值。后缀 `p.x` 读取：标量字段立即拷贝（后续释放不影响副本），拥有字段（string/vec/结构体）是**只读借用**——永远不能把字段移出结构体。`p.x = 值;` 字段赋值要求可变变量，覆盖前先释放旧拥有值。整体赋值 `p = 表达式;` 按移动语义：旧值先释放。结构体拥有其全部字段：赋值/传参/返回移动整个结构体，作用域退出、提前返回与跳转路径按类型生成的释放例程清理，绝不泄漏或双重释放。移动后使用由编译期移动集合拒绝（结构体无运行时哨兵，编译期检查是其防护，文档如实标注）。结构体名不能用作变量名；内置保留名同样不可作结构体/字段名外的顶层冲突。

## 可恢复错误与数字转换

程序预置八个结果枚举（无需导入，变体顺序固定，编号 0–7）：`ReadResult{Ok(string), Err(string)}`、`WriteResult{Ok(i64), Err(string)}`、`IntResult{Ok(i64), Err(string)}`、`DirResult{Ok(vec[string]), Err(string)}`、`FileResult{Ok(i64, i64, bool), Err(string)}`、`StatusResult{Ok, Err(string)}`、`BoolResult{Ok(bool), Err(string)}`、`CommandResult{Ok(i64, string), Err(string)}`。可失败内建：`try_read_file(path) -> ReadResult`、`try_write_file(path, content) -> WriteResult`（成功返回写入字节数）、`parse_int(text) -> IntResult`。失败返回 `Err` 负载的错误描述，**不终止进程**；`read_file`/`write_file` 仍是失败即终止的不可恢复版本。`parse_int` 严格按十进制：可选 `+`/`-`、只允许 ASCII 数字、整串必须消费完、正负分别以 `9223372036854775807`/`-9223372036854775808` 为界，其余情况均为 `Err`。`to_string(i64) -> string` 是十进制格式化，与 `parse_int` 构成往返。

结果值是普通枚举值：必须先用 `let` 绑定，再用 `match` 取出负载（`match` 不接受表达式主题）。错误文本随平台 `strerror` 语言变化，程序不应依赖精确内容。`try_read_file`/`try_write_file`/`try_list_dir`/`path_exists`/`path_is_dir`/`path_join`/`path_name`/`path_parent`/`try_file_info`/`try_copy_file`/`try_rename_path`/`try_remove_file`/`try_create_dir`/`try_remove_dir`/`parse_int`/`to_string`/`map_new`/`map_get`/`map_has`/`map_remove`/`keys`/`utf8_len`/`utf8_prefix`/`strvec_new`/`run_command` 与八个结果类型名均为保留名。

## 错误传播 `?`

`let x = 调用()?;` 与 `调用()?;` 是传播语句：结果枚举处于 `Err` 时，先释放当前函数内所有仍存活的拥有型变量，再把这个结果值本身返回给调用者；处于 `Ok` 时，`let` 形式把唯一的负载移出并绑定（`Ok` 必须恰好一个负载），裸语句形式则释放负载后继续。裸语句是语言里唯一的「表达式语句」，且**必须以 `?` 结尾**，因此不存在「静默丢弃返回值」的写法。

可传播的类型必须形如 `{ Ok(...), Err(string) }`（`Ok` 在前、`Err` 在后），八个预置结果枚举都满足；自定义枚举照此形状同样可用。传播要求外层函数返回**同一个**枚举类型，`main` 里不能传播；一个函数只能传播一个结果族，跨族（例如 `try_list_dir` 的 `DirResult` 与 `try_copy_file` 的 `StatusResult`）必须在边界处用 `match` 转换。`?` 是语句而非表达式，不能出现在更大的表达式里。

## 路径代数与文件元数据

路径函数是纯字符串操作，不触碰文件系统，也不做规范化（不解析 `.`/`..`、不折叠重复分隔符）。`path_join(dir, name) -> string` 在需要时插入 `/`：`dir` 以分隔符结尾则直接相接，`name` 是绝对路径（以 `/`、`\` 开头或形如 `C:`）时**整个替换** `dir`，任一参数为空则返回另一个的副本。`path_name(path) -> string` 返回最后一个分量（先丢弃尾部空分隔符，所以 `a/b/` 的 name 是 `b`；纯分隔符路径返回分隔符本身）。`path_parent(path) -> string` 返回最后一个分量之前的部分并去掉多余分隔符（`/a` 的父是 `/`）；没有分隔符的路径没有父，返回空字符串而不是编造 `.`。输出一律使用 `/`：Windows 接受 `/`，两平台打印同一路径文本，测试与记录才可比。

`try_file_info(path) -> FileResult` 返回元数据：`FileResult::Ok(size, modified, is_dir)` 中的 `size` 是字节数（i64）、`modified` 是**自 Unix 纪元起的秒数**（Windows 的 FILETIME 已换算成同一口径）、`is_dir` 表示是否为目录。路径不存在或无权限时返回 `Err`，**不会**以 0 大小静默成功。

## 文件整理操作

文件系统写入操作全部返回 `StatusResult`（`Ok` 不带负载，`Err(string)` 带原因），不会终止进程：

- `try_copy_file(from, to) -> StatusResult`：复制内容（64 KiB 分块），目标已存在则截断覆盖；不保留权限/时间戳（当前限制）。
- `try_rename_path(from, to) -> StatusResult`：同卷移动并覆盖已存在的目标文件（Windows `MoveFileExW` 带 `REPLACE_EXISTING`，POSIX `rename`）。
- `try_remove_file(path) -> StatusResult`：删除文件；**目录一律拒绝**（POSIX `remove()` 会删空目录，为两平台一致改为先判定并报错）。
- `try_create_dir(path) -> StatusResult`：按 `mkdir -p` 语义创建所有缺失的父目录；目录已存在不算错误。
- `try_remove_dir(path) -> StatusResult`：只删空目录，且**没有**递归删除接口——静默删除整棵树是最容易造成数据丢失的 API。

`for name in names` 与这些函数组合即可写出「列出→判定→搬移→清理」的文件整理流程，示例见 `examples/file_tidy.ae`。

## 排序、时钟与环境变量

`sort(v)` 与 `reverse(v)` **就地**重排可变向量：`i64` 向量按数值升序，`vec[string]` 按字节序（与 `keys()`、目录列表同一口径，所以输出在两平台一致）。空向量与单元素向量是空操作。`v` 必须是 `var` 变量，且不能在被借用期间（例如 `for x in v` 体内）调用；想反序就 `sort(v); reverse(v);`。

`now_s()` / `now_ms()` 返回 Unix 纪元以来的秒/毫秒（`i64`）。`now_s()` 与 `try_file_info` 的 mtime 同单位，可直接相减算出「多久以前」。

`env(name) -> ReadResult`：存在返回 `Ok(值)`，不存在返回 `Err("variable not set")`，**存在但为空**返回 `Ok("")`——「空」与「没有」是两件不同的事，混为一谈会让 `PATH=""` 悄悄变成「没有 PATH」。空名字报 `Err("variable name is empty")`。环境变量只读：没有 `set_env`，也没有枚举。

`try_append_file(path, text) -> WriteResult` 以追加模式写入并返回字节数；`try_write_file` 仍然整体覆盖。每次调用都会开合文件，写很多小行时应先拼成一个字符串再一次写入。`sort`/`reverse`/`now_ms`/`now_s`/`env`/`try_append_file` 为保留名。

## 标准输入与工作目录

`read_stdin() -> ReadResult` 读入**全部**标准输入并返回原始字节：输入为空返回 `Ok("")`，读取失败返回 `Err(原因)`。内容不做换行翻译（Windows 上先把 `stdin` 切成二进制模式），所以 `a\r\nb` 读回来是 4 个字节而不是 3 个——`len`、`bytes`、`split` 看到的就是磁盘/管道上的真实字节。读取时不校验 UTF-8，与 `read_file` 一致；`slice`/`utf8_*` 仍然按码点检查边界。整个输入会驻留内存，超大流不适合。

```ae
fn main() {
    let input = read_stdin();
    match input {
        ReadResult::Ok(text) => { print(len(text)); print(utf8_len(text)); }
        ReadResult::Err(reason) => { print("cannot read: " + reason); exit(2); }
    }
}
```

`cwd() -> ReadResult` 返回当前工作目录的绝对路径，分隔符统一为 `/`（与 `path_join`/`path_parent` 同一口径），可继续传给 `path_join` 或 `try_*` 系列。`getcwd` 会失败（目录被删除、权限变化），所以是结果值而不是裸字符串。不解析符号链接，也没有 `set_cwd`。

`grep_cli.ae` 与 `pipe_stats.ae` 是两个验收示例：前者把 `-` 当作「从标准输入读」，后者统计管道字节/字符/行/最长行（`cat notes.txt | ae run examples/pipe_stats.ae`）。

## 空字符串向量与子进程执行

`strvec_new() -> vec[string]` 造一个空的 `vec[string]`。`let v = vec[];` 推不出元素类型时会被当成 i64 向量，所以「先空后逐个 push」的字符串向量必须用它起步。

`run_command(program, args) -> CommandResult` 启动一个子进程并等它结束：`program` 是 `string`/视图，`args` 必须是**裸 `vec[string]` 变量**且未移动（不复制、不取视图，调用期间借用）。成功返回 `CommandResult::Ok(status, output)`，`status` 是退出码、`output` 是 stdout 与 stderr **合并**后的全部文本（不设上限，读完为止）；程序根本起不来（找不到文件、无执行权限）返回 `Err(原因)`——这是唯一算「失败」的情形。

```ae
fn main() {
    let argv = args();
    var command = strvec_new();
    for i in 2..len(argv) { push(command, argv[i]); }
    let result = run_command("git", command);
    match result {
        CommandResult::Ok(status, output) => { print(status); print(output); }
        CommandResult::Err(reason) => { print("cannot start: " + reason); exit(1); }
    }
}
```

退出码非零**不是** `Err`：子进程正常退出但报告失败，和你自己的程序 `exit(3)` 是一回事，把两者混在一起会让「命令没跑起来」和「命令跑了但失败了」无法区分。被信号杀死时 POSIX 报 `128 + 信号号`（与 shell 的 `$?` 一致）；Windows 直接给 `GetExitCodeProcess` 的退出码。

两个平台都**不**隐式经过 shell：`program` 被直接执行，所以 Windows 上想用 `echo`、`dir` 这类内建命令得自己写 `run_command("cmd", ["/c", "echo", ...])`。Windows 的参数规则取自 `CreateProcessW` 的引号约定，但故意留了保守边界：只有含空格/制表符的参数才加引号（给每个参数都加会让 `cmd` 把引号当成命令名的一部分），含 `"` 或以 `\` 结尾的参数直接报 `Err` 而不是猜——把 `CreateProcess` 的转义规则猜错会静默改变子进程收到的内容，那比启动失败危险得多。

子进程**继承**当前标准输入（两个平台都一样），所以它能读到的内容和你自己调 `read_stdin()` 抢的是同一个流，谁先读谁拿到——想让子进程完全不吃输入，就把它接到空文件上再启动本程序。除此之外没有可调项：不能设置环境变量、工作目录或超时。输出一次性读进内存，长时间高吞吐的命令会占满内存；需要流式处理时当前语言还不支持。示例见 `examples/run_tool.ae`（把自身参数转发给子进程）。

## JSON 路径查询

`json_get(text, path) -> ReadResult` 按路径取一个值：字符串会解码转义（含 `\uXXXX` 与代理对），数字与 `true`/`false`/`null` 返回原文，数组与对象返回原始片段。路径写成 `key.sub[2].name`，空路径表示整个文档，`[0]` 可以直接用在根数组上。

`json_int(text, path) -> IntResult` 只接受纯整数（有小数点或指数就报错，溢出 `i64` 也报错）；`json_bool(text, path) -> BoolResult` 只接受 `true`/`false`；`json_len(text, path) -> IntResult` 返回数组元素数或对象成员数；`json_has(text, path) -> bool` 只问存在与否（`null` 也算存在）。

```ae
fn port(config: &string) -> i64 {
    let found = json_int(config, "server.port");
    match found {
        IntResult::Ok(n) => { return n; }
        IntResult::Err(reason) => { print("no port: " + reason); return 8080; }
    }
}
```

三种失败是分开的：`no member named 'timeout'`（没有这个键）、`'name' is a string, not a number`（键在但类型不对）、`path segment [9] is past the end of the array`（越界）。路径自身写错（`a.`、`a[`、`a..b`）会在扫描文档之前报出来。

这是**读取器**而不是验证器：它只沿着路径检查语法，不校验整份文档；数字的严格性在 `json_int` 转换时才保证；重复键取第一个；键名里含 `.` 或 `[` 的字段无法用路径表达；没有写入与序列化。每次调用都会重新扫描文档，配置级输入足够快，同一字段反复查询应先取出存进变量。

## 哈希表

`var counts = map_new();` 建立一个空的 `map[string, i64]`——当前唯一的映射实例化：键必须是 `string`，值必须是 `i64`，类型书写为 `map[string, i64]`（可用于变量标注、参数、返回类型与结构体字段）。

- `counts[key] = n;` 插入或覆盖。键由运行时代为**复制**，因此视图（`&string` 参数、`vec[string]` 元素）与临时值都能直接作键；值是 `i64`，不涉及所有权。目标必须是可变映射（`var` 变量，或可变结构体变量的映射字段 `s.counts[key] = n;`），且在借用期间不可写。
- `map_get(m, key) -> IntResult`：命中返回 `Ok(n)`，缺失返回 `Err("key not found")`——「不存在」与「值为 0」因此可以区分，不会像 C 风格 API 那样用 0 冒充缺失。
- `map_has(m, key) -> bool`、`map_remove(m, key) -> bool`（返回是否真的删掉了）、`keys(m) -> vec[string]`（**复制**并按字节序排序，因此遍历顺序在两平台一致）、`len(m)` 返回条目数。

映射是拥有型值：赋值、按值传参、`return` 都移动整个表，作用域退出与跳转路径按类型生成的释放例程回收所有键与槽位数组，不会泄漏或双重释放。`map_get`/`map_has`/`map_remove`/`keys` 只在调用期间借用映射，接收者必须是**裸映射变量或它的结构体字段**（`map_get(t.counts, k)`），不能是更复杂的表达式；对已移动的映射调用会被编译期拒绝，运行时还有 `slots` 空指针哨兵兜底。

实现是开放寻址 + 线性探测（FNV-1a 哈希，`AE_MAP_EMPTY/FULL/DELETED` 三态，删除留墓碑以维持探测链），负载因子达 3/4 时扩容一倍并清除墓碑。限制：不能直接 `for k in m` 遍历（请用 `keys(m)`），值类型只有 `i64`，没有自定义哈希与容量预分配；`keys()` 会分配一整份副本。

## 枚举与模式匹配

`enum Shape { Circle(i64), Rect(i64, i64), Empty }` 声明枚举：变体可带 0–16 个位置负载，负载类型限于 `i64`、`bool`、`string`、`vec`、`vec[string]`（泛型枚举里还可以是类型参数；因此枚举不能自包含，不存在递归布局）。变体名在同一枚举内唯一，枚举名与结构体、内置名冲突时报错；与结构体一样，只能在同文件内先声明后使用。构造 `Shape::Circle(3)`、`Shape::Rect(4, 5)`、`Shape::Empty`；负载个数必须与声明一致，`string` 负载按移动传入。

`match 变量 { Shape::Circle(r) => { ... } Shape::Rect(w, h) => { ... } Shape::Empty => { ... } }` 是语句：主题必须是**裸枚举变量**，每个变体必须恰好出现一次（穷尽性；缺一个变体、重复匹配或写错枚举名都报错），不支持通配符与嵌套模式。分支绑定是只读的：`i64`/`bool` 负载按值拷贝，`string` 负载绑定为 `string view`，`vec` 绑定为 `&vec`，`vec[string]` 绑定为 `&vec[string]`——都不能被移出枚举，视图可用 `len`/`contains`/`slice`/`==`/`print`/`+`，借用向量可 `len`/索引/`for-in`/继续借用。匹配期间主题被借用：不能移动或整体赋值，改绑其他变量不受影响。分支体内的 `break`/`continue` 作用于最内层循环，而不是 `match`。

枚举是拥有型值：赋值、按值传参、`return` 都移动整个枚举，移动后使用被编译期拒绝；作用域退出、提前返回与跳转路径按当前变体释放负载，`string` 负载因此不会泄漏或双重释放。给枚举变量整体赋值时先释放旧负载再移入新值。枚举可用于函数参数与返回类型，也可作为结构体字段。枚举整体不能被 `print` 或比较。

## 泛型枚举与 `Option[T]`

`enum Either[T, U] { Left(T), Right(U), Both(T, U), Neither }` 声明泛型枚举：类型参数写在名字后的 `[T, U]`，只能是单个大写字母（`T`、`U`、`K`、`V`），最多 8 个；负载类型可以是类型参数，也可以是固定类型。泛型**声明**不产生任何布局，只有实例化才产生。

使用时要写出类型实参：`Option[string]::Some("x")`、`Option[i64]::None`、`Either[string, i64]::Both("n", 3)`。类型实参限于 `i64`、`bool`、`string`、`vec`、`vec[string]`——与负载限制一致，因此不存在递归布局。**同一个实参列表只产生一份布局**（`Option[string]` 在任何地方都是同一个类型，可自由赋值、传参、返回）；不同实参是不同的类型（`Option[string]` 与 `Option[i64]` 不能互换，各自的 `Some` 负载类型不同）。

`match` 的模式写**声明名**而不是实例化名：`Option[string]` 与 `Option[i64]` 的值都用 `Option::Some(v)` / `Option::None` 匹配，`v` 的实际类型由被匹配值的类型决定（`string` 负载仍按视图绑定）。穷尽性、借用规则、移动语义与普通枚举完全一致。

预置 `Option[T] { Some(T), None }` 对所有文件公开，意思是「可能没有的值」：`fn find(text: &string, needle: &string) -> Option[i64]` 比用 `-1` 或空串当哨兵清楚得多，因为哨兵无法与真实的 `-1`、真实存在的空行区分。注意 `?` 只作用于 `Ok`/`Err` 形状的结果枚举，`Option` 需要用 `match` 取出（见 [ADR-21](decisions.md)）。

每个程序最多 256 个枚举类型（含预置与全部实例化），超出时在实例化处报错；这是代码体积预算，不是最终限制。

## 泛型结构体

`struct Pair[K, V] { left: K, right: V }` 声明泛型结构体，类型参数规则与泛型枚举相同（单个大写字母、最多 8 个、不得重复）。声明本身不产生布局，`Pair[string, i64] { left: "a", right: 1 }` 实例化时才产生一个具体的结构体类型。

- 同一实参列表只产生一份布局：`Pair[string, i64]` 在任何地方都是同一个类型，可互相赋值、可作为同一个参数类型传给函数；`Pair[i64, i64]` 是另一个类型，混用会在调用处报类型不匹配。
- 类型实参可以是任何可命名的类型：`i64`、`bool`、`string`、`vec`、`vec[string]`、`map[string, i64]`、另一个结构体、一个枚举，或已实例化的泛型（`Pair[string, Pair[string, string]]`）。这里比泛型枚举宽松——枚举负载只接受扁平类型，因为枚举没有可变布局，而结构体字段本来就允许嵌套结构体。
- 字段访问、字段赋值、移动与释放与普通结构体完全一致：拥有型字段只能借用不能移出，`p.right = 值;` 覆盖前先释放旧值，作用域退出按实例化后的字段类型生成 release。
- 不存在递归布局：结构体名在自己的体内尚未注册，`struct Node[T] { next: Node[T] }` 报未知类型；实例化深度因此天然有限。
- 每个程序最多 256 个结构体类型（含普通结构体与全部实例化）；`pub struct Pair[K, V]` 可被其他文件导入，可见性规则与普通结构体相同。

泛型函数与 trait/约束尚未实现。

## 当前实现限制
编译器当前限制表达式嵌套深度及表达式节点数各为 128，块嵌套深度为 64，循环嵌套 64；超出时报告错误。这些是早期实现的资源限制，不是最终语言能力承诺。移动/所有权跟踪覆盖所有拥有型值：string、vec、map 与结构体变量共用同一套 moved 集合（编译器以 MoveKind::{Var, Field, StrVecElement} 记录），结构体字段与 vec 元素按设计不能被拆出、只能作为借用使用（具体编译期/运行时边界见上文）。比较不可链接；字符串不可变（无原地修改）；这些限制将在后续版本按规范逐步放开。
