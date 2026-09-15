# ANGE 真实任务验证：Stage R 尚不能通过整体验收

> 历史失败记录，数据与原判定保留。[后续修复报告](ANGE_FIX_REPORT.md) 已关闭这些
> 已定位缺陷，重新验收为 8/8；403 个 Rust 测试与 18 个变异控制通过。
> 增强关系优先仍没有胜过 source-first 的完整成本，不将修复通过混同为效率收益。

## 结论

**本轮有效性/采用判定：FAIL。**

新增机制有可复现的局部可靠性收益，但增强关系优先流程没有胜过已有工具的强组合，
而且在一题中信任了错误的调用位置计数。不能用之前的单元测试、固定数量或性能预算
PASS，推导“新机制整体更有用”。**先修复本轮红色控制，再考虑 impact；不扩大默认工具面。**

本轮只新增评测脚本与报告，**没有修改生产 Rust、原有测试或冻结答案**，没有提交、发布、
委派 agent，亦没有运行 ANGE 全项目构建或推荐测试选择器。

## 测的是什么

使用用户指定的 `/Users/allen/Documents/GodotProjects/ange`，但不直接修改主工作树：

- 本轮固定 commit：`24ceb20f158fe6a54eb5a1f6c132e14e3e533532`。
- 它不同于上轮性能验收的 `70fe1922…`；不能把两轮计数混在一起。
- 主树起始有 568 条 dirty/untracked 状态。本轮使用 `git archive` 的独立副本。
- 测试期间观察到主树 dirty 状态及 diff 变化，HEAD 不变；没有尝试恢复或覆盖。
  本轮写入目标全部是 cx 的评测目录或临时目录，实验语料不依赖这些外部变更。
  **不宣称用户主树前后字节一致。**
- 8 道未修改真实代码的机制导向 dev 题，另加真实文件上的漏刷新实验及两个微型 C++
  诊断控制。后面三类控制不计入 8 题分母。
- 源码先读，问题、必要答案、禁止连接、位置及预算先冻结，然后才运行新 commit 上的
  两个 binary。没有从候选输出生成 gold。
- 这是人为按机制选题的单仓库诊断，不是自然任务频率抽样，也不是 held-out。
  题目偏已知主体的代码审查，结果不能外推为所有开发任务的胜负。

### 三个流程

| 组 | 策略 |
| --- | --- |
| A | 旧版 relation-first，按能力/不确定性补 definition 或源码读取 |
| B | 旧版已有工具的 source-first 组合：已知函数先 definition，反向查找先 references，再按语法证据及生产路径筛选 |
| C | 新版 relation-first，保留分页、覆盖、不确定性，按同一规则补充读取/回退 |

B 不读整个仓库，也不使用新解析事实、gold 路由或排名。它就是“已知符号先 definition”
这一现有低成本阶梯。C 表示被测的**增强关系优先策略**，不是新版 binary 的所有用法；
新版仍保留这些廉价工具，不能据此声称新版的每一种使用方式都变慢了。

每题最多 12 个 CLI/read 操作、128 KiB 请求与 stdout、30 秒。完整计入分页、失败、
空 definition、撤销 scope 重试、源码复核。每臂隔离冷索引，warm 流程重复 3 次，
顺序轮换 ABC / BCA / CAB。不同重复不是新的独立题。

**“证据充分”不等于 agent 作答正确。** 没有启动 LLM 会话。本轮检验原始关系事实是否
正确，以及完整流程是否拿到了足够且原文一致的证据，供审查者回答问题；没有把 gold
复制成某个未运行 agent 的答案。源码可以让审查者纠正原始假边，但原始错误仍单独记录。

## 原协议与公开补充协议

冻结协议 v1 的结果完整保留：A、B 都在 R04 缺少 `cache()` 的类型证据；C 还在 R07
给出错误数量。原因是新旧版都未索引引用返回函数，`definition cache` 为空，而 v1 的
回退阶梯到此停止。

这不足以代表一个强的人工/agent 基线，因此**公开增加 v2 补充诊断**：对任何空 definition，
三臂统一读取题目原已指定的文件，最多 2,000 行 / 50 KiB；不按 gold 找行、不换题，
不改 Rust、不改答案。只在空结果出现时触发，所有额外成本照计。

v2 是看过 v1 后的协议修订，不冒充未见结果的预注册；原协议、原输出、v1 分数、修订说明
和新分数均保留。下面主表使用更强的 v2；v1 的对照表另列。

