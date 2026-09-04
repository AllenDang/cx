# cx 完成版验收测试标准

> 用途：把本文件原样交给 cx 的开发 session，并要求它按顺序执行、保存证据、给出唯一 verdict。
> 适用基线：完成 `AGENT_NATIVE_DEVELOPMENT_ROADMAP.md` Phase 0–7 后的 cx。
> 原则：正确性是硬门；性能必须实测但区分主机；任何空 subject、部分运行、被管道掩盖的退出码都不算通过。

## 1. 最终 verdict

测试 session 最终只能给出以下一个结论：

- `PASS`：所有本机必跑硬门通过，真实 ANGE 验收通过，性能在参考主机预算内，工作树状态已如实记录；
- `PASS_CORRECTNESS_PERF_UNGRADED`：所有正确性硬门通过，但不是指定参考主机，性能只有数据、没有通过判定；
- `FAIL`：任一正确性、契约、非空性、隔离或参考主机性能硬门失败；
- `INVALID`：环境、grammar、固定 ANGE commit、计时工具或测试主体缺失，导致测试实际没有覆盖声明的对象。

不得使用“基本通过”“大体没问题”。`FAIL` 和 `INVALID` 必须列出最早失败的门、原始命令、退出码及 stdout/stderr 文件。

## 2. 不得破坏现场

### 2.1 开始前记录

在 cx 仓库根目录执行：

```bash
pwd
date -u '+%Y-%m-%dT%H:%M:%SZ'
uname -a
rustc --version
cargo --version
git rev-parse HEAD
git branch --show-current
git status --short
git diff --stat
git diff --cached --stat
```

把输出保存进本次测试目录。测试 session 不得：

- `git reset`；
- `git checkout -- <file>`；
- `git restore` 用户现有修改；
- 自动提交；
- 自动 re-pin 失败断言；
- 通过删除失败测试让 suite 变绿。

当前工作树是否 dirty 不决定实现正确性，但必须写进报告。发布级 `PASS` 还要求最终提交可在 clean checkout 中复现；开发验收允许在已记录的 dirty tree 上运行。

### 2.2 所有破坏性测试必须用临时副本

`scripts/bench.sh` 会临时修改一个被索引文件来测增量刷新。它只能指向：

- `mktemp` 创建的 fixture 副本；或
- detached disposable worktree。

禁止直接对用户的 cx 或 ANGE 工作树运行会写文件的 benchmark。

### 2.3 原始证据目录

```bash
export CX_TEST_RUN="$(mktemp -d "${TMPDIR:-/tmp}/cx-acceptance.XXXXXX")"
echo "$CX_TEST_RUN"
```

所有命令都必须分别保存：

- command line；
- exit code；
- stdout；
- stderr；
- wall/user/sys；
- peak RSS（性能命令）。

禁止用 `cmd | tail` 后读取 `$?`。需要管道时启用：

```bash
set -o pipefail
```

## 3. Gate A：源码与测试硬门

在 cx 根目录逐条运行，任何非零退出立即标记 `FAIL`，但继续收集其余门的证据：

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --bins
cargo test --locked --tests
cargo test --locked --all-targets --all-features
```

### 3.0 只运行本 crate 实际存在的 target

`--lib` 和 `--doc` 只在对应 Cargo target 存在时运行。cx 当前是 binary-only
crate（`Cargo.toml` 只声明 `[[bin]]`，没有 `[lib]`），unit test 编译进 binary
target，doctest 需要 library target。在这种布局下：

- `cargo test --lib` 与 `cargo test --doc` 会以 exit 101 失败并输出
  `no library targets found in package ...`；
- 这是布局与命令不匹配，不是覆盖缺失，也不构成实现缺陷；
- 因此 binary crate 使用 `--bins` / `--tests` / `--all-targets`。

判定规则：

```bash
# 仅当 crate 有 library target 时才跑 --lib / --doc
if cargo metadata --no-deps --format-version 1 \
   | python3 -c 'import json,sys; m=json.load(sys.stdin); \
sys.exit(0 if any(t["kind"]==["lib"] for p in m["packages"] for t in p["targets"]) else 1)'
then
    cargo test --locked --lib
    cargo test --locked --doc
else
    echo "no library target: --lib/--doc not applicable; --bins covers unit tests"
