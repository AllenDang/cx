# pi-cx Extension 集成开发计划

> 状态：已实现；跨平台修订见下方
> cx 基线：`0e853f33438b669697d9a5bcee019d13919bf147` 之后已通过完整验收的主线
> Pi API 基线：`@earendil-works/pi-coding-agent` extension/package API
> 目标平台：macOS、Linux、Windows 的 arm64/x86_64
>
> **0.7.3 修订：** 本文中所有“第一版仅 darwin-arm64”、单一 `darwin-arm64` vendor 目录和单一 Pi asset 的描述，均由 0.7.3 的六目标矩阵取代。每个 Rust release target 都发布独立的 `pi-cx-<target>.tar.gz`，包含该目标的 cx binary 和 language-pack 1.16.1 grammars。Release CI 不再发布 crates.io 或更新 Homebrew。

## 1. 目标

在 cx 仓库内增加一个名为 `pi-cx` 的 Pi package，将已验收的 cx release binary 和常用 Tree-sitter grammars 作为 release asset 安装到 package 中，为 Pi 注册结构化代码查询工具。

集成必须保持 cx 的核心产品属性：

- 无 daemon；
- 无 LSP document lifecycle；
- 每次工具调用启动一个短生命周期 cx 进程；
- cx 是索引、freshness、identity、relations 和 map 语义的唯一实现；
- Extension 只负责 Pi tool schema、进程适配、结果协议、安装和诊断；
- Pi 当前 session 的 `ctx.cwd` 是唯一 project root；
- binary、grammar 或 schema 不匹配时严格失败，不回退 PATH 中的其他 cx；
- 安装完成后，内置语言的首次工具调用不访问网络。

## 2. 已确认的产品决策

| 主题 | 决策 |
|---|---|
| Package 名 | `pi-cx` |
| 分发 | cx 仓库本身作为 Git Pi package |
| 安装 | `pi install git:github.com/AllenDang/cx@<tag>` |
| Binary | Release workflow 构建，不提交 Git |
| Asset 获取 | package `postinstall` 下载同 tag asset 并校验 |
| 第一版平台 | `darwin-arm64` |
| Tool 形态 | 8 个强类型 `cx_*` 工具 |
| Agent 行为 | 只添加工具和 prompt guidance，不覆盖/拦截 read、grep、LSP |
| Project root | 固定 `ctx.cwd`，模型不能传任意 root |
| 输出协议 | cx 原始 JSON envelope 作为 tool content |
| Cache | 与独立 cx CLI 共用标准 cx cache |
| Refresh | 显式 `cx_refresh`，不监听 edit/write |
| 额外 grammar | TUI/RPC 中询问下载；非交互模式返回结构化错误 |
| 诊断 | 人类命令 `/cx-status` |
| 失败恢复 | 严格失败；不回退宿主 cx，不在工具调用期间自动修 binary |

## 3. 非目标

第一版不实现：

- Linux、Windows、macOS x86_64；
- npm 发布；
- 在安装时运行 Cargo；
- 在首次工具调用时下载 cx binary；
- 替换 Pi 内置 `read`、`grep` 或 LSP 工具；
- 自动监听 `edit`/`write` 并刷新索引；
- 一个 session 查询多个 project root；
- daemon、watcher 或常驻 cx 子进程；
- Extension 内重新实现 TOON、symbol resolution、import resolution 或 call graph；
- 在 cx schema v1 之外维护第二套结果事实模型；
- 自动将缺失 grammar 永久视为 unsupported。

## 4. Package 布局

cx 是 Rust 仓库，当前根目录没有 `package.json`。Git Pi package 通过仓库根安装，因此必须在根增加 package manifest。

建议布局：

