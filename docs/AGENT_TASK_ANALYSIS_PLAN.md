# cx 任务级代码分析增强：开发与有效性验收指南

> 状态：待实现的设计交接文档，不是功能完成报告。
> 目标：在现有 cx 上增加可信的影响分析、变更分析和任务上下文检索。
> 首要要求：测试必须证明新增能力确实有效，不能只证明命令能执行或返回非空结果。
> 实施顺序：关系基础加固 → `impact` → `changes` → `context`。
> 本文中的新增接口、目录、字段和阈值均为设计建议；现有实现事实另行标明。

## 1. 给后续开发 session 的说明

在 cx 项目根目录开展工作。先读本文，再读以下文件：

- [既有开发路线](AGENT_NATIVE_DEVELOPMENT_ROADMAP.md)：产品边界、身份和证据模型。
- [验收标准](ACCEPTANCE_TEST_STANDARD.md)：现场保护、正确性硬门、固定 ANGE 语料和性能预算。
- [直接调用关系](PHASE7_DIRECT_RELATIONS.md)：当前一跳契约及已知边界。
- [符号身份](PHASE5_QUALIFIED_IDENTITY.md)、[新鲜度](PHASE4_FRESHNESS.md)。
- 涉及扩展时再读 [Pi 扩展说明](../extensions/pi-cx/README.md)、[发布流程](PI_EXTENSION_RELEASE.md)，并核对当前宿主的扩展 API 文档。

本文延续既有 Phase 0–7，不取代它们。既有路线文档描述的是更早基线，判断当前行为以源码和实际测试为准。

执行纪律：

1. 先记录 commit、branch、dirty 状态和工具版本；不得覆盖用户工作。
2. 每阶段先写测试和标准答案，再改生产实现。
3. 一次完成一个阶段并验收，不把四项能力同时铺开。
4. 未通过有效性验收的功能只能报告为实验实现，不能报告为已证明有收益。
5. 不因本文件而自动提交、发布、启动委派或批量执行流程；这些操作依照用户在新 session 中的授权。
6. 所有会修改被测语料的实验在临时副本或专用 disposable worktree 中执行。

## 2. 已检查的源码基线

本次只读检查时：

| 项目 | 基线 |
| --- | --- |
| cx 本地位置 | `/Users/allen/Documents/RustProjects/cx` |
| cx commit | `18337372cc95092fd9daca45936159e3d28652d9` |
| branch | `master` |
| 工作树 | 开始编写本文前 clean |
| crate / Pi package | `0.7.10` |
| 索引版本 | `INDEX_VERSION = 13` |
| JSON envelope | `SCHEMA_VERSION = 1` |
| ripwire 参考位置 | `/Users/allen/Documents/Projects/ripwire` |
| ripwire 参考 commit | `216802ad74e928f30c1dd2183567e76be6ddf530` |

以上记录不代表测试通过。本次检查没有构建或运行 cx 测试，也没有完成 cx 与 ripwire 的实测对照。后续 session 必须重新核对基线；路径不可用时不能假装已阅读参考实现。

### 2.1 已有机制与落点

| 机制 | 当前源码锚点 | 可复用部分 |
| --- | --- | --- |
| 文件增量索引 | `src/index.rs::Index`、`FileData` | redb、文件元数据、symbols、imports |
| 符号身份 | `Symbol`、`StableSymbolId`、`Symbol::stable_id` | role、scope、qualified name、signature |
| 调用证据 | `src/relations.rs::EvidenceKind`、`ResolutionLevel` | 语法、作用域、import 等证据分类 |
| 一跳关系 | `callers`、`callees`、`resolve_call` | 调用位置、候选收集和消歧规则 |
| AST 调用点 | `src/language/extract.rs::CallSite`、`find_call_sites` | 名称、书写限定符、行号、字节位置 |
| 文件 import 查询 | `src/map.rs::ImportIndex` | 路径查找、外部/歧义分类 |
| 新鲜度 | `Freshness`、`FreshnessRequest`、`Index::load_or_build` | metadata / verified / paths、generation |
| 输出 | `src/output.rs::Envelope`、`src/query.rs` | 固定 JSON 根、分页、warnings、next_queries |
| 扩展入口 | `extensions/pi-cx/tools.ts` | typed schema、参数校验、工具注册 |
| 回归测试 | `tests/relations.rs`、`qualified_identity.rs`、`freshness.rs` | 既有一跳、身份和刷新契约 |

### 2.2 不能假定已经存在的能力

- `FileData` 当前不持久化调用点或已解析调用图。`callers/callees` 在查询阶段读取并重新解析源码。
- `EdgeRow` 的端点和候选主要是字符串；候选去重使用显示限定名，不是完备的图节点身份。
- `StableSymbolId` 不包含文件身份；scope 未建模时退化为裸名称；签名是当前签名文本。不能直接充当跨版本永不变化的 ID。
- freshness 是索引检查的证据，不是整个工作树的不可变源码快照。关系查询随后再次读盘，有混合新旧数据的时间窗口。
- 部分关系读取/解析失败路径直接跳过文件。新增分析不能把这种缺失解释为没有影响。
- `call_model` 显式覆盖 C/C++、Rust、TypeScript；HTML 脚本通过独立解析单元处理。不能由“支持符号提取”推导出“支持调用分析”。
- 语法级测试识别主要是 Rust 属性，其他场景还有路径分类；不是通用测试发现或运行时覆盖率。
- `cx_map` 按子系统 fan-in、符号数、名称排序，不是任务相关性检索或符号级 PageRank。
- `src/util/git.rs` 目前只找项目根，没有 Git diff、历史内容或跨版本符号分析层。