fi
```

跳过必须写进报告，并说明 unit test 实际由哪个 target 执行；不得用“没有 lib
target”掩盖真实的测试缺失——`--bins` 的测试数必须大于零。

不能只运行最后一个命令并假定它覆盖前面的不同模式。报告必须列出每条命令的：

- exit code；
- passed/failed/ignored 数量；
- 耗时；
- 如果失败，首个 failure block。

### 3.1 测试二进制完整性

以下 integration test target 必须实际运行且测试数大于零：

```bash
cargo test --locked --test fixture_corpus
cargo test --locked --test path_identity
cargo test --locked --test freshness
cargo test --locked --test qualified_identity
cargo test --locked --test map
cargo test --locked --test relations
cargo test --locked --test integration
```

若 target 不存在、显示 `0 tests`、被 filter 全部排除或仅编译未执行，结论是 `FAIL`。

### 3.2 禁止弱化断言

审核本次实现相关测试，满足：

- 已知结果数量使用精确值，不使用 `> 0`；
- ambiguous subjects 同时 pin unresolved target 和 candidate set；
- 空结果与错误结果分别断言；
- JSON key set、error code、freshness mode、generation 行为有精确断言；
- pagination 测试实际执行 `next_queries`，而不只检查字符串存在；
- metadata 的已知盲区仍有控制测试，不能被“修成”虚假强保证；
- direct relations 不以短名强行连接；
- 无 `--depth` 的关系查询约束仍被测试固定。

测试 session 若认为数字应更新，必须先解释行为变化并找出生产代码原因；不得把 re-pin 当机械工作。

## 4. Gate B：CLI 契约硬门

先构建 release binary：

```bash
cargo build --locked --release
export CX_BIN="$PWD/target/release/cx"
"$CX_BIN" --version
"$CX_BIN" --help
```

以下 subcommand 必须存在：

```text
overview symbols definition references callers callees map refresh lang cache skill
```

### 4.1 JSON envelope

对 fixture corpus 的每个查询命令运行至少一个成功、空结果、分页结果和错误结果。成功 JSON 的根必须始终是 object，并始终包含：

```text
schema_version
query
freshness
page
results
warnings
next_queries
error
```

硬要求：

- `schema_version == 1`；
- 成功时 `error == null`；
- 空成功为 `results: []`、exit 0；
- 查询失败为非空 `error.code`、exit 1；
- clap 参数错误 exit 2；
- `results`、`warnings`、`next_queries` 永远是 array；
- `page.total` 等于分页前总数；
- `page.truncated` 与实际剩余结果一致；
- `--json` 时 stderr 不重复结果事实；
- 非 JSON/TOON 输出保持可读且有截断提示。

### 4.2 Symbol role

fixture corpus 必须精确证明：

- C++ `validate_param` 有 1 个 definition 和 1 个 declaration；
- definition 排在 declaration 前；
- `--role definition` 只返回实现；
- `--role declaration` 不返回实现；
- Markdown heading 使用 `heading` role；
- 已建模语法不把无法确认的 declaration 静默标成 definition。

### 4.3 Qualified identity

必须证明：

- `alpha::run`、`beta::run`、`ange::EcsWorld::run` 不合并；
- declaration/definition 共享合理的 logical identity，但保留各自 role/location；
- C++ namespace + out-of-class method 组合成正确 qualified name；
- TypeScript class/interface scope 使用语言对应分隔规则；
- 无法建模的语言或 scope 返回 unqualified/unknown，而不是猜测。

### 4.4 References 与 direct relations

fixture corpus 的 `run` 是反空转控制。必须满足：

- callers 查询确实检查多个同名 `run`；
- `runner.run()` 不绑定到任意候选；
- unresolved edge 的 `to` 为空；
- unresolved edge 列出全部同语言 candidates；
- 跨语言同名 candidate 被排除；
- `alpha::run()` 和 `beta::run()` 即使位于同一行也形成两条不同 edge；
- 每条 edge 有 file、line、`evidence: call`、resolution；
- cx 从不输出 `type_resolved`，除非未来真的加入类型解析和相应红色控制；
- ambiguous `callees --name run` 拒绝任选一个函数体；
- leaf callee 查询是空成功，不是错误；
- callers/callees 没有多跳 `--depth` 参数。

### 4.5 Repository map

fixture corpus 必须满足：

- vendor/generated/tests 默认在排名前被排除；
- 各排除类都有数量和 opt-in 提示；
- `--include-vendor`、`--include-generated`、`--tests` 分别有效；
- `--exclude <glob>` 在 ranking 前生效；
- `run/get/new/name/std/string` 等低信息 symbol 不主导 API sample；
- resolved import/include 才形成 edge；
- ambiguous import 不形成 edge，并被报告；
- external import 有计数；
- ranking 原因出现在 JSON warnings 和非 JSON stderr；
- 列表字段有上限和明确 elision；
- 基础 `overview` 没有因 map 实现变重或改变契约。

## 5. Gate C：freshness、并发与路径身份硬门

### 5.1 Freshness

必须分别测试：

1. 普通 metadata 查询；
2. `--fresh verified`；
3. `cx refresh <paths>`；
4. 无参数 `cx refresh`。

硬要求：

- pure read 不增加 generation；
- committed index write 后 generation 单调增加；
- generation 只在持久化成功后对外可见；
- 相同 size + 相同 mtime 的内容变化：metadata 明确 miss，verified 和 paths 必须发现；
- 相同 size + 新 mtime：metadata 发现；
- size 变化 + 保持 mtime：metadata 发现；
- new/delete/rename 的 checked/updated/removed 计数正确；
- refresh 每个 path 返回 `updated/removed/unchanged/not_indexed` 之一；
- project root 外路径被拒绝，不得悄悄创建另一个项目索引；
- missing grammar 的计数只包含可识别但缺 grammar 的语言；
- 4 个并发 reader 看到一致 generation；
- writer + 4 reader 全部正常退出，结束后 index 可查询且不损坏。

### 5.2 Canonical path

在 macOS 上必须覆盖 `/tmp` 与 `/private/tmp`；Unix 通用测试使用 symlink alias。要求：

- alias 与 canonical path 产生同一个 cache identity；
- 从 alias 建索引、canonical path 查询成功；
- 从 canonical path 建索引、alias 查询成功；
- relative、absolute、`..`、symlink file path 都解析到同一 indexed file；
- project 外路径返回结构化错误；
- Windows CI 额外覆盖 drive letter/case/UNC 约定。

## 6. Gate D：真实 ANGE 固定语料验收

fixture 证明局部机制，ANGE 证明大型真实仓库没有把这些机制组合坏。必须使用固定 commit，不允许在用户 dirty tree 上直接写入。

### 6.1 固定 subject

```bash
export ANGE_REPO="$HOME/Documents/GodotProjects/ange"
export ANGE_COMMIT="70fe1922de2f05bec4a94f5967c68a92a8320b1a"
export ANGE_WT="$CX_TEST_RUN/ange"

