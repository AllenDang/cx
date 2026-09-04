# cx Agent 原生代码查询开发路线

> 状态：后续开发指导文档
> 基线：cx 0.7.2，索引版本 `INDEX_VERSION = 8`
> 目标：把 cx 从“面向 Agent 的轻量 symbol 查询 CLI”发展为“无常驻进程、结果有界、证据等级明确、可稳定集成的 Agent 代码查询层”。

## 1. 为什么以 cx 为基础

2026-09-04 在 ANGE 仓库上对 `cx`、`codes`、`cona`、`srcwalk` 做了同机测试。测试项目包含：

- 13,109 个 tracked files，约 500 MB；
- 2,088 个 C/C++ source/header，约 34 MB；
- 大量 Markdown、Python、JavaScript、YAML 和项目自定义 DSL 文件。

在 C++ 查询可用的候选中，cx 最接近目标：

- Rust 单一 CLI；
- Tree-sitter 解析；
- 无 daemon、无 LSP document lifecycle；
- 持久化增量索引；
- `overview → symbols → definition/references` 的 Agent 查询阶梯；
- 输出紧凑；
- 修改文件后，普通查询能自动更新索引。

ANGE 实测基线：

| 项目 | cx 0.7.2 |
|---|---:|
| 空工具索引、热文件系统建索引 | 2.04 s |
| 建索引峰值 RSS | 102.5 MiB |
| 索引大小 | 16.1 MiB |
| warm outline | 0.08 s / 36.2 MiB RSS |
| warm definition | 0.08 s / 36.0 MiB RSS |
| warm references | 0.19 s / 44.5 MiB RSS |
| warm symbol search | 0.09 s / 36.3 MiB RSS |
| root overview | 0.08 s / 766 B 输出 |
| 单文件变化自动发现 | 0.39 s，正确更新 1 个文件 |

所有 cx 进程在命令结束后退出，没有会话级常驻内存。当前短板不是基本性能，而是身份、语义边界、输出契约和大型项目查询质量。

## 2. 产品边界

### 2.1 必须做到

cx 必须为 Agent 提供：

1. 文件和目录结构查询；
2. 精确 symbol body 读取；
3. definition 与 declaration 区分；
4. 语法级 reference/call 查询；
5. 每条结果的证据类型和解析等级；
6. 可检查的索引新鲜度；
7. 稳定、有界、可版本化的 JSON 输出；
8. 无常驻进程；
9. 不要求项目能够编译；
10. 查询失败或不确定时明确降级，不伪装成完整语义结果。

### 2.2 暂不追求

以下能力不应在缺少可靠基础时提前承诺：

- 编译器级 overload resolution；
- C++ template 实例化；
- macro expansion 后的完整语义；
- virtual/interface 动态派发；
- Python 动态绑定的精确解析；
- 与 LSP 完全等价的 rename/find implementations；
- 仅凭同名 identifier 构造的“完整 call graph”。

Tree-sitter 提供的是结构事实。语言专用 scope/import 规则可以逐步提高解析等级，但结果必须标注真实能力。

## 3. 当前架构事实

后续设计必须从当前实现出发，而不是假设 cx 已经拥有语义图。

### 3.1 索引模型

`src/index.rs` 当前使用 redb，包含：

- `META_TABLE`；
- `FILES_TABLE`；
- `SYMBOLS_TABLE`；
- 启动时加载到 `Index.entries: HashMap<PathBuf, FileData>` 的内存镜像。

当前 `Symbol` 只有：

```rust
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub signature: String,
    pub byte_range: (usize, usize),
    pub is_test: bool,
}
```

它没有：

- declaration/definition role；
- qualified name；
- owner/scope；
- stable symbol ID；
- language-specific identity；
- graph edge；
- resolution confidence。

任何 call graph 或语义查询都必须先补齐身份模型。

### 3.2 新鲜度模型

`src/index.rs::needs_update()` 当前：

- 每次查询遍历项目文件；
- 用 relative path 与缓存项对应；
- 主要以 `mtime` 判断变化；
- 用文件数量差异发现删除；
- 需要更新时切换为独占 redb 打开并重建变化文件。

这使普通编辑能自动更新，但“索引已验证等于磁盘内容”的强保证尚不存在。相同高精度 mtime、相同文件数量等边界必须有测试和明确契约。

### 3.3 References 模型

README 明确说明 references 在查询时通过 AST walking 计算，不写入持久索引。ANGE 样本中：

- 26 个调用；
- 1 个定义；
- 1 个声明；
- 7 个注释文本。

cx 返回 28 个结构位置，正确排除了 7 个注释。这证明语法过滤有效，但 28 个结果仍是 syntax evidence，不等价于编译器解析后的 symbol references。