这些是增强设计的起点。潜在身份碰撞、竞态和漏报必须通过新增 fixture 复现，不把源码观察冒充已确认的运行缺陷。

## 3. 产品边界与总体结构

保留 cx 的低成本查询阶梯：已知符号时继续使用 `definition`；一跳问题继续使用 `callers/callees`。不自动给每次查询附加地图、风险分数和测试列表。

建议分层：

```text
文件内容与解析事实
  symbols / call sites / imports / content identity
                    ↓
关系分析层
  节点身份 / 候选解析 / 正反向邻接 / 分析覆盖披露
                    ↓
任务能力
  impact / changes / context
                    ↓
统一 JSON、CLI、Pi 工具适配
```

非目标：

- 不引入 daemon、LSP 生命周期或运行时网络依赖。
- 不承诺编译器级重载解析、动态分派、模板实例化、完整宏展开。
- 不给出“安全修改”“没有任何影响”“无需运行测试”的无条件结论。
- 不先做全套复杂度、质量评分、代码生成或重命名执行器。
- 不为模仿 ripwire 改成 XML，不直接搬运其所有命令。
- 不预先决定换数据库、引入 embedding 或 PageRank；这些需要消融实验支持。

## 4. 什么才算新增能力有效

必须分别回答四个问题，不能用一个分数掩盖其他维度：

1. **正确性**：输出的关系、变更和位置是否真实，是否存在错误连接或错误归属？
2. **完整性与诚实性**：要求范围内是否漏掉必要信息；无法分析、歧义和截断是否明确？
3. **任务收益**：相同问题是否更容易得到足够证据，而不是只有更多字段？
4. **成本**：包含失败、分页和回退在内，调用次数、token、时间和内存是否合理？

仅有以下结果均不算有效性证明：

- 命令 exit 0；
- 返回结果数量大于零；
- 新输出与自身缓存一致；
- 新工具比故意读取整个仓库的弱基线更短；
- 只比较第一次调用，不计算补充读取和回退；
- 从新工具输出抄录 gold，再证明命中率很高；
- 推荐的测试文件确实存在，但未验证它是否能发现相关回归。

后续章节给出分层测试、冻结任务集、对照组和阶段验收条件。

## 5. 测试层一：独立标准答案与精确 fixture

### 5.1 标准答案如何建立

标准答案由阅读源代码、明确语言规则和题目要求建立，不运行候选能力来产生答案。建议新增 `tests/fixtures/task_analysis/`，每个案例配套声明式 oracle。

oracle 至少包含：

```json
{
  "id": "impact-diamond-01",
  "family": "impact",
  "question": "改变 leaf 后，在两跳内哪些函数受调用关系影响？",
  "subject": {"file": "src/core.rs", "qualified": "leaf"},
  "required_nodes": ["left", "right", "entry"],
  "forbidden_nodes": ["unrelated"],
  "required_depths": {"left": 1, "right": 1, "entry": 2},
  "required_disclosures": [],
  "evidence": ["src/core.rs 中逐条列出的调用位置"],
  "provenance": "人工阅读 fixture 后编写；候选工具运行前冻结"
}
```

上例是格式示意，不是已存在的测试文件。实际节点使用包含文件、语言和定义位置的精确标识，不能只用示例中的短名。

要求：

- 断言精确集合、数量、深度、候选、来源位置，不只断言某字符串出现。
- oracle 不能调用生产 resolver、生产 BFS 或生产排序函数计算期望值。
- 同时保存正例与禁止出现的反例，防止通过“返回所有文件”提高 recall。
- 受限静态模型的期望和真实运行时行为分开记录。模型不能解析的调用必须被披露，但不能被提升成确定路径。
- 修正 oracle 必须说明源码或题意依据；失败本身不是修改 gold 的理由。

### 5.2 关系基础与 impact 测试矩阵

| 案例 | 必须验证的性质 | 典型错误实现 |
| --- | --- | --- |
| 线性链 `entry → middle → leaf` | 反向遍历 leaf 得到深度 1 的 middle、深度 2 的 entry；方向正确 | 把 callees 当 callers |
| 菱形图 | 两条路径共享的祖先只计一个符号，深度为最短距离 | 按路径重复计数 |
| 自递归和互递归 | 终止，seed 单列，不重复进入影响计数 | 无 visited、重复计 seed |
| 两文件同名局部函数 | 文件身份不合并，路径不串接 | 以显示名称作为节点键 |
| 同限定名的不同重载 | 不凭限定名唯一就认定某个重载 | labels 去重制造唯一候选 |
| C++ 声明/定义分离 | 声明位置不与定义丢失关联，也不把两个实体误合并 | 全局 definition 优先屏蔽别的声明 |
| 不同语言同名符号 | 候选过滤正确，无虚假跨语言连接 | 仅按短名建图 |
| 嵌套函数/闭包/类方法 | 调用归于实际所属执行容器；不能把内层全部调用算到外层 | 仅按外层字节范围收集 |
| 接收者未知的方法调用 | 保留候选和原因；默认不沿歧义边传播 | 任意挑候选或全部当确定边 |
| 仅名称唯一 | 输出仍为 syntax 依据，不冒充作用域或类型证明 | 有 to 就标成已语义解析 |
| import 歧义、别名、遮蔽 | 能证明才关联；不支持的形式明确保留未知 | 文件 import 等同符号绑定 |
| 宏调用和动态分派 | 各类事实不冒充普通已解析函数调用 | 将宏名或虚调用强连到函数 |
| 无调用支持的语言 | capability/coverage 明确，零结果不等于零影响 | parser 成功便宣称图完整 |
| 缺 grammar、读失败、语法错误 | 错误或 partial 有结构化原因和范围 | continue 后报告完整空集 |
| 查询中修改/删除文件 | 不混用新调用点与旧符号范围 | 仅在开始检查 freshness |
| 未修改 caller，新增同名 target | 原唯一候选失效并重新消歧 | 只更新修改文件的已解析边 |
| 深度/节点/输出预算分别耗尽 | 截断轴分开，未知总数不能伪装 exact total | 用分页掩盖遍历提前停止 |
| 重排输入、并发、冷热缓存 | 结果集合与稳定排序相同，见证路径选择确定 | HashMap 顺序决定输出 |

