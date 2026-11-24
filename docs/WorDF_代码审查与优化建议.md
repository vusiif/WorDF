# WorDF 项目代码审查与优化建议

> 项目地址：<https://github.com/vusiif/WorDF>  
> 审查对象：仓库 `windows-sync` 分支（审查时版本：`1.0.0`）  
> 审查范围：Rust 核心转换流程、DOC/DOCX 格式校验、字体处理、文件安全、跨平台兼容性、CLI、性能与测试。  
> **说明：** 本报告主要基于源码静态检查与已有项目资料，未在当前环境执行完整构建、测试或性能基准。下文区分代码中可直接观察到的逻辑问题与仍需运行验证的潜在风险。

## 一、总体评价

WorDF 已经具备较好的基础能力，适合定位为**轻量级、离线优先、无需 LibreOffice 的 Word 转 PDF 工具**。目前已有值得保留的设计：

- 纯内存形式的转换 API；
- DOC 与 DOCX 两种输入路线；
- DOCX ZIP 防炸弹检查；
- 内置中文字体和字体回退处理；
- 类型化错误与诊断信息；
- 原子输出提交与并行批量转换。

若希望进一步用于生产环境，建议优先完善**跨平台兼容性、转换正确性、资源隔离、安全边界、批量任务一致性和回归测试**，再逐步增加功能。

## 二、优先修复的问题

### 1. [P0] macOS 原子文件提交存在编译兼容性问题