```text
cx/
├── Cargo.toml
├── package.json
├── extensions/
│   └── pi-cx/
│       ├── index.ts
│       ├── tools.ts
│       ├── runner.ts
│       ├── protocol.ts
│       ├── binary.ts
│       ├── grammars.ts
│       ├── status.ts
│       └── types.ts
├── scripts/
│   ├── install-pi-cx.mjs
│   ├── build-pi-cx-asset.sh
│   └── verify-pi-cx-package.mjs
├── vendor/
│   └── pi-cx/
│       └── darwin-arm64/        # postinstall 生成；不提交 Git
│           ├── manifest.json
│           ├── bin/cx
│           └── grammars/*.dylib
├── tests/
│   └── pi-extension/
│       ├── package.test.ts
│       ├── protocol.test.ts
│       ├── runner.test.ts
│       ├── tools.test.ts
│       ├── grammar.test.ts
│       └── fixtures/
└── docs/
    └── PI_EXTENSION_INTEGRATION_PLAN.md
```

`vendor/pi-cx/` 加入 `.gitignore`。源码 checkout 不包含 native artifact；`npm install`/Pi package install 后才出现。

### 4.1 根 package.json

建议字段：

```json
{
  "name": "pi-cx",
  "version": "0.7.2",
  "private": true,
  "type": "module",
  "keywords": ["pi-package", "code-navigation", "tree-sitter", "ai-agents"],
  "pi": {
    "extensions": ["./extensions/pi-cx/index.ts"]
  },
  "scripts": {
    "postinstall": "node ./scripts/install-pi-cx.mjs",
    "test:pi": "tsx --test ./tests/pi-extension/*.test.ts",
    "verify:pi": "node ./scripts/verify-pi-cx-package.mjs"
  },
  "peerDependencies": {
    "@earendil-works/pi-coding-agent": "*",
    "@earendil-works/pi-ai": "*",
    "typebox": "*"
  },
  "devDependencies": {
    "tsx": "^4",
    "typescript": "^5"
  }
}
```

实现时可以选择 TypeScript test runner，但 runtime 依赖必须放在 `dependencies`；Pi 安装 Git package 时使用 production install，不能依赖 `devDependencies` 才能加载 extension。

### 4.2 版本单一来源

Cargo package version、Pi package version、Git tag、asset manifest 中的 cx version 必须一致。

增加机械检查：

```text
Cargo.toml package.version
== package.json version
== tag 去掉 v 前缀
== target/release/cx --version
== asset manifest.cx_version
```

任一不一致，release workflow 在上传 asset 前失败。

## 5. Release asset

### 5.1 Asset 名称

```text
pi-cx-aarch64-apple-darwin.tar.gz
pi-cx-aarch64-apple-darwin.tar.gz.sha256
```

Asset 与 cx 普通 binary asset 分开。普通 `cx-aarch64-apple-darwin.tar.gz` 继续只包含 cx CLI；Pi asset 包含 binary、grammar 和 manifest。

### 5.2 Asset 内容

```text
manifest.json
bin/cx
grammars/libtree_sitter_rust.dylib
grammars/libtree_sitter_typescript.dylib
grammars/libtree_sitter_tsx.dylib
grammars/libtree_sitter_python.dylib
grammars/libtree_sitter_go.dylib
grammars/libtree_sitter_c.dylib
grammars/libtree_sitter_cpp.dylib
```

用户选择的“9 种常用语言”是语言表面：

```text
Rust
C
C++
JavaScript
JSX
TypeScript
TSX
Python
Go
```

cx 中 JS/JSX/TS/TSX 共享 `typescript` 配置，实际需要 `typescript` 和 `tsx` 两个 grammar library，因此 asset 包含 7 个 dylib，覆盖 9 种文件表面。

Markdown 由 cx 自己解析，不需要 native grammar asset。

### 5.3 Asset manifest

建议：