竞态测试使用可控 hook/barrier，避免依赖 sleep 碰运气。静态原始调用点的来源内容 hash 必须与符号事实匹配。

### 5.3 changes 测试矩阵

为每个案例建立临时 Git 仓库及明确 old/new blob；不要依赖开发者当前分支状态。

| 案例 | 预期 |
| --- | --- |
| 函数体内一行修改 | 精确映射目标函数，邻接函数不是 changed |
| 签名修改 | old/new 两侧均可定位，签名改变不等于符号删除后毫无关联 |
| 函数整体删除 | old-side 符号存在于报告；不能只查当前索引 |
| 新增函数 | old-side 不存在是正常事实；影响只按可证明关系计算 |
| 重命名/移动 | 文件移动与符号身份匹配分开；启发式匹配必须标注 |
| 顶层 import、类型、常量和文件级代码改变 | 不强塞给最近函数；保留文件级变更 |
| 相邻 hunk、空行、注释、CRLF、Unicode | 行到字节映射准确；不跨函数误归属 |
| staged 与 unstaged 同时存在 | 所选比较模式明确，结果不混用 Git index 与工作树 |
| diverged base 分支 | direct 与 merge-base 模式不同且符合契约 |
| mode-only、binary、symlink、submodule | 分类报告；不以“无源码行”丢失变更 |
| untracked 文件 | 默认是否纳入明确，未纳入时披露 |
| 非 Git、unborn HEAD、坏 ref、无 merge-base | 结构化失败/不支持，不报告 clean |
| 空格、换行、前导减号路径，恶意 ref | argv 安全、NUL 路径解析；不执行输入 |
| 以仓库子目录为 cx root | repo-relative 和 index-relative 映射正确，域外变更有计数 |
| diff 后工作树再次修改 | 检测不一致，不能用第三个内容版本映射旧 hunk |

变更影响的 gold 要分开保存 `changed_symbols`、`before_impact`、`after_impact`，不能把三种集合相加当作一个精确影响数。

### 5.4 context 测试矩阵

- 精确符号名、qualified name 和路径提示优先命中目标。
- camelCase/snake_case 子词可查，短名噪声不会淹没精确匹配。
- 只有注释或函数体包含任务词时仍可命中，并报告实际证据字段。
- 注释、字符串和代码事实分开；检索命中不自动成为调用证据。
- vendor/generated/fixtures 中的词汇孪生体不能默认压过真实实现；明确询问它们时仍能找到。
- 零词汇证据时可以不返回候选，不能由 PageRank 填满名单伪装任务相关。
- 多文件任务评估全部必要文件，不只评估找到了一个。
- 测试文件是否纳入由任务和过滤规则决定，不隐藏必要测试以压低 token。
- 中文、英文及中英混合题分层记录；不默认宣称跨语言语义检索。
- 极小预算、长签名、超大函数体和长路径下，仍保留主体身份和不完整说明。
- 源码正文必须是原文；截断有范围，不能生成假代码或悄悄改写。
- 增大预算不应让精确命中主体消失；不强求整个排名集合单调。

## 6. 测试层二：证明测试本身能识别错误

对关键机制做定向变异测试。可以用隔离副本中的小补丁或测试替身，不要求先引入通用 mutation framework。

至少覆盖以下必杀变异，每个都必须有一个稳定失败的断言：

1. 将节点键改成裸名称。
2. 将反向边误用为正向边。
3. 沿 ambiguous candidate 作为确定边传播。
4. 移除 visited 去重或把 seed 计入 reach。
5. 忽略内容 hash 或强制复用旧关系缓存。
6. 忽略 deleted hunk / old-side symbols。
7. 将坏 ref、缺 grammar、分析失败改成空成功。
8. 删除必要披露，以更小输出伪装预算成功。
9. 让精确名称查询走纯热门符号排名。
10. 将测试关联改成仅文件名相似。

报告列出 `mutation → 预期失败测试 → 实际失败断言`。存活变异必须解释并补测试；不能用编译失败或无关测试失败算作杀死。每阶段只要求其已实现机制对应的变异，但不能宣称未实施阶段也完成验收。

## 7. 测试层三：冻结真实任务集，比较完整查询流程

### 7.1 样本设计

建议在 `bench/task_analysis/` 下建立独立于 unit tests 的评测包。最小试点：

- 6 个固定 commit 的仓库：Rust、C/C++、TypeScript 各 2 个。
- 每仓库 12 题：impact、changes、context 各 4 题；总计 72 题。
- 按仓库拆分 dev 与 held-out：每种语言各 1 个 dev、1 个 held-out 仓库。因此每能力 12 道 dev、12 道 held-out 题。
- 每能力至少包含 1 道简单/无需增强题、1 道跨文件题、1 道歧义或不支持题、1 道其特有难题。
- changes 的难题包含删除/签名变化；impact 包含多跳；context 包含描述性和多目标问题。

这是工程试点，不足以证明对所有仓库普遍领先。每能力只在自己阶段运行对应题组，不能让 context 的收益掩盖 impact 的退化。fixture 不计入真实任务分母。

