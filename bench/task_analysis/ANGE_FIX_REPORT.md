# ANGE 缺陷修复与重新验收（TODO #10）

## 判定

**本次修复范围：PASS。** 已定位的 cast/模板 callee、引用返回定义、正文选择、scoped
unresolved frontier 和失败定位缺陷均有回归控制；全部转绿，重新执行的冻结 ANGE 题为
**8/8 证据充分**，没有修改题目、gold 或 v2 流程策略。

**不宣称整体效率胜出。** 增强关系优先流程仍比已有工具的 source-first 强基线昂贵。
这不影响本次缺陷修复完成，但不能当成采用更多高级工具或自动推进 impact 的理由。
本报告是本机、当前修复范围的验收，不是 clean-checkout、多平台、Pi release asset 的发布 PASS。

历史失败、原始数字和协议修订保留在 [原真实任务报告](ANGE_TASK_REPORT.md)，没有覆盖为绿。

## 修复内容

1. **按 grammar 的 callee 字段走 AST，不再向最右叶子取名。**
   - C++ template_function/template_method/template_type 使用 name，Rust generic_function
     使用 function；限定名和接收者保留。
   - C/C++ primitive functional cast 与四种显式 cast 不产生普通函数调用，但仍遍历操作数，
     保留其中真正的调用。Rust 中同名普通函数不被误删。
   - 函数返回值再调用、下标选择的动态 callee 不再伪造 argument/key 调用，明确披露
     `unsupported_call_form`，而不是把空结果当完整分析。只承诺命名的语法调用，不声称运行时全集。
2. **C++ 查询覆盖 reference_declarator。** 自由函数、类成员、声明/定义、T&/T&&、模板均有控制。
   `INDEX_VERSION` 从 13 升至 **14**：即使 source size/mtime/hash 不变，也重建旧索引缺失的
   符号事实。实际修改旧 redb 符号表并盖回版本 13 的测试证明重建与后续 warm 读取均正确。
3. **正文选择与声明等价性分开。** Callees 默认选匹配的 definition bodies；不要求先证明
   static/类型别名/参数文本不同的声明与定义等价。未关联声明明确警告并仍保留在 target resolver。
   多个 definition sites 仍拒绝合并；仅有声明时仍明确没有正文。
4. **Callees 不为读取正文而解析 declaration-only header。** 仍检查候选源内容 hash，仍披露
   全局候选与请求 call-site scope 的区别；没有把该优化说成全仓解析完成。
5. **Scope 过滤保留匹配的未知前沿。** Unresolved row 的任一 typed candidate 匹配 scope 时，
   保留整条未知记录及完整候选集，`to` 仍为空，增加 `relation_scope:` 警告；不猜 target。
   没有候选匹配的记录仍排除。修复绝对 `::` 限定名，避免退回外层 namespace。
6. **错误样本优先级先于路径顺序。** 先保留 content/read/grammar/parse 失败，再保留未建模调用
   与 unsupported-language 样本；总数、分类计数、遗漏数保留，样本仍最多 16 个。

JSON envelope 与旧 edge 字段不变；schema_version 仍为 1。Scope 的 frontier 行数含义已明确更新，
不能再假定带 scope 的所有行都有非空 target。原测试没有删掉：改为精确断言 1 条 resolved +
2 条匹配的 unresolved，并检查完整候选集和警告。

## 红 → 绿与测试完整性

- 最初 5 个语言层控制全部红：casts、C++ template heads、Rust turbofish、computed callees、
  reference-return 符号缺失。
- 最初 6 个 ANGE 缩减集成控制全部红：伪 cast、模板接收者、正常方法正文被拒绝、声明头影响
  正文覆盖、scoped frontier 被丢弃、失败文件被 unsupported 样本淹没。
- 迁移测试在版本仍为 13 时红；版本 14 后绿。绝对限定名补充控制也先红后绿。
- 最终新增 **16 个控制**：7 个语言 unit tests、1 个索引迁移 unit test、8 个 integration tests。
- 全套 Rust 测试 **403 passed / 0 failed / 0 ignored**，其中 binary unit tests 219。
- 18 个定向变异全部由指定断言杀死，未把编译失败或不相关失败算作 kill。

已有 mutation 的目标适配有说明：截断 declaration signature 不再通过“拒绝读唯一正文”来证明，
而由多行重载的精确 candidate set 控制；完整声明仍是 resolver 的必需证据。新增变异覆盖 casts、
turbofish、reference queries、definition-body selection、scoped frontier、错误优先级、版本 13 复用。