```json
{
  "format_version": 1,
  "package": "pi-cx",
  "cx_version": "0.7.2",
  "cx_schema_version": 1,
  "target": "aarch64-apple-darwin",
  "tree_sitter_language_pack_version": "1.3.1",
  "languages": ["rust", "c", "cpp", "javascript", "jsx", "typescript", "tsx", "python", "go"],
  "files": {
    "bin/cx": { "sha256": "...", "bytes": 0 },
    "grammars/libtree_sitter_rust.dylib": { "sha256": "...", "bytes": 0 }
  }
}
```

每个文件都必须有 digest 和 bytes。安装器先解压到临时目录，再校验全部文件，最后 atomic rename 到 `vendor/pi-cx/darwin-arm64`。

### 5.4 Workflow 顺序

现有 Release workflow 扩展为：

```text
fmt/test/clippy/package gates
→ build aarch64-apple-darwin cx
→ 获取/构建固定 language-pack 1.3.1 grammars
→ 只选定 7 个 dylib
→ 生成 manifest + per-file SHA-256
→ 生成 pi-cx tar.gz + archive SHA-256
→ 本地 staged package smoke test
→ upload artifact
→ create GitHub Release
→ existing crate/Homebrew jobs
```

建议使用 GitHub artifact attestation 记录 provenance；第一版安装器的硬门是 archive `.sha256` 加 manifest per-file digest。

## 6. Git package 安装

### 6.1 postinstall URL

`install-pi-cx.mjs` 从 package version 构造固定 URL：

```text
https://github.com/AllenDang/cx/releases/download/v<version>/pi-cx-aarch64-apple-darwin.tar.gz
```

不得使用 `releases/latest`，否则 Git tag package 会悄悄搭配另一个 cx binary。

### 6.2 平台检查

只接受：

```text
process.platform === "darwin"
process.arch === "arm64"
```

其他平台安装失败并打印：

- 当前 platform/arch；
- 第一版支持矩阵；
- 不回退 PATH cx；
- 不尝试安装另一个架构。

### 6.3 下载与校验

安装器步骤：

1. 下载 archive 和 `.sha256` 到 package 内临时目录；
2. 校验 archive SHA-256；
3. 解压到临时 staging；
4. parse manifest；
5. 验证 version、schema、target、language-pack version；
6. 校验 manifest 中每个文件的 digest 和 bytes；
7. 校验没有 manifest 外的可执行文件、绝对路径或 `..` archive entry；
8. 给 `bin/cx` 设置 executable bit；
9. 运行 `bin/cx --version`；
10. atomic rename 到最终 vendor 目录；
11. 删除临时目录。

任何失败都必须使 `npm install`/`pi install` 非零退出，不能留下看似已安装的半成品。

### 6.4 CI/local staged mode

Release asset 在 GitHub Release 创建前还不存在。安装脚本需要一个仅供 CI 和开发使用的显式输入，例如：

```text
PI_CX_ASSET_DIR=/absolute/staging/path
```

设置时从本地 staging 安装，但仍执行完全相同的 manifest/digest/atomic-install 逻辑。未设置时只能走固定 release URL。

不得自动搜索 `target/release/cx`，避免本地测试无意中绕过 package asset。

## 7. Shared cache 与 grammar seed

### 7.1 标准 cache

Extension 不设置独立 `CX_CACHE_DIR`，让 cx 使用标准目录：

```text
macOS: ~/Library/Caches/cx
```

因此 standalone cx CLI 与 pi-cx 共享：

- `indexes/`；
- `grammars/`；
- language-pack manifest。

### 7.2 Seed 时机

postinstall 把 native asset 保留在 package vendor 目录。Extension 第一次执行任意 cx 工具前做轻量 `ensureBundledGrammars()`。这里的“共享标准 cx cache”明确指 standalone cx CLI 和 pi-cx 使用同一个 `~/Library/Caches/cx`：

1. 读取 package asset manifest；
2. acquire cache-local lock；
3. 检查 7 个目标 dylib；
4. 已有文件 digest 相同则跳过；
5. 缺失或 digest 不同则从 package 复制到 cache 临时文件；
6. fsync/close 后 atomic rename；
7. 释放 lock；
8. 不访问网络。