若资源不足，可以先做 dev 样本和完整 fixture，但结论必须是“正确性已验证，真实任务收益待测”，不能用小样本自行降低正式门槛。

### 7.2 冻结与防泄漏

1. 先选仓库、commit、任务和取样规则，保存 `corpus.lock`，记录源文件/补丁 hash。
2. 按源码人工写 gold；任务问题不能只是把目标函数名改写一遍。
3. gold 包含必要答案项、禁止断言、证据位置和可接受的不确定状态。
4. impact 的真实仓库 gold 不宣称全仓运行时全集：冻结具体审查范围，并逐项验证报告新增的候选；范围外条目先标待审，不能自动当正确。
5. context 可使用真实 issue 文本和历史补丁；补丁不是绝对 gold。剔除与题意无关的格式/版本修改时须记录理由，不能按工具是否命中决定。
6. 固定问题、gold、分组、基线策略、预算、指标和接受规则，生成 hash 清单后才运行候选实现。
7. held-out gold 不进入索引目录，不放在被检索源码旁。评测脚本可以读 gold，工具和被测 agent 不可以读。
8. 不根据 held-out 失败逐题调参再重复宣称 held-out 成绩；看过结果后该集合成为诊断集。新一轮需要新的独立 holdout，并保留旧结果。

允许纠正明确的 gold 错误，但必须提交原值、新值、源码依据和对所有实验组的重评分，不能只修有利于候选组的标签。

### 7.3 三个必要对照组

| 组 | 能力 | 检验的问题 |
| --- | --- | --- |
| A：当前 cx | 固定基线 binary，现有八种工具；允许正常 grep/read/git 补充 | 相比用户当前工作流有无收益？ |
| B：机械组合基线 | 同一基线 binary，将既有查询批处理、去重、分页组合；不增加解析事实或排名模型 | 收益只是减少 RPC，还是新分析真正有价值？ |
| C：增强 cx | 新能力及必要后续读取；同样允许 fallback | 新实现是否更正确、更完整或更便宜？ |

A 不是故意读整个仓库的弱基线。B 的算法和成本必须公开，不能读取 gold 决定下一步；B 若无法可靠拼接显示名，应保留歧义而不是强行连图。

评测先采用确定性的任务 runner：每类题预先定义查询阶梯、最大调用数、分页和 fallback 规则。符号/查询词的选择来自题目及之前结果，不来自 gold。在 dev 集上确认 A/B 不是明显低效的实现，再冻结。

需要衡量真实 agent 使用效果时，另做小规模 agent-in-the-loop 验证：同模型/版本、同提示、同初始上下文和工具预算，随机化组顺序，隔离缓存和会话，记录实际调用。模型有随机性时至少 3 次重复；不能把 3 次当成 3 道独立题。

agent 试验不能仅靠提示“不要使用新工具”隔离控制组，应在工具和 PATH 层面限制可用能力，并记录实际调用。当前文档不授权启动这些 agent，执行时需用户授权。

### 7.4 完整流程成本与答案判定

一次题目从开始到证据充分，或者达到冻结的调用/时间上限。记录所有：

- 查询参数、返回内容、exit code、stderr、错误、重试和页数；
- grep/read/git、补充 definition、fallback 的成本；
- 工具请求和结果 token，agent 输入/输出/缓存 token 分开；
- wall time、CPU、peak RSS、index bytes、索引准备成本；
- 最终答案及其证据列表、未回答项。

用固定名称和版本的 tokenizer 离线计数，评测依赖不进入 cx 运行时。若只测 bytes，则只报告 bytes，不能把 `bytes/4` 写成真实 token。工具 schema/提示带来的固定上下文成本单列，避免新增多个工具的成本被隐藏。

两组必须采用相同缓存状态：分别报告 cold 和 warm。网络 grammar 下载不算查询耗时，单独记录。B 的本地拼接与序列化也计时；不能把所有 baseline 工作藏在计时器外。

定义：

- **严格满足率**：所有 required 答案项都有正确证据、无 forbidden 断言、必要 uncertainty/coverage 均披露。达到上限仍不完整是失败。
- **支持模型内 precision/recall**：针对独立 gold 中可建模的关系分别计算，不把未支持语言从全部任务成功率分母中删除。
- **误导性断言数**：错误确定边、漏报分析失败、把零结果说成安全、把测试候选说成覆盖证明。
- **context strict file@k**：所有必要文件都在前 k；同时报告 any@k 和符号级命中，不能只展示更好看的一个。
- **证据充分前的调用数/成本**：所有步骤累计，不仅是第一条响应。
- **完成一个严格满足任务的总成本**：整个题组所有尝试成本之和 / 严格满足任务数；失败尝试的成本也在分子，分母为零时记为不可定义。

“两个组都成功的子集”成本可以作为补充，但必须同时报告所有任务和失败 fallback 成本，防止通过放弃难题制造节省。

### 7.5 消融实验：确认究竟什么起作用

消融开关优先放在评测 harness 或内部测试配置，不为试验增加永久用户参数。

| 能力 | 对照/消融 | 应检验的机制 |
| --- | --- | --- |
| 关系层 | 当前按查询重解析 vs 一次构建关系快照 | 同等结果下是否减少解析次数、wall/RSS 是否合理 |
| impact | 一跳、B 的多次拼接、新的有界图遍历 | 多跳必要答案是否被补齐，歧义是否没有被放大 |
| changes | 文件列表、current-only 符号映射、before/after 映射 | 删除/签名修改是否因读取旧内容而真正被发现 |
| context | 精确名/路径基线、加子词、加注释/函数体、加图扩展 | 每个组件提高哪些题，带来哪些新污染和成本 |
| 预算策略 | 相同检索结果不同打包方式 | 减少 bytes 是否仍保留任务必要证据 |