## 完整流程结果

### v2：强回退，全部尝试计入成本

每行是 8 题的一轮；调用次数和字节在三次重复中完全相同，wall 是三轮总时间的中位数。
请求字节采用统一的 CLI/read 请求 JSON 加 stdout；不是 Pi 原生 RPC 或模型 token。

| 组 | 证据充分任务 | CLI/read 操作 | 请求 + stdout | 总 wall 中位数 | 每个充分证据包的字节成本（包含失败尝试） |
| --- | ---: | ---: | ---: | ---: | ---: |
| A：旧版关系优先 | 8/8 | 19 | 45,805 B | 2.260 s | 5,725.6 B |
| B：旧工具 source-first | 8/8 | 12 | 26,363 B | 1.114 s | 3,295.4 B |
| C：增强关系优先 | **7/8** | 19 | **80,070 B** | **3.926 s** | **11,438.6 B** |

C 相比 B：操作数增加 7，通信字节约 **3.04 倍**，总 wall 约 **3.53 倍**，还丢失 R07。
相比 A，C 没有减少总操作数，字节约 **1.75 倍**，wall 约 **1.74 倍**。
不能宣称效率收益，也不能只挑 C 节省一次调用的 R06 来掩盖整组退化。

这些是本机三次重复的描述性结果，不做统计显著性或 n=8 的普适推断。
完整逐步 CPU、RSS、两路输出及所有重复保存在原始记录中；没有把 `bytes/4` 当作 token，
也未计入一个不存在的 agent 会话、工具 schema 固定上下文或服务端 RPC 开销。

### 每题 v2：操作数 / 通信字节 / wall 中位数

| 题 | 任务机制 | A | B | C | 证据充分 A/B/C |
| --- | --- | --- | --- | --- | --- |
| R01 | trailing 计数 helper，简单无调用控制 | 2 / 1,887 / 254 ms | 1 / 1,175 / 118 ms | 2 / 4,061 / 376 ms | ✓ / ✓ / ✓ |
| R02 | materialises 的直接项目依赖，拒绝误绑 vector.empty | 2 / 3,313 / 257 ms | 1 / 1,778 / 117 ms | 2 / 6,471 / 374 ms | ✓ / ✓ / ✓ |
| R03 | GLSL 提取器的外层 / tok lambda 调用归属 | 2 / 10,513 / 255 ms | 1 / 4,603 / 119 ms | 2 / 18,116 / 370 ms | ✓ / ✓ / ✓ |
| R04 | ShaderRegistry cache().find 接收者与 loader | 4 / 15,215 / 370 ms | 3 / 11,093 / 233 ms | 4 / 14,251 / 481 ms | ✓ / ✓ / ✓ |
| R05 | clear() / clear(Lifetime) 不能并作一个主体 | 2 / 4,034 / 251 ms | 1 / 1,461 / 118 ms | 2 / 4,790 / 358 ms | ✓ / ✓ / ✓ |
| R06 | Python load_lock 的空结果不能证明无 IO | 2 / 1,935 / 250 ms | 1 / 1,310 / 120 ms | 1 / 2,780 / 240 ms | ✓ / ✓ / ✓ |
| R07 | reader_suffix_ok 的重复静态调用位置 | 2 / 4,396 / 261 ms | 1 / 1,445 / 118 ms | 2 / 20,005 / 503 ms | ✓ / ✓ / **✗** |
| R08 | trailing inline 判定的生产直接 caller | 3 / 4,512 / 366 ms | 3 / 3,498 / 171 ms | 4 / 9,596 / 1,205 ms | ✓ / ✓ / ✓ |

未经过源码复核的**关系输出**单独看：A 满足 R01/R08（2/8）；C 满足 R01/R05/R06
（3/8），但丢失了 A 已找到的 R08。B 是另一种证据形态，不把它的 source-first
完整证据包伪称为“相同一次关系查询的准确率”。

### v1：保留未增强回退策略的原记录

| 组 | 证据充分 | 操作 | 字节 | 总 wall 三轮范围 |
| --- | ---: | ---: | ---: | ---: |
| A | 7/8 | 18 | 37,084 B | 2.275–2.306 s |
| B | 7/8 | 11 | 17,642 B | 1.109–1.147 s |
| C | 6/8 | 18 | 71,349 B | 3.982–4.028 s |

补充源码读取挽救了三臂的 R04，没有消除 C 的 R07 缺陷，也没有把结果调整到有利于 C。