这样用户清理 cache 后，第一次工具调用仍可离线自修复 grammar。

### 7.3 共享 cache 冲突

由于选择了 shared cache，不能假设已有 grammar 一定来自同版本。规则：

- package asset manifest 是内置 binary 的期望版本；
- digest 相同：复用；
- digest 不同：在 lock 内用 package 版本原子替换目标 7 个 grammar；
- 不删除其他语言 grammar；
- 不删除已有 indexes；
- `/cx-status` 报告 replacement 和当前 digest；
- 所有替换行为写 stderr/诊断，不写入模型工具结果，除非失败。

如果未来多个 cx major 版本需要不兼容 grammars，应改成 versioned grammar cache；第一版不提前设计。

## 8. Binary 定位与严格验证

Extension 只能执行：

```text
<package-root>/vendor/pi-cx/darwin-arm64/bin/cx
```

禁止：

- `which cx`；
- PATH fallback；
- Homebrew fallback；
- 工具调用时下载 binary；
- 自动运行 `cargo build`。

每个 session 第一次调用前缓存一次验证结果：

1. realpath 仍在 package vendor root；
2. executable；
3. file digest 与 asset manifest 一致；
4. `cx --version` 等于 package version；
5. 一个最小 JSON probe 返回 `schema_version == 1`。

验证失败时所有 `cx_*` 工具抛出明确错误，提示重新安装 `pi-cx`。不使用未知 binary 继续运行。

Extension factory 不启动进程；验证延迟到 `/cx-status` 或第一次工具调用，符合 Pi 对 factory 不启动长期资源的约束。

## 9. Process runner

### 9.1 统一 runner

所有工具通过一个内部函数：

```ts
runCx({ cwd, command, args, signal, timeoutMs }): Promise<CxEnvelope>
```

要求：

- 使用 argv API，不通过 shell；
- command 固定为内置 binary；
- 总是传 `--root ctx.cwd --json`；
- cwd 同样设为 `ctx.cwd`；
- 传 Pi tool execute 的 AbortSignal；
- 默认 timeout 60 秒；
- stdout/stderr 分开捕获；
- 限制捕获 bytes，防止失控子进程占满内存；
- 退出时清理 child process tree；
- 记录 duration、exit code、killed、stderr 到 `details`；
- 不把 Pi session id/provider/model 等无关环境传给 cx；
- 只继承 cx 需要的 HOME、PATH、TMPDIR 和 cache 环境。

可以使用 `pi.exec()`，前提是它满足 argv、cwd、signal、timeout 和输出上限；否则使用 `node:child_process.spawn` 封装，并为取消/超时写集成测试。

### 9.2 Exit code

- exit 0 + valid envelope：正常返回；
- exit 1 + valid cx error envelope：向模型保留 `error.code/message`，并把 Pi tool 标记为失败；由于 Pi 只通过 throw 设置 `isError`，应抛出包含精简 cx error JSON 的错误；
- exit 2：Extension 参数映射缺陷或 cx CLI mismatch，抛出 protocol error；
- signal/timeout：抛出 cancelled/timeout；
- invalid JSON：抛出 protocol error，附有限 stderr；
- schema != 1：抛出 incompatible schema，提示安装匹配 tag。

零结果是 exit 0、`results: []`，绝不能作为错误。

### 9.3 Output 上限

用户选择原始 JSON envelope，因此正常情况下 `content[0].text` 是 cx stdout 原文，不重新序列化、不重排、不删除字段。

Pi 要求工具输出不超过 50KB/2000 行。为了保持 JSON 有效：

- Extension tool schema 不暴露无界 `--all`；
- 暴露 `limit`，范围 `1..200`；
- 暴露 `offset >= 0`；
- definition `maxLines` 上限 200；
- 子进程 stdout 超过 50KB 时，不可直接 `truncateHead()` 破坏 JSON；
- 保存完整 stdout 到临时文件，返回一个有效的 extension-level JSON truncation object，包含 cx query/page/error/freshness 摘要、原始 bytes/lines、临时文件路径和下一页建议；
- `details` 保存 truncation metadata，不复制超大正文。