如果 B 与 C 在正确性和完整流程成本上相当，应如实结论为“批处理即可获得主要收益”，不为证明复杂实现必要而弱化 B。

### 7.6 预注册的采用/回退条件

以下是建议的第一轮工程接受规则，不是测量结果。实现 session 在运行 held-out 前确认并冻结；如需调整，应先说明原因，不能看完成绩后修改门槛。

**所有阶段的硬门：**

- 对应 fixture 的精确断言全部通过，关键变异全部被正确断言杀死。
- 已支持的旧能力无未经解释的契约回退；既有验收标准不放宽。
- 人工 adjudication 后，held-out 中误导性确定断言为 0。
- 明确披露的部分结果可以存在，但不因此自动算严格满足。

**基础加固阶段：**不要求新增任务命中率。要求修复由红色 fixture 证明的问题，避免基本查询明显变重；如实现缓存，另证明跨文件消歧失效正确。

**每个新能力的 12 道 held-out 题：**

1. C 不得丢失 A 或 B 已严格满足的题目，除非独立源码审查证明基线答案原本错误；审查须更新所有组。
2. 至少满足一种收益路径：
   - 质量路径：相对较强基线的严格满足题数净增加至少 2/12，且单题总 wall 的 p95 不超过该基线的 1.25 倍；
   - 效率路径：严格满足集合不缩小，完整题组工具通信 token 至少减少 20%，配对题目调用数中位数至少减少 1，wall p95 不超过较强基线的 1.25 倍。
3. 两条路径都要报告 C 对 A、对 B 的结果；“较强基线”按预先冻结规则取严格满足数较多者，同数时取总通信 token 较少者。
4. 简单题单独列出，不允许复杂题的收益遮盖基础查询被强制变重。新增能力不是简单题的默认入口。

以上 p95 在 n=12 下非常不稳定，必须同时报告全部配对数据和 max；这些是小样本工程门，不宣称统计显著或普适领先。计数不足、环境失效或缺 oracle 时为证据不足，不是 PASS。

若质量显著改善但超出延迟门，保留实验记录，向用户提出明确权衡，不自行宣布通过。若只有效率通过，结论只写“更省调用/通信成本”，不宣称解析更准。若没有收益，停止扩张默认工具面，保留测试与失败分析。

## 8. 测试推荐的专项有效性实验

“找到了关联测试文件”和“该测试能发现这次回归”是两个不同命题。任何阶段一旦输出测试推荐，必须增加以下实验，不能只测试路径存在。

### 8.1 可执行的回归样本

在受控、可运行的小型项目中，为支持的语言设计至少 8 个独立故障补丁。补丁来自人工明确的行为契约，不来自 cx 的调用图。例：边界条件错一位、漏更新缓存版本、返回值反转、错误分支失效。

对每个故障：

1. 未打补丁时全套受控测试成功；否则案例无效。
2. 打补丁后至少一个测试因目标行为断言失败，而不是编译失败、依赖缺失或超时。
3. 分别运行全部已知测试，冻结哪些测试检测了该故障；这是独立 detection oracle。
4. 在相同修改上让工具给出测试候选，运行它推荐且确实可执行的测试。
5. 恢复临时副本并验证原测试重新成功，避免污染后续案例。

同时包含：直接调用测试、多跳测试、与目标同名但无关的测试、同目录无关测试、只通过 CLI 子进程覆盖的测试、修改测试本身。运行时证据只作为评测 oracle，不偷放到 cx 静态索引中。

### 8.2 指标与接受条件

- **fault detection recall**：推荐集能检测到的故障数 / 所有被全套测试检测到的故障数。
- **selection cost**：被推荐测试数、实际运行时间，相对全套测试的比例。
- **无证据推荐率**：工具声称有关联但不能指出调用/清单/其他明确证据的比例。
- **unsupported detection cases**：例如 CLI 子进程测试，单独报告，不能从总体分母消失。

工程硬门：已建模、证据充分的受控故障不能漏掉全部能检测它的测试；无关控制测试不能被当作具有确定调用证据；不支持案例必须明确披露。还要与 A/B 对照报告选择成本。

对全套测试都推荐的方案，检测率可能为 100%，但它只证明保守性，不证明测试选择有效。只有在不降低检测能力的情况下减少实际测试成本，才能宣称推荐有实用收益。

不从 `no test found` 推导 `untested` 或 `safe`；可用措辞是“在当前模型内未找到测试调用路径”。runner 无法推导时只提供文件和原因，不生成猜测的测试命令。

## 9. 数据与输出契约：先固定含义，再实现字段

### 9.1 身份与关系

内部建议区分三种对象：

- `DefinitionSiteId`：当前分析快照内精确的定义位置，至少包含规范化文件身份、语言/嵌入解析单元、范围或可区分同名定义的键。
- `LogicalSymbolKey`：可以有证据地关联的声明/定义实体；重载和局部绑定不能只靠名称合并。
- `VersionMatch`：old/new 实体的对应关系，带匹配依据和歧义，不能复用位置 ID 假装跨版本稳定。

名称只用于显示。关系内部保留 typed `from`、可选 `to`、typed candidates、调用位置、证据类型、解析依据、来源内容身份。对外兼容既有 `EdgeRow`，不能直接把旧字符串字段改成 object 而不处理版本迁移。

`resolution` 不是简单概率刻度。import 文件匹配不一定比所有 lexical 事实强；路径结果需要保留各边的实际依据，而不是求 enum 的最小值后宣称整条路径有统一置信度。