已执行且 exit 0：

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --bins
cargo test --locked --tests
cargo test --locked --all-targets --all-features
cargo test --locked --test {fixture_corpus,path_identity,freshness,qualified_identity,map,relations,integration,ange_regressions}
cargo package --locked --allow-dirty
```

测试在独立 Cargo target directory 中构建，使用已安装且固定的 grammar，禁止自动网络下载。
此前默认 debug artifact 被 Cargo 错误判为 Fresh，仍是旧 binary；相应失败记录标为旧产物诊断，
没有用它验收修复。独立目录重建后有效；随后对本 crate 的默认 dev/release 构建缓存执行清理重建，
默认 `target/debug/cx` 的 8 个新集成控制也通过。源码和用户工作没有被清理。

另一个初次失败来自空 grammar cache 下并发自动安装时生成不同 generation。固定预装 grammar
后并发控制通过；没有放宽 generation 断言，也没有用网络安装耗时冒充查询耗时。

## 同一批真实任务，不改 gold

语料仍为 ANGE `24ceb20f158fe6a54eb5a1f6c132e14e3e533532` 的独立归档副本。
仍执行冻结 v2 的 A/B/C 流程，每题三次、ABC/BCA/CAB 轮换；成本含分页、源码复核和回退。
指标是证据充分性，不是没有实际运行的 LLM 作答率；字节不是 token。

| 流程 | 证据充分 | 操作/8题 | 请求+stdout /8题 | 总 wall 中位数 |
| --- | ---: | ---: | ---: | ---: |
| A 旧版关系优先 | 8/8 | 19 | 45,805 B | 2.272 s |
| B 旧工具 source-first | 8/8 | 12 | 26,363 B | 1.117 s |
| C 修复后关系优先 | **8/8** | **14** | **66,690 B** | **2.814 s** |

修复前 C 是 7/8、19 操作、80,070 B、3.926 s（历史另一轮，不当作严格随机配对速度比）。
本轮 C 的未经 source 审核的关系输出满足 7/8；剩余 R04 仍需源码证明接收者容器类型，符合
不进行 compiler/type resolution 的边界。

| C 各题 | 操作 | 字节 | wall 中位数 |
| --- | ---: | ---: | ---: |
| R01 简单无调用 helper | 1 | 2,864 | 255 ms |
| R02 直接项目依赖 | 2 | 6,461 | 370 ms |
| R03 外层/lambda 归属 | 1 | 13,247 | 253 ms |
| R04 接收者与 loader | 3 | 13,213 | 471 ms |
| R05 重载主体拒绝合并 | 2 | 4,734 | 363 ms |
| R06 Python 不支持披露 | 1 | 2,777 | 241 ms |
| R07 重复调用位置 | 1 | 16,444 | 259 ms |
| R08 生产直接 caller | 3 | 6,950 | 613 ms |

R07 的字节含请求封装，stdout 仍受 16 KiB 页目标约束。C 不再丢失 R07，R03 精确为 13 个
外层调用位置，R07 精确为 14，R08 的 scoped 查询直接保留两处生产位置，不再需要撤去 scope。
B 仍更便宜：不要把“缺陷修复、较旧 C 改善”写成“击败强基线”。

## 独立诊断与固定旧语料性能

诊断程序的 C 臂全部通过，且有正控制防止通过空转获取假绿：

| 控制 | 旧版 | 修复版 |
| --- | ---: | ---: |
| 合法 C++ cast，char caller 期望 0 | 1 | **0** |
| 合法引用返回 cache，definition 期望 1 | 0 | **1** |
| 普通 probe 定义、真实 isspace 调用 | 均为 1 | 均为 1 |
| 漏刷新后错误旧 owner | 3 | **0，content_changed** |
| 显式 refresh 后恢复新定义 | 成功 | 成功 |

真实文件编辑只在额外的临时副本中进行，修改前两臂都有 3 条有效记录，恢复后删除副本。

旧性能验收语料 `70fe1922…` 也重新测了 release：1 definition、28 references/10 file rows、
26 callers、31 outgoing sites 全部保持。每查询预热一次，再采样 9 次：

| 查询 | 旧版 median/p95 ms | 修复版 median/p95 ms | 修复版峰值 RSS MiB |
| --- | ---: | ---: | ---: |
| Root overview | 105.0 / 110.9 | 98.8 / 102.1 | 49.1 |
| File overview | 98.3 / 101.4 | 95.9 / 99.3 | 49.0 |
| Definition | 97.0 / 102.6 | 98.0 / 101.0 | 49.3 |
| Symbols | 106.2 / 108.4 | 106.5 / 108.6 | 49.5 |
| References | 207.6 / 216.1 | 207.9 / 216.5 | 58.2 |
| Callers | 211.1 / 216.8 | 217.5 / 221.1 | 74.0 |
| Callees | 108.3 / 113.8 | 204.2 / 207.9 | 72.6 |
| Map | 106.5 / 115.8 | 109.3 / 113.1 | 52.5 |

修复版 cold 2.093 s / 138.0 MiB，DB 16,846,848 B；metadata/path/verified 增量分别
348.8 / 217.6 / 523.1 ms。标准旧语料的绝对预算仍满足；callees 相比旧版仍明显更重。
新增引用返回符号事实使 root overview 的计数变大，stdout 从 1,813 变为 1,814 B，不是格式回退。

## 证据、兼容性和边界

```text
/tmp/cx-ange-fix.Cfkeuu/
  initial.diff / initial.status
  red-*.stdout / red-*.stderr / red-*.exit
  candidate-manifest.json
  gates.json / gate-*.stdout / gate-*.stderr
  mutations/report.json
  abc/runs.json / score.json
  diagnostics/findings.json
  performance/report.json
  candidate
```

验证 binary SHA-256：
`c8211a39c8af2e5a94b2198b0666f1c32cd9d3d7b37a7ff69f684621eba5ce2f`。
默认 release 路径也已重新构建；不同构建目录产生不同 binary hash，不冒充同一测量产物。
旧 corpus/tasks/gold 的摘要与原冻结文件一致；新候选身份另记 manifest，没有篡改旧 lock。

- 仍无多跳分析、运行时覆盖、类型解析、自动测试推荐或新 Pi 工具。
- Computed callee 明确部分完成；它及宏、动态分派等不因此成为已解析依赖。
- Declaration equivalence 仍保守；没有为让普通 body 可读而猜测类型别名绑定。
- `INDEX_VERSION=14` 强制重建符号事实，JSON schema/package version 未发布改版。
- 未改变 ANGE 主工作树；没有自动提交、发布或启动后续阶段。
- 本轮曾在本地环境诊断误显示 registry 凭据，已明确提醒轮换；没有复制到代码/报告或再次输出。