默认查询必须依赖 cx pagination，而不是 Extension 猜测如何裁剪 facts。

## 10. Project root 与路径安全

### 10.1 Root 固定

每次调用：

```text
root = canonicalize(ctx.cwd)
```

Tool schema 中没有 `root` 参数。模型无法查询 `ctx.cwd` 外的另一个仓库。

### 10.2 Path 参数

接受 path 的工具：

- 去掉模型常见的单个前导 `@`；
- 相对 `ctx.cwd` 解析；
- reject NUL；
- canonicalize 已存在路径；
- 确认最终路径位于 root 内；
- 给 cx 传 root-relative path；
- 新文件 refresh 使用 lexical normalized absolute path，再验证父目录位于 root；
- symlink 逃出 root 必须拒绝。

`scope` 是 qualified-name glob，不按文件路径解析。`exclude` 是 map path glob，但不得被解释为 root。

## 11. 八个强类型工具

所有 string enum 使用 `StringEnum`，避免 Google tool schema 兼容问题。Extension 通过 `pi.registerTool()` 注册全部 8 个工具，并让它们默认 active。

公共可选参数：

```text
fresh: metadata | verified      default metadata
limit: integer 1..200
 offset: integer >= 0
noTests: boolean
```

Extension 将 camelCase 参数机械映射到 cx flags；每个工具有独立参数 builder，禁止通用“把对象键拼成 flags”。

### 11.1 cx_overview

用途：目录一层结构或文件 outline。

参数：

```text
path: string                    default "."
full?: boolean
noTests?: boolean
fresh?: metadata | verified
limit?: 1..200
offset?: >=0
```

Prompt：定位目录/文件结构时先用；不要为了找一段 raw text 使用。

### 11.2 cx_symbols

用途：仓库 symbol 搜索。

参数：

```text
name?: string glob
file?: project-relative path
kind?: cx SymbolKind
role?: definition | declaration | heading | unknown
scope?: qualified-name glob
kinds?: boolean
noTests?, fresh?, limit?, offset?
```

至少一个过滤条件或 `kinds=true`；禁止模型无条件请求整个 symbol corpus。

### 11.3 cx_definition

用途：读取一个 symbol body，替代整文件 read。

参数：

```text
name: string required
from?: project-relative file
kind?: SymbolKind
role?: definition | declaration | heading | unknown
scope?: qualified-name glob
maxLines?: 1..200
noTests?, fresh?, limit?, offset?
```

默认 `role=definition` 是否由 tool schema 注入，必须通过兼容性测试决定；不得改变用户显式请求 declaration/heading 的能力。建议默认注入 definition，以符合 Agent “读取实现”的主要意图。

### 11.4 cx_references

用途：语法过滤后的 references。

参数：

```text
name: string required
file?: project-relative path
context?: boolean
noTests?, fresh?, limit?, offset?
```

Tool description 明确：resolution 上限由 cx envelope 决定，不是编译器类型引用。

### 11.5 cx_callers

参数：

```text
name: string required
scope?: qualified-name glob
noTests?, fresh?, limit?, offset?
```

描述明确：只是一跳；unresolved edge 的空 target 和 candidates 必须原样保留。

### 11.6 cx_callees

参数同 callers。描述明确：ambiguous symbol 会拒绝任选函数体；没有多跳 depth。

### 11.7 cx_map

参数：

```text
depth?: integer 1..8           default 1
includeVendor?: boolean
includeGenerated?: boolean
tests?: boolean
exclude?: string[]             max 32
fresh?, limit?, offset?
```

保留 cx warnings 中的 ranking、external/ambiguous import 和过滤信息。