### 9.2 分析快照、覆盖与缓存

- 同次分析的符号和调用点必须来自同一内容版本。
- 不要求锁住整个用户工作树；可以使用被读取内容的不可变副本/hash 清单，或索引内同代解析事实。
- 再读正文时校验内容身份；变化则有界重试或返回结构化失败，不偷偷混入新正文。
- metadata 模式仍保留其已知盲区，不能因为做了某些内容校验就把整个仓库标 verified。
- 报告支持/未支持的调用语言、缺 grammar、读取失败、解析降级、跳过文件、分析范围和模型限制。
- 无解析错误不等于图完整；完整性只能描述本次指定模型、范围和预算内的枚举情况。
- 缓存原始调用点时同时记录提取规则版本与内容 hash；缓存目标解析结果还需要候选集合/import 变化的失效机制。
- 如改变持久布局，更新 `INDEX_VERSION` 并验证旧索引安全重建；纯内存派生结构不必自动扩大所有基础查询的持久数据。

### 9.3 分页与遍历预算不能混为一谈

建议 impact 分开表达：

```text
analysis: roots, model, snapshot identity, complete-within-model, stop reasons
limits: max depth, max nodes, max edges examined, output limit
results: symbol identity, minimum depth, witness path, evidence
uncertainty: ambiguous frontier, unsupported/read/parse coverage
```

这是设计形状，不是已确定 JSON schema。实现前用 golden 测试冻结实际字段。

既有 `page.total` 表示分页前精确匹配数，不能改成“遍历停止时碰巧找到的数量”。如果图遍历被截断，应明确另列已发现数和未知全集状态；新的结果包装与扩展 parser 必须一起测试。输出截断可翻页，分析预算截断通常需要重算；`next_queries` 必须区分二者，不能提供并不存在的后续页。

用相同快照/参数分页才可比较。若继续查询时内容已变化，必须披露 generation/hash 改变，不能拼成同一答案。未知数量不能填 0；必要的身份、错误和截断元数据不能为满足预算被删除。

### 9.4 错误与部分结果

延续旧 envelope。新增能力应区分 subject 不存在、subject 多义、能力不支持、Git 输入错误、内容变化、分析部分完成和真正空成功。

具体 error code 和退出码由阶段契约固定，遵循既有错误码体系并测试 CLI/Pi 一致性。新增枚举值可能使严格 consumer 拒绝，不能仅凭“增加字段是 additive”就假定不需要适配。检查 `extensions/pi-cx/protocol.ts` 和 `types.ts`。

“查询执行成功”不等于“代码通过检查”。第一版 impact/changes 是证据查询，不引入模仿 ripwire 的测试义务退出码，不把查询 exit 0 当作 CI 安全门。

## 10. 分阶段开发计划

### 10.1 阶段 R：关系基础加固

先做 §5.2 中身份、调用归属、歧义、内容一致性和失败披露的红色 fixture。再抽取共享关系分析层；新文件可命名为 `src/relation_index.rs`，名称不是强制。

建议落点：

- `src/index.rs`：节点身份、来源内容和可选原始调用点缓存。
- `src/language/mod.rs::FileParse`：需要缓存时，在同一 AST parse 中提取调用事实，避免重复解析。
- `src/language/extract.rs`：调用位置、所属容器、不同语法形式的证据。
- `src/relations.rs`：候选表、结构化解析结果和一跳投影。
- `src/map.rs::ImportIndex`：共享 import 查询，但不要为任务功能改变 map 的旧含义。

先构建一次命令内的共享图，实测后决定是否持久化。若增量重用复杂度过大，允许每次从缓存原始事实重建解析图；不能为追求 warm 延迟牺牲正确失效。

退出条件：红色控制转绿、旧一跳测试保持、关键 mutation 被杀死、内容一致性可验证，基础查询性能满足旧标准。R 阶段不新增工具，不用“尚无用户功能”跳过正确性验收。

### 10.2 阶段 I：独立的有界 impact

候选 CLI（尚未实现）：

```text
cx impact --name load --scope 'Cache::load' --file src/cache.rs --max-depth 3 --limit 50
```

参数设计要求：可精确定位文件与实体；scope 仍匹配完整 qualified name。root 多义时返回候选供用户选择，默认不将多个同名实体 union 成一个目标。

默认只沿当前支持的消歧规则建立的唯一目标传播；syntax-only 唯一名称关联单列为可能影响，不混入较强证据路径。即使 lexical/import 唯一也不是编译器证明，要保留其限制。未知/歧义 frontier 可以展示，不默认展开全部组合。

BFS 计算最短深度；visited 使用实体身份；seed 单列；见证路径按固定排序选择。按深度和稳定实体键排序，不必引入 PageRank。import 文件影响如纳入，则放独立分区，不和符号调用影响相加。

退出条件：§5.2 全部适用 fixture、§6 变异、§7 impact held-out 门，以及 CLI/Pi 对同一输入的等价结果。保留旧 callers/callees 无多跳参数的测试；新能力通过独立 impact 提供，不删掉旧约束。

### 10.3 阶段 C：changes 分两步交付

**C1：变更定位。** 定义工作树、staged、两个 commit、merge-base 比较的语义，先实现最小明确子集。建议默认工作树对 HEAD（不含 untracked），其他模式显式选择。尚不支持的模式拒绝，不猜测。默认值和是否纳入 untracked 在接口冻结前确认。

用 `std::process::Command` 的 argv 调 Git，禁用 external diff/textconv，校验 ref 为 commit/OID，使用安全的参数终止和 NUL 路径协议。不要拼接 shell，不执行仓库内容提供的命令。merge-base 失败不默默退成 direct diff。