### 3.4 JSON 模型

`src/query.rs` 当前 JSON 在分页时使用 envelope，不分页时直接输出 array。相同命令会因结果数量不同改变根类型，不适合作为长期 Agent API。

## 4. 已测出的具体问题

### 4.1 canonical path 身份不一致

macOS 中 `/tmp/x` 可能 canonicalize 为 `/private/tmp/x`。测试中曾出现：

```text
cx: file not in index: /private/tmp/...
```

重建时统一使用 canonical root 后恢复。当前 `cache_path_for()` 会 canonicalize root，但 `Index.root`、相对路径计算和查询输入不一定共享同一个 canonical identity。

### 4.2 declaration 被当成 definition

C++ outline 中，forward declaration 与函数定义都使用 `definition.function` 类捕获，产生同名重复条目。例如：

```cpp
void validate_param(...);
void validate_param(...) { ... }
```

Agent 无法只靠当前字段机械地区分二者。

### 4.3 缺少 qualified identity

`EcsWorld` 查询能返回 class、constructor declaration、constructor definition 和 destructor，但它们仍主要靠 `name + file + range` 表示。若未来直接以 `run`、`find`、`get` 等短名连接 callers/callees，会产生大量错误图边。

### 4.4 新鲜度不可观察

当前 Agent 能得到结果，却不能从 JSON 判断：

- 检查了多少文件；
- 更新了多少文件；
- 使用 mtime 还是 content hash；
- 结果来自哪个 index generation；
- 是否跳过了缺 grammar 的文件。

### 4.5 “semantic”措辞过强

当前结构查询非常有用，但 CLI、README 和输出必须避免让用户把 syntax-level evidence 理解为 compiler-resolved semantics。

## 5. 目标数据模型

先扩展事实模型，再增加高级查询。

### 5.1 SymbolRole

新增独立于 `SymbolKind` 的角色：

```rust
pub enum SymbolRole {
    Definition,
    Declaration,
    Heading,
}
```

不要通过“是否有 `{}`”做跨语言通用猜测。每种语言的 query 应显式捕获：

```text
@definition.function
@declaration.function
```

如果 grammar 无法可靠区分，使用明确的 `Unknown`，不要默认为 definition。

### 5.2 StableSymbolId

建议以可序列化字段组成稳定身份，而不是直接持久化 display string：

```rust
pub struct StableSymbolId {
    pub language: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub signature_key: Option<String>,
}
```

要求：

- 同文件重排但 symbol 身份不变时，ID 尽量稳定；
- declaration 与 definition 可以共享 logical ID，但保留各自 location/role；
- 无法解析 scope 时，必须降低 resolution level；
- display name 与 identity 分离。

### 5.3 Scope/owner

为 symbol 增加：

```rust
pub owner: Option<StableSymbolId>,
pub scope_path: Vec<String>,
pub qualified_name: String,
```

第一阶段只需要 lexical owner：namespace、class、module、enclosing function。不要在这一阶段声称已解析 imports 或 types。

### 5.4 Evidence 与 Resolution

所有 references/graph edge 使用统一等级：

```rust
pub enum EvidenceKind {
    Definition,
    Declaration,
    Call,
    TypeReference,
    IdentifierReference,
    Import,
    Text,
}

pub enum ResolutionLevel {
    Text,
    Syntax,
    LexicalScope,
    ImportResolved,
    TypeResolved,
}
```

规则：输出只能标注实际达到的最高等级。Tree-sitter node 类型识别出的调用一般是 `Syntax`；只按名字匹配的结果不能标成 `LexicalScope`。

## 6. 稳定 Agent API

### 6.1 JSON 永远使用 envelope

`--json` 应始终返回 object，不因分页与否改变根类型：

```json
{
  "schema_version": 1,
  "query": {
    "kind": "references",
    "subject": "validate_stmt_against_action_spec"
  },
  "freshness": {
    "generation": 42,
    "mode": "metadata",
    "files_checked": 3905,
    "files_updated": 1,
    "files_skipped_missing_grammar": 2
  },
  "page": {
    "total": 28,
    "offset": 0,
    "limit": 50,
    "truncated": false
  },
  "results": [],
  "warnings": [],
  "next_queries": []
}
```

### 6.2 错误与空结果分开

以下状态不得都用“空 array + exit 0”表达：

- 查询成功但没有结果；
- 文件不在索引；
- grammar 未安装；
- 查询语法不支持；
- 索引损坏；
- 结果因预算被截断；
- symbol 名称有多个候选且无法消歧。

JSON 中提供机器可读 code；CLI exit code 保持简单、稳定并写入文档。

### 6.3 输出预算