## 真正有用的机制，以及尚未解决的问题

### 1. 有用：阻止旧 owner 与新源码混用

在独立的真实文件副本中，仅将 `tex_pass_plan_materialises` 定义改为等长的
`tex_pass_plan_materialized`，保持 size 和 mtime。两臂先建立索引，再故意不 refresh 查询旧名。

- 修改前，两臂都返回 3 条有效 owner 记录；新版不是从一开始就空转。
- A：返回 **3 条错误旧 owner** 的调用记录；freshness 仍为 metadata、updated=0。
- C：错误旧 owner **0 条**，明确 `content_changed`；没有将 freshness 冒充 verified。
- 随后使用两臂本就有的 `refresh` + `definition`，**两者都能恢复正确新定义**。

因此证明的是漏刷新的**故障拦截收益**，不是“只有新版会刷新”，更不是一般任务的效率收益。
所有修改发生在临时副本，恢复后删除；不计入 8 道未修改代码任务。

### 2. 有用：归属、主体与不支持状态更诚实

- R03 不再把 tok lambda 内的记录归给外层函数；旧版在该范围混入了内层记录。
- R05 新版拒绝把两个重载的正文并作一个主体，旧版返回混合调用列表。
- R02 新版不再把 `passes.empty()` 绑定到项目里的同名 `empty`。
- R06 明确披露不支持，而不是让 Python 的空 callee 结果看起来像无调用。

但 R02 的两个真实 helper 同时降为 unresolved；R06 相比 A 少一次读取，却仍比 B 更慢、
字节更多。诚实性收益不能自动变成“更容易完成任务”或“更省 token”。

### 3. 阻断问题：static_cast 产生伪调用

独立源码计数如下，**没有随候选结果改 gold**：

- R03 外层实际应有 13 个显式调用位置。旧版 20，新版 14；新版虽修复归属，仍多一个 cast。
- R07 实际应有 14 个位置。旧版去重后只剩 9；新版恢复重复位置后为 **15**。
- R07 的 556 行应为 5 个 size + 1 个 compare，新版正确保留 6；563 行只有 isalnum
  是函数调用，`static_cast<unsigned char>` 不应再产生一个名为 `char` 的调用。

编译有效的最小 C++17 控制证明：两臂的 `callers --name char` 都返回 1，期望为 0。
这不是新加代码独有的缺陷，而是原 `split_callee` 向最右命名叶子递归时把类型参数当成
callee 的旧漏洞。旧版的另一个去重错误会偶然遮住它；新版不再遮住，但也尚未修复。

冻结流程在 R07 看到 direct_syntax_v2、当前主体无解析错误后信任位置事实，因此没有再读
definition，最终证据包错误。R03 因 header 的 parse_error 触发了源码复核，才挽救该题。
不能依赖另一个降级条件偶然触发来掩盖调用事实本身的错误。

### 4. 阻断问题：引用返回定义缺失，普通主体又被保守拒绝

- 两臂均未索引 ShaderRegistry 文件内返回引用的 `cache()`；编译有效的最小
  `std::unordered_map<int,int>& cache()` 控制中，definition 期望 1，实际均为 0。
- 当前 C++ 查询规则覆盖 function_declarator / pointer_declarator，但没有对应的
  reference_declarator 形状。下一轮修复还需考虑旧索引对新增提取规则的安全重建。
- R04 **没有复现所担心的自递归误连**：旧版在 cache().find 处本已 unresolved。
  不能把这个没有发生的错误算成新版胜利。
- 新版却因类内 static、限定类型与 using namespace 等签名差异，未将该普通方法的声明
  与定义关联，导致 scoped callees 拒绝整个主体。需要区分“读取唯一已知定义的正文”与
  “证明声明/定义为同一个逻辑实体”，而不是以弱标签合并或一律拒绝代替设计。

### 5. 可用性问题：局部错误污染整个解析，重要文件位置淹没在覆盖样本里

R08 的两个生产调用点分别位于：

- `src/engine/texture_ops/tex_pass_plan.cpp:86`
- `src/godot/builder/graph_gpu_bake.cpp:1043`

旧版 scoped 查询找到两处。新版因匹配文件集合中有 4 个 parse_error，取消全部 target
绑定，再由 scope 过滤掉所有 unresolved 边，首查返回 **0**。完整流程撤去 scope、筛生产
文件并复核后恢复两处，但多一次查询，整题约 1.205 s，对照 source-first 约 0.171 s。