通过 Git 对象读取旧内容、索引或已校验工作树内容读取新侧；不 checkout 用户分支。纯 hunk 对应与符号跨版本匹配分开。不支持的二进制、symlink、submodule、mode-only 变更保留分类记录。

**C2：影响与测试证据。** 组合 I 阶段，不重新实现 BFS。before/after 的调用图分别来自各自快照。只解析 changed old files 足够识别删除符号，但不够证明旧全仓调用影响；未建立 baseline 图时必须声明 before impact 未分析。

测试输出先限于候选及依据。包含 runner 时只能由明确配置或可验证规则推导，运行测试不是 changes 查询的隐式副作用。

退出条件：§5.3 与 Git 安全控制、§7 changes 题组；有测试推荐时必须做 §8。C1 可独立交付，但任务集和报告必须明确仅验收变更定位，不声称 C2 完成。

### 10.4 阶段 Q：context 检索与打包

先实现精确路径/标识符与子词匹配，再加入注释/函数体词汇检索；可以采用 BM25，但要保存实际命中字段和位置。倒排缓存是否持久化由索引大小、刷新成本和 warm 测量决定。

检索、图扩展、预算打包分成可消融组件。优先返回签名和可直接使用的 definition 查询，按需带有限原文正文；不要默认读入所有函数体。多个正文必须去重并附来源内容身份。

图扩展必须有理由，例如“直接调用某个词汇命中的符号”，不能把图中心性当作任务正确性。零证据时允许 abstain；排名 margin 不是语义置信度，第一版无需输出 high/low confidence。

tokenBudget 若采用估计值，应明示估计器，不承诺真实模型 token 硬上限。可首先提供确定的 byte budget，并用评测 tokenizer 比较真实成本。任何预算都包含封装和必要披露。

退出条件：§5.4、§7 context held-out 和消融；只有图扩展有可重复净收益才纳入默认路径。不要把 overview/map 默认升级为 Q，以免每次导航都支付复杂检索成本。

## 11. 如何参考 ripwire，而不是复制其假设

本节路径均相对于 §2 的 ripwire 仓库。参考 commit 用于定位，行号会漂移，应按符号搜索。所列文件存在性及部分实现锚点已检查，不表示每项机制都已完整审计或适用于 cx。

| 主题 | 参考位置 | 借鉴重点 | 不能直接照搬 |
| --- | --- | --- | --- |
| 正反向图及来源证据 | `src/graph.h::Graph` | 紧凑邻接、节点句柄、每边 provenance | cx 不应因此改写数据库或混入歧义分摊边 |
| 多跳反向遍历 | `src/graph.h::transitiveCallersDepth`、`transitiveCallers` | seed、去重、深度的共享计算 | 算法正确不代表输入调用边语义正确 |
| 影响输出与独立 import 层 | `src/verbs_navigate.h::ImpactView`、`runImpact` | 分析一次、多种输出共享；文件与符号计数分离 | 不直接移植 XML 或把 import 当函数调用 |
| Git 比较锚点 | `src/prcontext.h::resolveDiffAnchor`、`gitDiffChangedMaskNumstat` | direct/merge-base 差异、坏 ref、mode-only 和路径问题 | cx 必须自行定义比较契约，不复制其降级和 shell plumbing |
| 变更上下文组合 | `src/situ.h::computeSituationFacts`、`computeTestGateFor` | 复用分析、拒绝非法输入、测试证据 | 不把该工具的 exit code、测试识别能力视作 cx 已具备 |
| 词汇检索 | `src/lexical.h::lexicalScoresTiered` | 子词、BM25、字段证据与过滤 | 不搬未经 cx 语料验证的参数、路由阈值 |
| 预算任务包 | `src/packtask.h`，检索 `packTaskBundleText` | 检索、正文、关联信息共用预算，封装也计费 | 不复制固定章节比例，先测 cx 的任务需求 |
| 身份/声明盲区 | `test/blindspotcheck.sh` | 声明与定义分离导致空影响的反例 | 当前测试通过不等于所有 C++ 布局可靠 |
| 影响计数 | `test/impactpartitioncheck.sh`、`test/impactimportcheck.sh` | 集合划分、import 与 call 的不同单位 | 不能把 floor 当真实全量影响 |
| diff 与非法输入 | `test/situdiffcheck.sh`、`test/testgaterefusecheck.sh` | 多接口一致、子目录 root、坏输入不返回安静零值 | cx 的 JSON/错误契约必须独立测试 |
| 检索及预算 | `test/subtokencheck.sh`、`test/routecheck.sh`、`test/tokenbudgetcheck.sh`、`test/packtaskmonotoncheck.sh` | 正反例、预算边界、增大预算的行为 | 不能只测输出更小而不测答案是否完整 |
| 独立语料和任务评测 | `bench/recalleval/`、`bench/headtohead/`、`bench/agentloop/`、`docs/EVALS.md` | corpus lock、held-out、损失案例、完整流程成本 | 历史对其他工具的数据不能当 cx 的胜负证据 |

特别注意：ripwire 的 Graph 包含某些歧义候选分摊的边及相应标签；cx 第一版 impact 的默认保守传播策略与之不同。可以借鉴表示和测试方法，不能把这些边无条件转换成确定 reach。

参考实现不是 oracle。相同 fixture 下两个工具给出一样的答案，也可能是共享相同缺陷。必须先有独立 gold，再将 ripwire 作为可选诊断组。

如需移植源码而非借鉴思路，先检查双方 LICENSE、第三方代码来源和通知义务；不能将外部实现无标注地放入 cx。运行参考工具前核对其构建规则，不为了读参考代码自动安装、构建或修改 ripwire。