### 11.8 cx_refresh

参数：

```text
paths?: string[]               default [] = whole-project verified refresh
```

规则：

- 最多 200 paths；
- 每个 path 必须在 root 内；
- 不接受 `fresh/limit/offset`；
- 不自动由 edit/write hook 调用；
- prompt guideline：Agent 编辑后需要证明索引 generation 包含变化时显式调用；普通查询仍有 metadata 自动刷新。

## 12. Prompt guidance

只使用工具自己的 `promptSnippet` 和 `promptGuidelines`，不在每轮 `before_agent_start` 重写 system prompt。

Guideline 必须点名工具：

```text
Use cx_overview before reading a whole source file when only its structure is needed.
Use cx_symbols for identifier-oriented discovery; use grep for raw strings, logs, SQL, routes, and generated text.
Use cx_definition to read one implementation before falling back to a full-file read.
Use cx_references for syntax-classified occurrences and cx_callers/cx_callees for one-hop call evidence; do not treat unresolved edges as resolved.
Use cx_map for bounded repository orientation, not as a runtime dependency graph.
Use cx_refresh after edits when the next decision requires proof that the current index generation includes those paths.
```

不写“禁止使用 read/LSP”。现有工具是 cx 无法完成类型解析、raw text 搜索和编辑时的正确 fallback。

## 13. 缺失 grammar 的交互

### 13.1 检测

当 cx envelope、stderr 或 freshness 表明可识别语言缺 grammar 时：

1. 解析缺失的 cx language config name；
2. 如果语言在内置 9 种中，先重新运行 `ensureBundledGrammars()`；仍失败则 package 损坏，严格失败；
3. 如果是额外语言，按运行模式处理。

### 13.2 TUI/RPC

当 `ctx.hasUI` 为 true：

```text
Install missing cx grammar '<lang>' from the network?
```

用户确认后：

- 执行内置 binary `cx lang add <lang>`；
- 传 signal 和 timeout；
- 成功后原查询只重试一次；
- 返回结果 details 标记 `grammarInstalled` 和 `retried`；
- 用户拒绝则返回结构化 missing-grammar error。

不得在 extension factory 或 session_start 主动联网。

### 13.3 Print/JSON

`ctx.hasUI == false` 时不联网、不假定同意，返回：

```json
{
  "error": {
    "code": "grammar_not_installed",
    "language": "java",
    "fix": "<bundled-cx-path> lang add java"
  }
}
```

该错误必须使 Pi tool 标记为失败。

## 14. /cx-status

注册人类命令：

```text
/cx-status
```

不注册同名模型 tool。

输出：

- pi-cx package version；
- package root；
- target platform/arch；
- bundled binary path；
- binary digest status；
- `cx --version`；
- expected/actual schema version；
- `ctx.cwd` canonical root；
- cx cache path；
- cache writable；
- bundled 9 种语言的 grammar installed/digest 状态；
- 其他已安装 grammar；
- 当前 project index path、存在性、bytes；
- 最小 JSON probe 结果；
- overall `OK` 或逐项 failure。

命令不得修改 shared cache 或下载 grammar；它是纯诊断。若 package grammar 尚未 seed，报告 `available in bundle, not seeded`，不自动修复。最小 JSON probe 必须使用命令自己创建的临时 fixture 和临时 `CX_CACHE_DIR`，完成后删除，不能为了 status 给当前项目建索引。

TUI 用 `ctx.ui.notify` 给摘要，详细报告写入 transcript 或返回可复制文本；print/RPC 模式不依赖自定义 TUI。

## 15. Tool result 与 TUI

模型 content 保持 cx 原始 JSON。TUI 可提供轻量 renderer，但不得改变 content：

- call：`cx_definition Foo`、`cx_map depth=2`；
- partial：`indexing…` 或 `querying…`；
- collapsed result：结果数、generation、freshness mode、duration、warnings 数；
- expanded result：格式化原始 JSON；
- error：error code + message。