**位置：** [`src/transaction/atomic.rs` 第 91–123 行](https://github.com/vusiif/WorDF/blob/windows-sync/src/transaction/atomic.rs#L91-L123)

`atomic_commit_rename()` 对全部 Unix 系统使用了 Linux 特有的 `renameat2` 接口：

```rust
#[cfg(unix)]
{
    libc::renameat2(
        libc::AT_FDCWD,
        old_c.as_ptr(),
        libc::AT_FDCWD,
        new_c.as_ptr(),
        libc::RENAME_NOREPLACE,
    )
}
```

`#[cfg(unix)]` 同样覆盖 macOS 等平台，而 macOS 不提供此 Linux 接口，因此 macOS 构建可能在此处失败。

**建议：**

- Linux 继续使用 `renameat2(RENAME_NOREPLACE)`；
- macOS 使用对应的 `renamex_np(RENAME_EXCL)` 等原生接口；
- 其他 Unix 平台实现独立分支，或明确声明不支持；
- CI 增加 Linux、Windows、macOS 真实构建与测试。

**注意：** 不建议直接改成 `exists()` 后调用 `rename()`，因为会引入 TOCTOU（检查与操作之间）的竞态条件。

### 2. [P0] 批量转换存在同名文件输出冲突

**位置：** [`src/cli/batch.rs` 第 134–164 行](https://github.com/vusiif/WorDF/blob/windows-sync/src/cli/batch.rs#L134-L164)

```rust
let out_pdf = config.output_dir.join(format!("{}.pdf", stem));
```

例如：

```text
input/
├── report.doc
└── report.docx
```

两个输入均映射到 `report.pdf`。如果批量任务并行执行：

- 不使用 `--overwrite` 时，任务可能出现跳过或提交冲突；
- 使用 `--overwrite` 时，最终 PDF 内容可能由线程完成顺序决定；
- 在大小写不敏感的文件系统上，大小写不同的文件名也可能冲突。

**建议：** 在任务调度之前集中计算所有目标路径并检测冲突。默认拒绝冲突，并考虑新增：

```bash
--collision-policy error|rename|prefer-docx
```

**验收标准：** 并行调度顺序不会影响最终文件内容，同一批输入的结果可重复、可预测。

### 3. [P1] 批量模式未完全复用单文件模式的安全校验

**位置：** [`src/cli/batch.rs` 第 150–218 行](https://github.com/vusiif/WorDF/blob/windows-sync/src/cli/batch.rs#L150-L218)

单文件模式使用 `validate_and_isolate_targets()` 校验输入与输出文件目标；批量模式则直接执行读取、临时文件创建与两阶段提交。两种入口的安全约束可能不一致。

**建议：** 抽取统一的单文件转换任务执行器，将以下步骤封装并复用：

1. 路径和文件实体验证；
2. 文件大小与资源预算检查；
3. 输入读取与转换；
4. 临时输出和原子提交；
5. 错误及诊断报告。

批量模式仅负责任务发现、冲突检查、调度及汇总。

### 4. [P1] DOCX 字体适配可能错误跳过中文字体回退

**位置：**

- [`src/docx_adapt/xml.rs` 第 20–46 行](https://github.com/vusiif/WorDF/blob/windows-sync/src/docx_adapt/xml.rs#L20-L46)
- [`src/docx_adapt/styles.rs` 第 15–20 行](https://github.com/vusiif/WorDF/blob/windows-sync/src/docx_adapt/styles.rs#L15-L20)

当前逻辑可能依据 `w:eastAsia`、`w:ascii`、`w:hAnsi` 中某一字体是否存在，就跳过对当前文本运行的回退字体注入。但**字体存在不等于包含所需字形**。

例如：

```xml
<w:rFonts w:ascii="Arial" w:hAnsi="Arial"/>
<w:t>中文测试</w:t>
```

即使 Arial 可用，也不能据此判断其能覆盖这段中文。样式级适配还可能因为整个样式 XML 包含某个已安装字体名称而过早返回。

**建议：**

- 根据实际字符和字体 CMap 字形覆盖情况做判断；
- 分离西文、东亚及其他脚本的字体策略；
- 采用按文字运行或样式节点的精细适配，而非全局字符串存在性判断；
- 为中英混排、不同样式继承和多字体回退补充测试。

### 5. [P1] 纯内存 API 的文件系统副作用承诺需核实与收紧

**位置：**

- [`patches/docxide-pdf/src/fonts/discovery.rs`](https://github.com/vusiif/WorDF/blob/windows-sync/patches/docxide-pdf/src/fonts/discovery.rs)
- [`patches/docxide-pdf/src/fonts/cache.rs`](https://github.com/vusiif/WorDF/blob/windows-sync/patches/docxide-pdf/src/fonts/cache.rs)

README 将 `wordf::convert()` 描述为不访问文件系统的纯内存接口，但字体发现与缓存模块包含本地字体目录扫描、磁盘缓存读取及潜在写入流程。这不意味着每次调用都会发生写盘，却说明“无文件系统访问”承诺需要进一步验证。

**建议：** 新增显式 `memory-only` 模式：只允许使用调用方传入的字体字节与内置字体，禁止系统字体扫描和字体缓存 I/O。CLI 可继续启用系统字体发现。

这将有利于 WebAssembly、Serverless、沙箱执行和敏感数据处理场景。

### 6. [P1] 输入文件大小限制执行得太晚

**位置：** [`src/main.rs` 第 94–124 行](https://github.com/vusiif/WorDF/blob/windows-sync/src/main.rs#L94-L124)

CLI 在调用含有输入大小限制的核心 `convert()` 前，就通过 `fs::read()` 将文件全部读入内存：

```rust
let input_bytes = fs::read(&validated.input_canonical)?;
```

因此即使输入超过 API 设定的 128 MiB 限制，CLI 也可能先完整分配和读取该文件。并行批量模式会进一步放大峰值内存占用。

**建议：**

- 读取前先检查文件元数据中的大小；
- 使用具备读取上限的输入方式，避免文件在检查后增长；
- 为批量模式引入总内存预算，例如 `--max-memory 1024M`；
- 限制同时进行的重型渲染任务数量，而非只按 CPU 核心数控制线程。

### 7. [P1] DOCX 字体预处理可能产生无意义的 ZIP 重压缩

**位置：** [`src/docx_adapt/mod.rs` 第 58–113 行](https://github.com/vusiif/WorDF/blob/windows-sync/src/docx_adapt/mod.rs#L58-L113)

当前流程可能在适配后的 XML 内容没有变化时，仍然重新写入 XML 和压缩 ZIP 部件。对无需 CJK 字体替换的普通 DOCX，这会增加不必要的 CPU 消耗。

**优化示意：**

```rust
let (adapted_xml, cjk_count) =
    adapt_xml_text(&xml_content, fb, available_fonts);

total_cjk_in_package += cjk_count;

if adapted_xml == xml_content {
    writer.raw_copy_file(file)?;
    continue;
}

writer.start_file(&name, options)?;
writer.write_all(adapted_xml.as_bytes())?;
modified_entries += 1;
```

以上为逻辑示意，实际实现应结合原有分支和 ZIP Writer API 调整。

**建议：**

- 仅真正修改 XML 时才重压缩；
- 对确认无需适配的 DOCX，直接使用原始输入；
- 内置字体延迟解压和注册；
- 通过基准测试比较优化前后的耗时、吞吐量与峰值内存。

### 8. [P1] 字符覆盖检查范围不够完整

**位置：** [`src/font/mod.rs` 第 72–87 行](https://github.com/vusiif/WorDF/blob/windows-sync/src/font/mod.rs#L72-L87)

`is_cjk_char()` 主要覆盖汉字、部分汉字扩展与全角字符，未充分覆盖日文假名、韩文音节及全部现代汉字扩展范围。另外，预检查的 XML 部件范围也未必覆盖所有绘图或特殊文本结构。

**建议：**

- 使用 Unicode Script 分类，而非仅手动列举字符区间；
- 基于字体实际字形覆盖能力执行回退；
- 为每个缺失字符记录所在部件及字体信息；
- 明确文档中“字体缺失预检测”能够覆盖的范围。

### 9. [P2] CLI 文档与实际行为存在偏差

**位置：**

- [`src/main.rs`](https://github.com/vusiif/WorDF/blob/windows-sync/src/main.rs)
- [`src/cli/batch.rs`](https://github.com/vusiif/WorDF/blob/windows-sync/src/cli/batch.rs)

主要包括：

- `--report-json` 在目录批量输入场景中可能被静默忽略；
- 批量转换可能返回退出码 `1`，但 README 的稳定退出码表未说明该退出码。

**建议：** 为批量转换实现统一的 JSON 汇总报告，逐文件记录输入、输出、状态、耗时、页数、诊断与错误类型，并正式定义各退出码语义。

### 10. 其他需要补充测试的风险

| 问题 | 位置 | 建议 |
|---|---|---|
| 加密 DOCX 可能被归类为不支持格式 | `format/doc.rs` | 识别 `EncryptedPackage` / `EncryptionInfo`，返回明确的 `Encrypted` 错误 |
| 指定不存在的字体路径时可能静默回退 | `cli/discovery.rs` | 显式指定路径必须验证存在性、可读性和格式 |
| XML 处理依赖固定 `w:` 前缀 | `docx_adapt/xml.rs` | 引入命名空间感知的 XML 解析 |
| CDATA、实体及特殊 XML 标签处理不完整 | `docx_adapt/xml.rs` | 添加边界输入和回归测试 |
| 图片尺寸解析失败可能绕过某些检查 | `format/docx.rs` | 对不明尺寸或不明格式图像采用明确的解码资源策略 |
| 原子提交前仅 `flush()` | `main.rs`、`cli/batch.rs` | 增加可选的 `sync_all()` / 目录同步持久化模式 |
| OOXML 部件校验仍可完善 | `format/docx.rs` | 校验重复条目、内容类型、关系文件与目标路径 |

以上部分属于**潜在风险**，仍需构造相应输入、运行程序并确认实际影响。不要将其直接视为已复现的安全漏洞。

## 三、值得新增的功能

### 3.1 功能优先级总表

| 优先级 | 功能 | 价值 | 相对复杂度 |
|---|---|---|---|
| P1 | `--analyze` 转换质量诊断 | 预先识别字体、结构和布局兼容性风险 | 中 |
| P1 | 批量 JSON 汇总报告 | 方便 CI、脚本和服务端集成 | 低 |
| P1 | 同名输出冲突策略 | 保证批量结果确定性 | 低 |
| P1 | `--recursive` 递归批量转换 | 支持整个目录树 | 低 |
| P1 | 内存预算与超时控制 | 防止服务端资源失控 | 中高 |
| P2 | 多级字体回退链 | 改善中日韩混排和生僻字处理 | 中高 |
| P2 | `--resume` 增量转换 | 跳过未变更的输入文件 | 中 |
| P2 | PDF 页面范围导出 | 仅导出指定页面 | 中 |
| P2 | PDF 元数据设置 | 自定义标题、作者等 | 中 |
| P2 | stdin/stdout 管道接口 | 便于与其他 CLI 组合 | 中 |
| P3 | PDF/A 归档格式 | 支持长期文档存档 | 高 |
| P3 | WebAssembly SDK | 浏览器内离线转换 | 高 |
| P3 | 本地拖拽 GUI | 面向非技术用户 | 中 |

*复杂度属于当前架构下的相对判断，需结合实际渲染后端能力验证。*

### 3.2 最推荐的新功能

#### 功能 A：`--analyze` 文档诊断模式

无需生成最终 PDF，输出文档结构与可能的兼容性问题：

```bash
wordf document.docx --analyze --report-json analysis.json
```

可包含：文件格式、文档主要结构、所需字体、缺失字形、潜在不支持的元素、预计资源开销等。风险判断应基于转换后端的实际能力，而非只根据某个 OOXML 标签是否存在。

#### 功能 B：递归与增量批量转换

```bash
wordf ./input --recursive --resume --output-dir ./output
```

增量模式可以先比较文件大小与修改时间，再以内容哈希进行可靠确认；支持 `--fail-fast` 与失败后继续处理两种行为。

#### 功能 C：`--strict` 严格模式

将诊断结果划分为信息、警告和严重错误：

- 普通模式：尽力完成转换并输出诊断；
- 严格模式：在可能影响关键内容完整性时返回失败。

这对自动化文档处理流水线尤其重要。

## 四、架构与性能优化建议

### 4.1 建议的处理架构

```text
             DOC / DOCX 输入
                    │
                    ▼
        格式识别 + 资源预算 + 安全检查
                    │
                    ▼
        文档分析 + 字体解析 + 兼容性诊断
                    │
            ┌───────┴────────┐
            ▼                ▼
     DOC → rwml → DOCX    DOCX 字体适配
            │                │
            └───────┬────────┘
                    ▼
             统一 PDF 渲染后端
                    │
                    ▼
        PDF 校验 + 原子提交 + 结构化报告
```

**原则：**

1. 单文件与批量入口共享验证、转换与提交逻辑；
2. 分析模式尽可能复用转换前的文档解析过程；
3. 字体发现策略可配置，支持无磁盘 I/O 模式；
4. 将安全预算（大小、内存、并发、必要时的超时）统一管理；
5. 允许未来新增输出接口，而不复制基础处理逻辑。

### 4.2 建议的性能优化顺序

**第一阶段：低风险、容易度量的优化**

- 避免无内容变化时重写 OOXML ZIP 部件；
- 跳过无需字体适配的文档；
- 延迟初始化和解压内置字体。

**第二阶段：内存与数据流优化**

- 分析重复 ZIP 解析及不必要的字节复制；
- 度量文档模型、字体缓存和 PDF 缓冲区的峰值内存；
- 引入批量内存预算和并发限制。

**第三阶段：基准与依赖治理**

- 构建固定样本性能基准（小文档、大表格、图片密集、中文混排）；
- 比较不同并行度下的耗时、吞吐量、内存；
- 检查主项目和补丁中不同 `zip` 版本的重复依赖成本，兼容性验证后再考虑统一。

## 五、测试体系建议

| 测试类别 | 测试范围 |
|---|---|
| 平台兼容性 | Linux、Windows、macOS |
| 文档兼容性 | 不同 Word 版本、DOC/DOCX、不同语言 |
| 排版正确性 | 表格、分页、页眉页脚、图片、脚注、复杂样式 |
| 字体 | 简繁中文、日语、韩语、生僻字、Emoji、混排 |
| 输入安全 | ZIP Bomb、损坏的 CFB、畸形 XML、异常图片 |
| 并发 | 同名输入、覆盖策略、多线程重复执行 |
| 文件系统 | 符号链接、硬链接、竞态、磁盘写入失败 |
| 输出质量 | PDF 页数、文本完整性、视觉差异 |
| 性能 | 转换耗时、吞吐量、峰值内存、文件体积 |

**测试建议：**

- 在 CI 中加入 `cargo fmt --check`、`cargo clippy`、`cargo test`、`cargo audit`；
- 针对 ZIP、CFB、XML 与字体解析添加 Fuzz 测试；
- 建立真实 DOC/DOCX 样本集及参考 PDF；
- 同时进行文本提取核验与渲染后的图像差异对比；
- 为视觉差异设定合理容差，避免跨平台字体栅格化差异造成大量误报。

**重点：** “生成了 PDF”并不等于“转换正确”。应重点验证内容不丢失、中文不乱码、表格不溢出、分页和字体回退合理。

## 六、建议实施路线

### 第一阶段：稳定性修复（优先）

- [ ] 修复 macOS 原子提交的条件编译；
- [ ] 检测并处理批量同名文件冲突；
- [ ] 统一单文件与批量模式的安全校验；
- [ ] 修正字体路径错误处理；
- [ ] 规范批量退出码与 JSON 报告；
- [ ] 为上述问题补充回归测试。

### 第二阶段：性能与质量

- [ ] 加入 DOCX XML 无变化快速路径；
- [ ] 在读取输入前应用大小和内存预算；
- [ ] 改进字体字形覆盖检查与回退策略；
- [ ] 引入命名空间感知的 XML 解析；
- [ ] 建立真实性能及视觉质量基准。

### 第三阶段：产品能力扩展

- [ ] `--analyze` 文档诊断；
- [ ] `--strict` 严格转换模式；
- [ ] `--recursive` 递归批量转换；
- [ ] `--resume` 增量转换；
- [ ] 完整结构化 JSON 报告；
- [ ] 更明确的纯内存 SDK 配置。

### 第四阶段：生态与分发

- [ ] 稳定的跨平台 CI / Release 流程；
- [ ] WASM / 浏览器 SDK 可行性验证；
- [ ] 本地 GUI；
- [ ] 可选的 PDF/A 支持；
- [ ] 正式版本兼容性和功能支持矩阵。

## 七、最终建议

如果资源有限，优先做以下五件事：

1. **修复跨平台构建及原子文件提交兼容性。**
2. **修复批量文件名冲突与安全校验不一致。**
3. **完善中文字体覆盖判断和 XML 适配逻辑。**
4. **加强资源限制，并避免不必要的 DOCX 重压缩。**
5. **建立跨平台 CI、复杂文档回归测试和转换质量基准。**

现阶段**不建议立即重写 PDF 渲染器**。WorDF 已有较清晰的双路线转换架构，应先确保异常输入下的稳定性、转换结果的可靠性，以及 README/API 对外承诺与实际实现一致。基础完成后，再扩展诊断、批量处理、WASM 和 GUI 等能力。

---

## 相关源码链接

- [WorDF 仓库](https://github.com/vusiif/WorDF)
- [原子提交实现](https://github.com/vusiif/WorDF/blob/windows-sync/src/transaction/atomic.rs)
- [批量处理实现](https://github.com/vusiif/WorDF/blob/windows-sync/src/cli/batch.rs)
- [CLI 主入口](https://github.com/vusiif/WorDF/blob/windows-sync/src/main.rs)
- [DOCX XML 字体适配](https://github.com/vusiif/WorDF/blob/windows-sync/src/docx_adapt/xml.rs)
- [DOCX 样式适配](https://github.com/vusiif/WorDF/blob/windows-sync/src/docx_adapt/styles.rs)
- [DOCX 适配入口](https://github.com/vusiif/WorDF/blob/windows-sync/src/docx_adapt/mod.rs)
- [字体处理](https://github.com/vusiif/WorDF/blob/windows-sync/src/font/mod.rs)
- [字体发现补丁](https://github.com/vusiif/WorDF/blob/windows-sync/patches/docxide-pdf/src/fonts/discovery.rs)
- [字体缓存补丁](https://github.com/vusiif/WorDF/blob/windows-sync/patches/docxide-pdf/src/fonts/cache.rs)