R01/R02/R03 还有类似的过宽降级。将文件集合缩到原文未改的 tex_pass_plan.cpp/.h 后，
明确定位到 `tex_pass_plan.h` 的解析错误。全仓输出却把前 16 个 issue 样本全用在早排序的
unsupported 文件上，只给 parse_error 计数，不给实际失败文件位置。

改进必须保持保守性，但应区分调用位置、候选解析、正文读取的覆盖范围，并优先展示可采取
行动的失败。不能只通过忽略解析错误来恢复好看的非空结果。

## 环境与证据

原始证据目录（本机临时目录，长期保存需另行归档）：

```text
/tmp/cx-ange-tasks.M10rZU/
  tasks.json / gold.json / corpus.lock.json / frozen.sha256
  PREREGISTRATION.md / AMENDMENT-v2.md / amendment-v2.json
  runner-v1.py / scorer-v1.py
  abc/runs.json / abc/<repeat-task-arm>/<step>.{stdout,stderr}
  abc-v2/runs.json / abc-v2/<repeat-task-arm>/<step>.{stdout,stderr}
  score-v1.json / score-v2.json
  diagnostics/findings.json / diagnostics-checked/findings.json
  diagnostics-checked/*-compiler.json / diagnostics-checked/*.cpp
  grammar-identity.json / ange-preservation.json
```

- 每轮 8×3×3 = **72 个流程**；v1、v2 各 72，无运行错误或超预算隐藏失败。
- 索引对象：5,006 个文件，cold updated=5,006，missing grammar=0。
- v2 cold A/B/C：2.546 / 2.875 / 2.526 s；峰值 RSS 162,316,288 / 158,744,576 /
  164,478,976 B。冷准备成本单列，不混进 warm 题目排名；不同 arm 都有自己的隔离 cache。
- 语法库固定为 language-pack 1.16.1 的同一 versioned libs 目录。当前库摘要与上轮已冻结
  grammar 清单完全相同。网络下载被本地不存在的 manifest URL 禁止。
- macOS arm64；rustc/cargo 1.98.1。微型 C++ 控制使用本机 c++、`-std=c++17 -fsyntax-only`，
  均编译成功；不是把编译失败当分析缺陷。
- 补充反空转控制也通过：两臂能提取普通 probe 定义及真实 isspace 调用；漏刷新试验两臂
  初始都确有 3 条记录。checked 重跑保持同样的缺陷/拦截结论。

关键 SHA-256：

```text
baseline binary  822c07bd545cd20802e4561ccd05f6370819f821b7a7867db2a938c738e27460
candidate binary 10093335b01ebcc723f5e210fe50212d3e253f77aded52ea9e071ec864217244
tasks.json       0b84332d38b6bc390b9bd6b6cd1df902913d62f7c06b30265b9622a842916045
gold.json        f46dc337ef3eb2ffafef1e959fc1a05159eb77459b95a9a670646e8ff8159456
corpus.lock      ae3bffc6a29a4d3a3f3a088d1324df51113a2694edc2ca2e3cf6e4ec34155d76
corpus archive   d8e2773e2e76b0702028639b1785dc19e139c4919e3d2e94116e1aa1094b5d8a
```

Gold 和用户源码不进入 cx 的生产检索输入；本报告没有粘贴 ANGE 函数正文。
评测 runner 不读取 gold，scorer 只在运行后读取，并校验原文范围和源文件摘要。
评分器的 6 个独立控制测试通过：位置不能去重、错误文件不能冒充正确位置、不支持的空结果
不能当证据、歧义候选不能冒充已解析目标、重载不能靠加一句 warning 掩盖正文合并、源码必须
原文一致并覆盖必要范围。补充文件身份检查后，v1/v2 的全部评分与原记录精确一致。

## 建议

1. 保留经过独立控制证明有价值的内容一致性、未知接收者、主体不合并与失败披露机制。
2. 先补真实案例对应的红色测试：builtin/template callee 提取、引用返回定义、普通类方法
   的正文选择、scoped unresolved frontier、关键失败文件披露。
3. 修复后重跑**同一**诊断集和全部旧回归；该集合以后仍是 dev，不能重新称 held-out。
4. 已知函数继续 definition-first；不要强制给每次导航付关系快照和长候选列表的成本。
5. 下一阶段需要新的独立任务集证明额外能力的收益。当前结果不授权自动启动 impact、
   提交、发布或 agent 试验。