所有可能扩张的命令必须支持：

- `--limit`；
- `--offset`；
- path/language/kind/role filters；
- `truncated`；
- 精确的下一页命令；
- 可选 `--token-budget`，但不能为满足预算而静默改变事实含义。

## 7. 新鲜度设计

不要未经测量就把所有文件每次完整 hash。ANGE tracked corpus约 500 MB，语言源文件集合也因 grammar 配置而变化。

应先实现并比较三种模式：

1. `metadata`：path + size + high-resolution mtime，作为快速默认；
2. `verified`：对查询覆盖范围或全部 indexable files 做 content hash；
3. `paths`：Agent 编辑后显式传入变化路径，立即 hash/reparse 指定文件。

建议接口：

```bash
cx refresh src/a.cpp src/b.h
cx --fresh metadata definition --name Foo
cx --fresh verified references --name Foo
```

Agent harness 的推荐流程：

```text
编辑文件 → cx refresh <changed paths> → 后续查询
```

普通查询仍保留自动 metadata refresh。每个结果必须返回 freshness mode 和 index generation。

验收不能只检查“能看到新 symbol”，还要覆盖：

- 内容变化但文件大小不变；
- 尽可能构造相同 mtime 的变化；
- 新文件；
- 删除文件；
- rename；
- grammar 新安装后原先跳过的文件；
- 两个 cx 进程并发读；
- 一个进程更新、另一个进程查询。

## 8. Repository orientation

当前 `cx overview .` 很快且紧凑，应保留它作为最低成本入口。高级 orientation 另设命令，不要让基础 overview 变重。

建议分层：

```text
overview       一层目录和 symbol 计数
map            有界 repository map
impact         明确标注证据等级的反向关系
```

`map` 第一版只使用可证明事实：

- directories/subsystems；
- files/symbol counts；
- imports/includes；
- public/exported definitions；
- test 与 production 分类；
- changed files/symbols。

排序前必须支持过滤：

- vendor/thirdparty；
- generated；
- tests；
- docs；
- 用户配置的 path globs；
- common/low-information symbols。

不要重现按 `std`、`string`、`name`、`run` 等通用名字计数后主导排名的问题。

## 9. Callers/callees 的开发顺序

禁止先做“按名字连接全仓库”的多跳 call graph。正确顺序：

1. SymbolRole；
2. qualified name 和 lexical owner；
3. direct call AST evidence；
4. 同 lexical scope 消歧；
5. import/include resolution；
6. bounded callers/callees；
7. 最后才考虑多跳 impact。

每条边都必须保存：

```rust
pub struct RelationEdge {
    pub from: StableSymbolId,
    pub to: Option<StableSymbolId>,
    pub evidence: EvidenceKind,
    pub resolution: ResolutionLevel,
    pub location: SourceRange,
    pub ambiguous_candidates: Vec<StableSymbolId>,
}
```

无法唯一解析时保留 unresolved/ambiguous 状态，不能任意选一个目标。

## 10. 性能原则

### 10.1 不引入 daemon

无常驻进程是 cx 的核心差异，不应为了省几十毫秒引入 LSP 式生命周期。允许：

- redb/SQLite/mmap 持久索引；
- 每次命令短生命周期进程；
- OS page cache；
- 可选显式 watcher，但 watcher 不能成为正确性的唯一来源。

### 10.2 评估内存镜像

当前每次启动把 redb 内容加载到 `HashMap<PathBuf, FileData>`。ANGE warm 查询约 36–45 MiB RSS，目前可接受，但大型 monorepo 可能线性增长。

只有测量证明需要时，再比较：

- 继续全量内存镜像；
- 按查询从 redb 读取；
- mmap compact index；
- name/path secondary index；
- 分语言或分目录 shard。

不能只降低 RSS 而让常见查询退化为全库磁盘扫描。

### 10.3 性能结果与正确性结果分开

性能阈值受硬件影响，不宜直接作为跨平台绝对 CI gate。CI 必须严格 pin：

- 结果集合；
- role/evidence/resolution；
- 分页行为；
- 输出 schema；
- 空结果与错误；
- 新鲜度。

性能 benchmark 记录：

- cold index wall/user/sys；
- peak RSS；
- index bytes；
- warm query median/p95；
- output bytes/tokens；
- incremental refresh files/time。

## 11. 实施阶段

### Phase 0：固定基准和回归语料

新增可复现 benchmark/fixture，至少包含：

- C++ declaration + definition；
- namespace/class/member qualified names；
- 两个不同 scope 中同名 `run`；
- 注释和字符串中的同名文本；
- include/import；
- test/production/vendor/generated paths；
- symlink/canonical root；
- 修改、删除、rename；
- pagination 和超预算结果。

