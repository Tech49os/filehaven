# BUG-007 整理台删除文件功能缺失

| 项 | 值 |
|---|---|
| 来源 | `bugfix/20261122_bug_feedback_001.docx` 第 7 条 |
| 确认结论 | **属实**：前端无删除入口/键位，后端无删除命令、无回收站 API；`drop_file` 只改 DB 状态（待定区移出） |
| 优先级 | P0（用户明确的整理刚需，但安全要求最高） |
| 主要代码 | `app-v2/src/core/ipc.ts:123-180, keys.ts:8-77`；`app-v2/src-tauri/src/main.rs:2421-2468`；`app-v2/src-tauri/src/index.rs:52-108, 2049-2052`；`app-v2/src-tauri/src/migrate.rs:147-305`；`app-v2/src-tauri/src/plan.rs:263-282`；`app-v2/src-tauri/src/schema.sql:96-118` |
| 测试落点 | `app-v2/src-tauri/src/tests.rs`（三段式 2165-2206、撤销 2409-2466）；`app-v2/src/pages/workspace/Workspace.test.ts`、`core/keys.test.ts` |

## 1. 问题陈述

用户原话：「整理台删除功能欠缺，有些文件在整理过程中明确的需求就是要删除，当前没有这个功能。」

现状核对：
- 整理台只有确认/备选/新建目录/暂缓/先放着/撤销/重新推荐/换一批/结束本批，无删除。
- `dropFile → drop_file`（`index.rs:2049-2052`）只是 `parked → skipped` 状态流转，磁盘文件原封不动，且入口在待定区。
- 后端命令表 46 个命令无删除；无 `SHFileOperation` / 回收站调用；状态机 8 态无 `trashed`。

## 2. 目标 / 非目标

**目标**
- 用户在整理台（待确认卡片、已确认条目）可把明确不要的文件「删除」——实现为**移入隔离区、可撤销、可审计**。
- 提供二次确认，明确告知「移入隔离区，可撤销」，提供 Delete 键位与 Ctrl+Z 撤销链路。

**非目标**
- 绝不永久删除、绝不调用 Windows 回收站（`SHFileOperation`）。
- 不做「清空隔离区」功能（另案）。
- 不改变待定区 `drop_file` 的语义。

## 3. 行为规格

1. **Given** 用户在整理台选中一份文件点击「删除（移入隔离区）」或按 Delete，**When** 触发，**Then** 先弹二次确认，文案写明目标路径与可撤销性；取消则不执行任何写操作。
2. **Given** 用户确认删除，**When** 执行，**Then**：
   - 记录原路径与 SHA-256；
   - 复制/移动到 `<归档根>/.filebutler/quarantine/<批次id>/<源区相对路径>`（沿用 `plan::quarantine_path`）；
   - 校验哈希一致后，源文件才离开原位置；目标同名按 `_2` 错开，绝不覆盖；
   - 写 `migrations`（新增 `kind='trash'`）与 `migration_items`（src/dst/sha256），文件状态转为 `trashed`（记录删除前状态）。
3. **Given** 删除成功，**When** 前端刷新，**Then** 该文件从当前批次消失，可在隔离区/批次记录中查到；提示提供「撤销」入口。
4. **Given** 用户按 Ctrl+Z 或点撤销，**When** 批次撤销，**Then** 从隔离区取回原件 → SHA-256 校验 → 原位置无同名则还原 → 状态回到删除前；同名冲突或哈希不一致时拒绝撤销并给出原因，不得覆盖。
5. **Given** 状态机变更前状态不是合法来源态，**When** 调用删除命令，**Then** 返回明确错误，不写磁盘。

## 4. 变更点

- 后端：`Status` 枚举新增 `Trashed`，`index.rs` 合法转移表增加 `pending_confirm/confirmed/parked → trashed`，保留 `prev_status` 供撤销还原；新增 `trash_file` / `undo_trash` Tauri 命令并注册。
- 复用 `migrate.rs` 的 `sha256_of` / `copy_file` / `move_file` / `undo_one` 与 `plan::unique_name` / `quarantine_path`；不新造平铺隔离目录。
- 前端：`Workspace.vue` 加按钮与确认弹窗；`FileCard.vue` 已确认条目加同样入口；`core/keys.ts` 加 Delete 绑定；`core/ipc.ts` 封装新命令；撤销接入 `stores/batch.ts` 的 Ctrl+Z 栈。
- 文案：全链路使用「移入隔离区」，禁止「永久删除」字样。

## 5. 验收标准（先写红测）

- [ ] Rust 红测：删除后磁盘原路径不存在，隔离区副本存在且 SHA-256 与记录一致；`migration_items` 有 src/dst/sha256。
- [ ] Rust 红测：撤销后原路径字节级还原、状态回到删除前；隔离区副本被清理。
- [ ] Rust 红测：目标同名不覆盖（错开 `_2`）；隔离区原件被篡改时撤销被拒且原文件不动。
- [ ] Rust 红测：非法状态转移报错、不产生批次。
- [ ] 前端单测：Delete 键触发确认弹窗；取消无 IPC 调用；确认后调用 `trash_file`；Ctrl+Z 调用 `undo_trash`。
- [ ] e2e：整理台删除一份 → 文件进隔离区 → 撤销 → 文件回到源目录且内容一致。

## 6. 风险与边界

- **铁律**：永不直删。新增代码里不允许出现对源文件直接 `fs::remove_file` 的路径（`move_file` 中跨盘搬迁成功后清源是既有例外）。
- 删除只针对源区文件；已归档文件若要删除需另案（会影响归档树与目录计数）。
- 隔离区目录已由 `main.rs:329-342` 创建，监听忽略隔离区（`watch.rs:217-221`），不要重复造轮子。
- 批次数据写入必须与文件操作同事务语义：失败时不得留下「库里有记录、文件没动」的幽灵批次。