git -C "$ANGE_REPO" cat-file -e "$ANGE_COMMIT^{commit}"
git -C "$ANGE_REPO" worktree add --detach "$ANGE_WT" "$ANGE_COMMIT"
```

测试完成后执行：

```bash
git -C "$ANGE_REPO" worktree remove --force "$ANGE_WT"
```

即使中途失败也必须 cleanup；建议用 `trap`。不得修改 ANGE 主工作树。

### 6.2 隔离 cache 与 grammar

```bash
export CX_CACHE_DIR="$CX_TEST_RUN/cache"
mkdir -p "$CX_CACHE_DIR"
"$CX_BIN" lang add cpp
"$CX_BIN" lang list
```

`cpp` 必须显示 installed。grammar 下载失败时 verdict 是 `INVALID`，不是把 C++ 空结果当成功。

### 6.3 冷索引

删除的只能是本次隔离 cache：

```bash
rm -rf "$CX_CACHE_DIR/indexes"
"$CX_BIN" --root "$ANGE_WT" --json symbols --limit 1
```

JSON 必须：

- exit 0；
- `page.total > 0`；
- `freshness.files_checked > 0`；
- `freshness.files_updated > 0`；
- 至少索引 C/C++ corpus；
- 没有数据库损坏或 lock error。

### 6.4 真实查询集合

逐条保存 JSON：

```bash
"$CX_BIN" --root "$ANGE_WT" --json overview src/engine/action/param_validator.cpp
"$CX_BIN" --root "$ANGE_WT" --json definition --name validate_stmt_against_action_spec --role definition --all
"$CX_BIN" --root "$ANGE_WT" --json references --name validate_stmt_against_action_spec --all
"$CX_BIN" --root "$ANGE_WT" --json callers --name validate_stmt_against_action_spec --all
"$CX_BIN" --root "$ANGE_WT" --json symbols --name EcsWorld --all
"$CX_BIN" --root "$ANGE_WT" --json map --depth 2
```

硬断言：

#### Definition

- 恰好 1 个 definition；
- file 为 `src/engine/action/param_validator.cpp`；
- body 包含函数签名和 `PARAM-002`；
- body 不只是 header prototype。

#### References

固定 commit 上文本总出现数为 35，其中：

- 26 个 call expressions；
- 1 个 definition；
- 1 个 declaration；
- 7 个 comments。

因此：

- `references` 必须恰好返回 28 个结构位置；
- 不得包含 7 个 comment-only 行；
- evidence 分类合计必须与 26 call + 1 definition + 1 declaration 一致；
- 所有 resolution 不得高于实现真实能力。

#### Callers

- 必须恰好返回 26 个 call sites；
- 每行 `evidence == "call"`；
- 不得把 definition/declaration 算作 caller；
- 不得出现 comment-only 行；
- unresolved/ambiguous edge 必须列 candidates，不得伪造 target。

#### EcsWorld

- 至少包含 `src/godot/bridge/ecs_world.h` 中 class definition；
- constructor declaration/definition 的 role 可区分；
- qualified identity 使用 `ANGE::EcsWorld` scope，而不是把全部短名合并；
- 字符串中的 Godot node name 不得成为 symbol definition。

#### Map

- 默认结果不包含 `thirdparty/` subsystem；
- warnings 报告 vendor/generated/test 的过滤或当前可证明的排除事实；
- API sample 不被 `std/string/name/run/get/new` 主导；
- 每个 dependency edge 来自 resolved import/include；
- 输出受 page/字段上限约束。

### 6.5 `/tmp` canonical 真实回归

`ANGE_WT` 位于 macOS `/tmp` 时，再用其 `/private/tmp/...` canonical spelling 查询同一文件，并比较：

- `cx cache path` 完全相同；
- definition 结果完全相同；
- 第二种 spelling 不创建第二个 index DB。

非 macOS 主机使用 symlink alias 完成等价控制。

## 7. Gate E：性能与内存标准

性能只在相同参考主机上作硬判定：Apple arm64/macOS，仓库位于本地 SSD，无并发编译/索引任务。其他主机记录数据并使用 `PASS_CORRECTNESS_PERF_UNGRADED`。

### 7.1 测量规则

- release binary；
- 固定 ANGE commit；
- 复用已下载 grammar；
- cold index 表示删除 cx index、但先做一次不计时文件遍历以减少工具顺序偏差；
- warm query 先 warmup 1 次，再测至少 9 次；
- 报 median、p95、max RSS、stdout bytes；
- `/usr/bin/time -l` 的退出码直接读取，不经过管道；
- 不以第一次网络下载或全冷 OS page cache 冒充纯索引成本。

可在 disposable ANGE worktree 上运行：

```bash
CX_BIN="$CX_BIN" scripts/bench.sh "$ANGE_WT" 9
```

运行前后必须证明 worktree clean：

```bash
git -C "$ANGE_WT" status --short
```

### 7.2 参考主机硬预算

#### Cold index

| 指标 | PASS 上限 |
| --- | ---: |
| wall | 5.0 s |
| peak RSS | 160 MiB |
| index DB | 32 MiB |

#### Warm 基础查询

| 查询 | median | p95 | peak RSS |
| --- | ---: | ---: | ---: |
| root overview | 250 ms | 400 ms | 96 MiB |
| file overview | 250 ms | 400 ms | 96 MiB |
| definition | 250 ms | 400 ms | 96 MiB |
| symbol search | 250 ms | 400 ms | 96 MiB |
| references | 500 ms | 1.0 s | 128 MiB |
| map depth 2 | 500 ms | 1.0 s | 128 MiB |
| callers/callees | 750 ms | 1.5 s | 192 MiB |

#### Incremental

| 指标 | PASS 上限 |
| --- | ---: |
| `cx refresh <one path>` | 500 ms |
| metadata 自动发现一个普通编辑 | 750 ms |
| verified 全 C/C++ corpus | 2.0 s |

这些是防止数量级回退的宽预算，不是优化目标。失败时先确认没有网络下载、debug binary、并发 Cargo、Spotlight/杀毒扫描或 HDD；无法排除环境干扰则性能结论为 `INVALID`，不得直接 re-pin。

### 7.3 输出预算

真实 ANGE 查询的默认输出必须保持有界：

| 查询 | 默认 stdout 上限 |
| --- | ---: |
| root overview | 4 KiB |
| EcsWorld symbol search | 8 KiB |
| references 默认页 | 16 KiB |
| map 默认页 | 16 KiB |
| callers/callees 默认页 | 16 KiB |

`--all` 明确请求全量时不使用以上输出上限，但仍必须有正确 `page.total`。默认输出超过预算是 `FAIL`，不能通过删除必要 ambiguity/evidence 字段来压缩。

### 7.4 无常驻内存

执行 100 次混合查询后：

- 每个命令均退出 0；
- 不存在残留 `cx` 进程；
- 没有 watcher/daemon 被隐式启动；
- index DB 大小在无源码变化时稳定；
- generation 在纯查询期间不增加；
- 单次 peak RSS 不随迭代序号持续上升。

这项是 cx 相对 LSP 的核心产品约束。

## 8. Gate F：安装与打包

开发验收至少运行：

```bash
cargo package --locked --allow-dirty
```

发布验收必须在 clean checkout 运行，不使用 `--allow-dirty`：

```bash
cargo package --locked
cargo install --locked --path . --root "$CX_TEST_RUN/install"
"$CX_TEST_RUN/install/bin/cx" --version
"$CX_TEST_RUN/install/bin/cx" --help
```

安装后的 binary 必须重复一个 fixture JSON 查询，证明不是只测试了 `target/release/cx`。

如果 release asset/安装脚本属于本次发布范围，还需分别验证 macOS arm64、Linux x86_64；Windows 的 path identity 和 PowerShell installer 由对应平台 CI 验证，不能用“本机没有 Windows”宣称通过。

## 9. 测试报告格式

开发 session 最终报告必须严格包含：

```text
VERDICT: PASS | PASS_CORRECTNESS_PERF_UNGRADED | FAIL | INVALID