**完成条件：** 当前行为有可重复记录；所有后续数据模型变化有 binary expected output。

### Phase 1：统一路径身份

修改重点：

- `src/main.rs` root resolution；
- `src/index.rs::cache_path_for`、`Index.root`；
- `src/query.rs::make_relative` 和 file filter；
- Windows drive-letter/case 行为。

**完成条件：** `/tmp/project` 与 `/private/tmp/project` 指向同一 index；任一别名下 build、另一别名下 overview/definition 均成功。

### Phase 2：SymbolRole 与 schema migration

修改重点：

- `src/index.rs::Symbol`；
- `src/language/extract.rs` capture 解析；
- `src/language/queries/*`；
- bump `INDEX_VERSION`；
- query filters/output。

**完成条件：** C/C++ forward declaration 与 definition 可机器区分；旧 index 自动安全重建。

### Phase 3：JSON v1 contract

修改重点：

- `src/query.rs`；
- `src/output.rs`；
- `src/main.rs`；
- `src/skill.md` 和 README。

**完成条件：** 所有 JSON 命令始终返回同一 envelope；schema golden tests 覆盖 0、1、截断、多义和错误结果。

### Phase 4：freshness contract

实现 generation、freshness metadata、显式 refresh paths，并实测 metadata/verified 成本。

**完成条件：** Agent 编辑后可以机械证明指定文件已进入当前 generation；查询输出显示检查与更新数量。

### Phase 5：qualified symbol identity

先支持 Rust、C++、TypeScript 三种不同语言模型，再抽象公共接口，避免只按某一种语言设计。

**完成条件：** 两个 scope 中同名 symbol 不会在 definition、reference 或 direct caller 查询中被静默合并。

### Phase 6：有界 repository map

加入 import/include graph、path filters 和低信息 symbol 抑制。保持 `overview` 的现有速度和简洁度。

**完成条件：** ANGE 类项目的 map 顶部不被 thirdparty/common identifiers 主导；输出受预算约束并解释排序依据。

### Phase 7：direct callers/callees

只在 identity 和 evidence 模型稳定后实现。

**完成条件：** direct edges 有 location、resolution 和 ambiguity；`run` fixture 不产生跨 scope 假边。

## 12. 每个阶段的开发纪律

每项修改必须回答：

1. Agent 提出的具体查询是什么？
2. 当前输出为什么不足？
3. 新字段是事实、推断还是文本证据？
4. 无法解析时如何降级？
5. 输出是否有界？
6. 修改后索引如何迁移或重建？
7. 是否增加常驻状态？
8. 如何构造会让错误实现变红的 fixture？
9. 结果数量是否精确 pin，而不是只断言 `> 0`？
10. benchmark 是否记录输出 bytes/tokens，而不只记录速度？

禁止：

- 用同名字符串搜索结果冒充 resolved reference；
- 用 README 声明代替 fixture；
- 用 existence check 代替已知精确数量；
- 静默丢弃 ambiguous candidates；
- 因输出预算而不标注截断；
- 先引入 daemon，再用“可选”掩盖正确性依赖；
- 在没有基准数据时重写存储层。

## 13. 建议的下一项工作

第一项实现应是 **Phase 0 + Phase 1**：

1. 把 ANGE 中测出的 `/tmp`/`/private/tmp` 情况缩成最小 fixture；
2. 为 project root、cache key、`Index.root`、query file path 定义唯一 canonical identity；
3. 加入跨别名 build/query 测试；
4. 保持现有 CLI 输出不变；
5. 重跑 cold/warm/incremental benchmark，证明没有性能回退。

这是一个范围小、验收二元、能直接消除真实失败的起点。完成后再进入 SymbolRole；不要同时开始 call graph。

## 14. 最终完成定义

cx 可以称为稳定的 Agent-native code query layer，至少需要满足：

- 无 daemon，命令退出后无残留进程；
- project/file identity 在支持平台上稳定；
- declaration/definition 可区分；
- symbol 有 qualified identity 或明确 unresolved；
- references 标注 evidence 与 resolution；
- JSON schema 有版本且根类型稳定；
- 每次结果携带 freshness；
- 所有扩张查询可分页、可过滤、可标注截断；
- 多义结果不被静默合并；
- repository map 不被 vendor/generated/common names 主导；
- correctness fixture 精确 pin 结果集合；
- ANGE 规模下 warm 基础查询保持亚秒级，索引仍是轻量磁盘状态；
- README 不把 syntax evidence 宣称为编译器级 semantics。

在这些条件之前，cx 应继续定位为“高效、结构感知的 Agent 查询 CLI”，而不是 LSP 或编译器语义服务的完整替代品。