自定义 renderer 是展示层；任何 renderer 异常必须回退 raw content。

## 16. 测试策略

### 16.1 纯 TypeScript 单元测试

覆盖：

- 8 个 schema 与 flag builder；
- `StringEnum` 枚举；
- `@path` normalization；
- root escape/symlink escape；
- version/target/schema validator；
- manifest/digest validator；
- cx envelope parser；
- empty success vs cx error；
- oversized valid-JSON fallback；
- stderr 限制；
- grammar missing mode routing；
- binary strict failure；
- no PATH fallback；
- `/cx-status` pure diagnostic。

### 16.2 Fake binary integration

使用可执行 fixture 模拟：

- exit 0 valid envelope；
- exit 1 valid error envelope；
- exit 2；
- invalid JSON；
- schema 2；
- stdout >50KB；
- stderr flood；
- sleep timeout；
- abort signal；
- missing grammar → confirm → install → retry once；
- missing grammar in print/json → no network/no retry。

测试必须检查 argv 数组，证明没有 shell 拼接和任意 root。

### 16.3 Real cx fixture

使用 release binary 查询 `tests/fixtures/agent_corpus`：

- overview；
- symbols role；
- definition；
- references；
- callers/callees ambiguity；
- map filters；
- refresh generation；
- empty result；
- pagination。

结果集合与 cx Rust integration tests 的 exact counts 对齐，不能只断言非空。

### 16.4 Pi SDK integration

通过 Pi `DefaultResourceLoader` 或 extension test harness 加载 package：

- 8 个工具全部注册且 active；
- `/cx-status` 注册；
- tool schema 可被 Pi/Google-compatible provider 接受；
- tool execute 收到 `ctx.cwd`；
- tool result content 是原始 JSON；
- details 包含 duration/exit/binary version；
- cancellation 生效；
- TUI renderer failure 回退；
- print/json 模式不调用 UI。

### 16.5 Package install smoke

在临时 Pi agent dir：

1. 用 `PI_CX_ASSET_DIR` 安装 staged package；
2. 验证 vendor artifact；
3. 清空标准 cache 中测试隔离目录；
4. 第一次工具调用在断网条件下查询 9 种语言 fixture；
5. 验证 grammar 从 bundle seed；
6. 再次调用不复制、不下载；
7. 删除一个 grammar，验证离线自修复；
8. 篡改 binary，验证严格失败且不使用 PATH cx；
9. 篡改 grammar，验证按 manifest 恢复；
10. unsupported platform fixture 安装失败。

真实 GitHub Release 发布后再运行一次不设置 `PI_CX_ASSET_DIR` 的安装验证。

## 17. Release CI

新增 jobs：

```text
pi-extension-lint
pi-extension-unit
pi-extension-build-asset-darwin-arm64
pi-extension-staged-install
pi-extension-real-cx-fixture
pi-extension-pi-sdk-smoke
```

Release job只有在这些 job 和现有 Rust fmt/test/clippy 全绿后才能上传/tag release asset。

Tag release 的二元条件：

- source tree clean；
- Cargo/package/tag versions 一致；
- cx acceptance tests green；
- Pi extension tests green；
- asset manifest/digest green；
- staged install green；
- offline first-query green；
- binary strict-failure control green；
- asset 上传后 URL smoke green。

## 18. 分阶段实施

### Phase P0：Package skeleton 与协议测试

- 根 `package.json`；
- extension entry；
- protocol types；
- fake binary runner；
- 不接入真实 release asset。

**完成条件：** 8 个工具能被 Pi test harness 加载；fake binary 的 success/error/timeout/abort/schema mismatch 全部通过。

### Phase P1：Real binary 与强类型工具

- 统一 runner；
- 8 个工具和 flag builders；
- fixed `ctx.cwd`；
- raw JSON content；
- output overflow fallback；
- prompt guidance。