## 12. 评测交付物与复现结构

建议结构，实施时按阶段逐步新增，不要求一开始创建空文件：

```text
tests/fixtures/task_analysis/    小型语义/路径/歧义/预算案例及独立 oracle
tests/relation_identity.rs      身份与调用归属（也可扩展现有 target）
tests/impact.rs                 多跳、预算、证据与快照
tests/changes.rs                Git 两侧映射及非法输入
tests/context.rs                检索和打包契约
bench/task_analysis/
  README.md                    如何复现，不隐式修改用户仓库
  PREREGISTRATION.md           样本、分组、基线策略、预算和接受条件
  corpus.lock                  repo commit、源码/补丁和 grammar 身份
  tasks.dev.jsonl              dev 题目
  tasks.heldout.jsonl           holdout 题目；gold 单独受控保存
  gold/                        独立标准答案，不进入被索引目录
  run.py                       运行 A/B/C 与消融，保存所有步骤
  score.py                     读取独立 gold，禁止导入生产 resolver/ranker
  mutations/                   定向变异说明/补丁，仅作用于临时副本
  REPORT.md                    配对结果、失败、成本和判定
```

具体语言可调整，但必须统一 scoring 实现。新增 test target 需实际运行，不能只创建目录或空测试。大体积原始日志可放外部证据目录，在报告中记录可获取位置和 hash；不要提交用户源码、凭据或私有任务文本。

每条评测记录至少含：

```text
task_id / family / repo_commit / split / arm / candidate_commit
binary_sha256 / grammar_version_or_digest / index_version / schema_version
query_ladder_version / budget / cache_state / tokenizer
steps[{argv_or_tool_args, exit_code, stdout_ref, stderr_ref, wall, bytes, tokens}]
answer_ref / strict_satisfied / missing_items / forbidden_claims
coverage_limitations / total_calls / total_wall / total_tokens / peak_rss
oracle_hash / reviewer_notes / invalid_reason
```

invalid 案例保留原记录，不静默删除；报告列出计划 N、实际运行 N、有效 N、无效原因。环境无效不是产品成功；工具无法处理合法题目则是失败，不得改标环境无效来逃避分母。

## 13. 开发验证命令与集成门

遵循 `ACCEPTANCE_TEST_STANDARD.md` 保存命令、退出码、stdout/stderr，至少执行：

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --bins
cargo test --locked --tests
cargo test --locked --all-targets --all-features
```

相关新增 target 还要逐项执行，确认非零测试数。当前 crate 是 binary-only，不用不存在的 `--lib/--doc` target 冒充额外验证。本文未新增可执行代码，因此编写本文本身不要求运行上述测试。

涉及 Pi 扩展时执行：

```bash
npm run typecheck:pi
npm run test:pi
npm run verify:pi
```

先检查这些脚本的当前含义与依赖。新增工具必须覆盖 schema、argv、数值边界、路径逃逸、错误/partial、超预算输出、dirty path 刷新和取消。CLI 与工具适配同样输入应得到同样事实，不能两边各做一套图或 diff。

Pi 扩展运行 manifest 校验的固定版本 binary，不会自动使用本地刚构建的 `target/release/cx`。集成测试必须确认实际执行的 binary hash/version；不能只改 Rust 然后拿旧 release asset 验收新工具。打包、版本对齐和发布依现有发布文档，发布不是开发测试的隐含步骤。

性能沿用既有固定 ANGE corpus、参考主机预算和隔离 cache 规则；对新能力增加 A/B/C 配对测量。禁止直接对用户 cx/ANGE 主工作树运行会修改源码的 `scripts/bench.sh`。没有固定语料、grammar 或参考主机时如实报告缺失，不宣称完整发布级 PASS。

不允许以新增高级功能为理由放宽基础 overview/definition 的既有预算。基础阶段与每个新增阶段都记录 cold index、warm median/p95、RSS、DB bytes 和增量刷新成本，尤其关注原始调用点缓存是否把基础查询成本提高。

## 14. 报告、阶段退出与下一 session 起手任务

每阶段报告包含两个独立结论：

1. **工程验收 verdict**：沿用既有标准的 PASS / PASS_CORRECTNESS_PERF_UNGRADED / FAIL / INVALID，并说明本阶段范围；未运行全套发布门则不能称全项目发布 PASS。
2. **有效性结论**：质量收益成立 / 效率收益成立 / 与组合基线相当 / 无收益 / 证据不足。

必须列出：

- 实际实现范围与未完成项；
- 新增红色 fixture 的实现前后结果；
- 变异杀死清单和存活变异；
- A/B/C 的逐题胜负、严格满足集合、误导性断言和失败 fallback；
- 消融是否支持采用每个新增组件；
- 测试推荐的真实故障检出率（如果有推荐）；
- 所有成本指标，不能只摘最有利的一列；
- 已修改文件、索引/协议兼容性、原始证据位置和后续风险。

建议给新 session 的启动指令：

> 阅读 `docs/AGENT_TASK_ANALYSIS_PLAN.md` 及其必读引用。先只实施阶段 R：记录当前基线，设计独立 oracle 和会让错误实现失败的身份/调用归属/内容一致性/错误披露测试，再补强共享关系层。保留现有一跳接口和基础查询成本。阶段 R 完成后提交测试证据和下一阶段计划，不自动开始 impact/changes/context，不自动提交或发布。后续推进以用户确认范围为准。

最重要的停止规则：**如果新工具没有在独立任务和强基线上证明收益，就不要用增加字段、扩大索引或叠加更多启发式来冒充进展。先解释失败，再决定修正、简化或停止。**