SUBJECT
- cx commit:
- branch:
- initial worktree status:
- host:
- rustc/cargo:
- ANGE commit:

CORRECTNESS GATES
- fmt:
- clippy:
- unit tests (--bins, or --lib when a library target exists):
- integration targets:
- doctests (only when a library target exists; otherwise "n/a: binary-only crate"):
- package/install:

CONTRACT RESULTS
- JSON envelope:
- symbol roles:
- qualified identity:
- freshness/generation:
- references:
- callers/callees:
- repository map:
- canonical paths:

ANGE EXACT COUNTS
- definition: expected 1 / actual N
- references: expected 28 / actual N
- callers: expected 26 / actual N
- comment-only false positives: expected 0 / actual N

PERFORMANCE
- cold wall/RSS/index bytes:
- warm median/p95/RSS/output bytes per query:
- incremental metadata/verified/paths:
- 100-query residual process/generation check:

CHANGED DURING TEST
- files changed by test:
- cleanup result:
- final worktree status:

FAILURES OR RESIDUAL RISKS
- exact command, exit code, evidence path, interpretation
```

报告中的每个 `PASS` 必须能指向 `$CX_TEST_RUN` 下的原始证据。只引用 Phase 文档中的历史数字不算本次测试。

## 10. 停止条件

遇到以下任一情况，立即把最终 verdict 至少降为 `FAIL`，即使其余测试是绿色：

- 测试改写或删除用户工作；
- 固定 ANGE commit 不存在却换成另一个 commit；
- C++ grammar 未安装却接受空结果；
- 任一 integration target 运行 0 tests；
- JSON 根类型随结果数量变化；
- metadata 输出声称 content-verified；
- unresolved relation 被绑定到任意同名 symbol；
- 真实 ANGE references/callers 数量与固定值不符且没有定位生产代码原因；
- 默认 map 被 vendor/generated/common symbol 主导；
- benchmark 在 dirty 用户工作树上修改文件；
- 通过 pipeline 尾命令读取了错误退出码；
- 用 re-pin、放宽预算或删测试代替定位回退。

## 11. 推荐执行顺序

```text
记录现场
→ Gate A 源码/测试
→ Gate B CLI 契约
→ Gate C freshness/并发/路径
→ 构建 release
→ Gate D 固定 ANGE 正确性
→ Gate E 性能/内存
→ Gate F package/install
→ 对比初始/最终工作树
→ 输出唯一 verdict
```

正确性失败后仍可继续收集性能数据，但性能数字只能标记为“失败实现上的诊断数据”，不能抵消正确性失败。