**完成条件：** real cx fixture exact counts 全绿；模型不能传 root；没有 shell。

### Phase P2：Asset builder 与 postinstall

- build asset；
- manifest/digest；
- download/local staged mode；
- atomic extract；
- strict platform/version/schema verification。

**完成条件：** 临时 Git package install 后 binary 可执行；篡改/半包/错误版本全部红。

### Phase P3：Bundled grammar 与 shared cache

- 7 dylib asset；
- 9 language surfaces manifest；
- cache lock + atomic seed；
- extra grammar interactive flow。

**完成条件：** 清 cache、断网后的首次工具调用可查询 9 种语言；非内置语言遵循 UI/非 UI 决策。

### Phase P4：/cx-status 与 TUI renderer

- 纯诊断 command；
- compact renderer；
- fallback render；
- mode matrix。

**完成条件：** status 不修改系统；TUI/RPC/JSON/print 均有确定行为。

### Phase P5：Release workflow 与真实安装验收

- CI jobs；
- release asset upload；
- Git tag postinstall；
- provenance/checksums；
- README 安装文档。

**完成条件：** 新机器执行一条 `pi install git:...@tag` 后，在断网状态启动 Pi 并成功调用 `cx_definition`。

## 19. 文档

实现必须同时新增/更新：

- README：`pi-cx` 一段简介和安装命令；
- `extensions/pi-cx/README.md`：工具表、平台、cache、grammar、错误处理；
- release docs：asset 内容和校验；
- security：package 执行 native binary、shared cache、安装时网络；
- troubleshooting：unsupported platform、checksum、schema、grammar、cache permission；
- acceptance report：固定 tag、Pi version、cx version、fixture counts、离线证明。

禁止写“支持所有平台”“完全 semantic”“自动理解所有引用”。沿用 cx 已验证的能力边界。

## 20. 最终验收

pi-cx 第一版只有在以下全部成立时完成：

1. `pi install git:github.com/AllenDang/cx@<tag>` 成功；
2. 安装时 asset URL 固定到 tag，不使用 latest；
3. binary 和每个 grammar digest 验证；
4. 不提交 native artifact 到 Git；
5. macOS arm64 外严格失败；
6. 8 个 `cx_*` 工具强类型、默认 active；
7. `/cx-status` 存在且纯诊断；
8. 每次调用固定 `ctx.cwd`，无 root 参数、无逃逸；
9. 不调用 shell、不回退 PATH cx；
10. AbortSignal 和 timeout 能终止子进程；
11. 原始 cx schema v1 JSON 进入模型 content；
12. 空结果不是错误，cx error 被 Pi 标记为 error；
13. 默认输出不超过 Pi 50KB/2000 行，overflow 仍返回有效 JSON；
14. 内置 9 种语言在 cache 为空、网络断开时首次调用成功；
15. 额外 grammar 只在 UI 确认后联网，非 UI 明确失败；
16. shared cache seed 有 lock、digest 和 atomic rename；
17. `cx_refresh` 仅显式调用；
18. 不覆盖 read/grep/LSP；
19. cx fixture exact counts、ambiguity 和 freshness 全部通过；
20. 安装包、release binary、Cargo version、package version、tag 完全一致；
21. 所有 Rust gates 与 Pi extension gates 全绿；
22. clean checkout 和新临时 Pi agent dir 上重复通过。

## 21. 实现顺序建议

严格按 `P0 → P1 → P2 → P3 → P4 → P5`。不要先写 release workflow 或 UI：

- P0 先证明 Pi tool contract；
- P1 证明 cx process/protocol 边界；
- P2 才引入 native distribution；
- P3 再处理 shared cache 和 grammar；
- P4 是诊断与展示；
- P5 最后闭合真实 Git tag 安装。

每个 phase 独立提交，并在 `docs/` 记录：问题、实现、延后项、测试和 measured cost。任何失败先修复当前 phase，不跨 phase 堆叠未验证行为。
