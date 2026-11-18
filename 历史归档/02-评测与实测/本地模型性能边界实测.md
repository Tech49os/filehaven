# 本地小模型性能边界实测：8GB 显存跑 9B，长 prompt 归并到底行不行

> 一台笔记本（RTX 4060 Laptop 8GB + Intel Ultra 9 185H）上的真实测量记录。
> 目的：把「看起来像硬件不行」的现象拆开，给出可复用的数字和配置，避免后来人重复踩坑。
> 测量日期 2026-11-18，Ollama 0.40.16。

---

## 一、结论摘要

| 问题 | 结论 |
|---|---|
| 8GB 显存能跑 9B Q4 吗 | **能**。5.22 GB 权重全部驻留显存，33/33 层上 GPU，运行中仍余 1.6 GB |
| 5000 字 prompt 会因显存不足失败吗 | **不会**。prefill 3179 token 只用 4.4 秒（730 tok/s），是全流程最快的环节 |
| 那到底为什么会「199 秒后连接断开」 | 输出侧：4.97 tok/s × 900 token ≈ 185 秒，撞上客户端超时；同时 `num_ctx=4096` 只剩 ~900 token 输出余量，写满被截断 |
| 40 个候选做语义归并可行吗 | **可行，约 150 秒/次**。是一次性配置能接受、交互式不能接受的量级 |
| 换 4B 会更快吗 | **不会**。速度快 65%，但输出 token 多 45%，墙钟时间几乎相同 |
| 加 NPU 能加速吗 | **不能**。见附录 B：没有运行时支持跨 NPU+GPU 拆分同一模型，且 NPU 带宽只有 GPU 的 1/3 |

**一句话：瓶颈是生成吞吐（约 5 tok/s），不是显存；这是产品节奏问题，不是硬件墙。**

---

## 二、测试机器

| 部件 | 规格 | 与 LLM 推理相关的意义 |
|---|---|---|
| CPU | Intel Core Ultra 9 185H（Meteor Lake，16 核 22 线程） | 附带 Intel AI Boost NPU，约 11 TOPS INT8 |
| 内存 | 32 GB DDR5-5600，双通道 2×16 GB | 理论带宽 89.6 GB/s，CPU/NPU/iGPU 共享 |
| GPU | RTX 4060 Laptop，8 GB GDDR6，128-bit 17 Gbps | 显存带宽 272 GB/s，CUDA 可用 |
| 系统 | Windows 11 | Ollama 走 CUDA 后端，需注意 mmap 在 Windows+CUDA 下被禁用 |

模型：`qwen3.5:9b`（Q4_K_M，文件 5.23 GiB，8.95B 参数）、`qwen3.5:4b`。

---

## 三、被测任务（真实，不是玩具）

用真实归档场景里抽出的 40 个高频词组，让模型把「同一件事的不同叫法」归到一组并起组名：

- prompt 由项目里的纯函数 `buildMergePrompt()` 生成
- 输入 `candidates-real.json` 前 40 条，每条含词、出现次数、3 个真实文件名样例
- 成品 prompt：**5039 字 ≈ 3179 token**（中文约 1.55 字/token）
- 输出要求：`{"items":[{"term":"词组原文","topic":"组名或 null"}]}`，40 条全覆盖

这就是俗称的「5000 字 prompt」。

---

## 四、实测结果

| # | 模型 | num_ctx | num_predict | 总耗时 | prefill | 生成 | 结果 |
|---|---|---|---|---|---|---|---|
| A | 9b | 4096 | 3000 | **152.9s** | 3179tok / 4.4s | 716tok / 4.83 tok/s | 正常返回，16 组合法 JSON |
| B | 9b | 16384 | 2000 | **148.7s** | 3179tok / 5.2s | 733tok / 5.60 tok/s | 正常返回，17 组合法 JSON |
| C | 4b | 8192 | 2000 | **138.5s** | 3179tok / 2.5s | 1062tok / 8.29 tok/s | JSON 缺右括号，解析失败 |

复现命令（本仓库内）：

```bash
cd app
npx tsx test/stress-merge.ts --n 40 --num_ctx 4096  --num_predict 3000
npx tsx test/stress-merge.ts --n 40 --num_ctx 16384 --num_predict 2000
npx tsx test/stress-merge.ts --n 40 --num_ctx 8192  --num_predict 2000 --model qwen3.5:4b
```

> 注：C 列 4B 的耗时差不多，是因为它把 JSON 输出成了多行缩进格式，1062 token 对 733 token。
> **别用「模型更小所以更快」来估时间，要估「总输出 token ÷ 吞吐」。**

---

## 五、原始证据：一次「失败」到底发生了什么

Ollama 服务端日志（`%LOCALAPPDATA%\Ollama\server.log`）里那次被记成"失败"的请求：

```
llama_model_loader: - type q4_K: 217 tensors
load_tensors: offloaded 33/33 layers to GPU
load_tensors:        CUDA0 model buffer size =  4812.25 MiB
load_tensors:    CUDA_Host model buffer size =   545.63 MiB
llama_context: n_ctx                 = 4096
llama_kv_cache:      CUDA0 KV buffer size =   128.00 MiB   ← 4096 上下文的 KV 只要 128MB
slot operator(): new prompt, n_ctx_slot = 4096, n_keep = 4, task.n_tokens = 3177

slot print_timing: prompt eval time =    4348.38 ms /  3177 tokens (730.62 tokens per second)
slot print_timing:        eval time =  184625.02 ms /   919 tokens (  4.97 tokens per second)
slot print_timing:       total time =  188973.40 ms /  4096 tokens
slot      release: stop processing: n_tokens = 4095, truncated = 1     ← 被上下文上限截断
[GIN] 200 | 3m10s | POST "/api/chat"                                   ← 服务端其实返回了 200
```

同时 `GET /api/ps` 显示：

```
qwen3.5:9b | size 5.22 GB | size_vram 5.22 GB     ← 全部在显存，没有 spill 到内存
```

读法：

1. `offloaded 33/33 layers` + `size == size_vram` ⇒ **没有显存不足，没有 CPU 卸载**；
2. `prompt eval 4348ms / 3177 tokens` ⇒ **prefill 只花 4.4 秒**，730 tok/s；
3. `eval 184625ms / 919 tokens` ⇒ 全部时间耗在**逐 token 生成**上；
4. `n_tokens = 4095, truncated = 1` ⇒ 4096 的上下文被写满，答案是被**截断**的；
5. `200 | 3m10s` ⇒ 服务端没崩，是**客户端先等不住**。

---

## 六、三个最容易被误判的归因

| 错误结论 | 为什么错 | 正确说法 |
|---|---|---|
| 「8GB 显存 + 9B 撑不住」 | 权重 5.22GB 全在显存，还剩 1.6GB；Qwen3.5 是混合注意力，4096 上下文 KV 仅 128MB | 8GB 跑 9B Q4 很宽裕，16K 上下文也放得下 |
| 「prefill 阶段就崩了」 | prefill 是 730 tok/s，4.4 秒读完 5000 字 | 崩在 decode，每 token 200ms |
| 「连接断开 = 模型失败」 | 服务端日志是 200 + 3m10s | 是客户端超时；而客户端默认超时可能只有 60 秒 |

**排查口诀：先分 prefill / decode 两段看耗时，再看 `truncated` 标志，最后才怀疑硬件。**

---

## 七、可直接抄的配置

归并/长 prompt 这类「一次性、要长输出」的调用：

```jsonc
{
  "model": "qwen3.5:9b",
  "stream": true,          // 必须。否则前端 2 分钟白屏，用户以为卡死
  "think": false,          // qwen3 系列必须关，否则 26s → 3s 的差距
  "options": {
    "num_ctx": 16384,      // 别用默认 4096：3179 的 prompt 只剩 900 输出余量
    "num_predict": 2000,   // 要显式给上限，否则写飞
    "temperature": 0.2
  }
}
```

客户端超时必须同步放大：

- 本仓库 `llm.ts`：`cfg.timeoutMs ?? 60000`
- Rust 侧 `main.rs`：`timeout_ms.unwrap_or(60000)`

**结论：默认 60 秒，这个任务（约 150 秒）根本不该用默认值，要传 `timeoutMs: 300000`。**
忽略这一条，你会得到「莫名其妙断连」的假故障。

---

## 八、产品决策含义

| 场景 | 单次约 | 可行性 |
|---|---|---|
| 40 候选 × 1 次（配置向导里跑一次） | 150 s | 可行，但要进度条 + 流式反馈 |
| 179 候选分 5 块 | 约 12 min | **不可行**，用户会以为死机 |
| 每份文件一次模型判断（96 token 输出） | 5–6 s | 可行（现有能力） |

所以正确的表述不是「本地模型做语义归并不可行」，而是：

> **一次性归并可行（150 秒）；全量分块归并不可行（10 分钟级）。前者靠产品设计吸收，后者要靠算法降维。**

想提速只有三条路，按性价比排序：

1. **压输出 token**：紧凑 JSON（去掉缩进/换行）能省 30–45% 输出，直接省时间；这是唯一无损的杠杆。
2. **减输入候选**：40 条 → 25 条，成本线性下降。
3. **换小模型**：无效，见第四节 C 列。

---

## 九、踩坑清单

1. **别用「模型更小」估速度**：4B 吞吐高 65%，但输出啰嗦 45%，墙钟时间打平。
2. **`num_ctx` 默认 4096 会静默截断**：Ollama 不会报错，只会 `truncated=1` 返回半截 JSON。看起来像"模型不会干活"。
3. **客户端默认超时 60 秒**：长任务必须显式放大，否则表现为"连接断开/模型不可用"。
4. **`parseMergeResult` 的空格陷阱**：模型输出 `AI 大赛`，候选池是 `AI大赛`，`pool.has()` 判为「模型编造」。实测每轮 40 条里 8 条"幻觉"大多是这类空格/全半角差异。
   → 解析前必须做归一化（去空格、统一大小写、全半角折叠），否则会**低估模型、高估算法**。
5. **Windows + CUDA 下 Ollama 禁用 mmap**（日志：`disabling mmap for llama-server load by default reason=windows_cuda`），首次加载比 Linux 慢。
6. **不要并发**：Ollama 默认单 slot 串行推理，并发 1/2/4 的总耗时相同。
7. **不要 `ollama stop <model>`**：会把 serve 进程一起带崩；要卸载驻留模型用
   `POST /api/generate {"model":...,"prompt":"","keep_alive":0}`。
8. **8GB 装不下两个大模型**：7B + 4B 同时驻留会让 llama-server 崩溃，切换模型前先卸载。

---

## 附录 A：如何判断「是显存问题还是速度问题」

三步，两分钟：

```bash
# 1) 模型是否全在显存（size 与 size_vram 相等即无 spill）
curl -s http://127.0.0.1:11434/api/ps

# 2) 显存实时余量
nvidia-smi --query-gpu=memory.total,memory.used,memory.free --format=csv

# 3) 服务端日志里把 prefill 和 decode 分开看
grep -E "prompt eval time|eval time|truncated|offloaded" "%LOCALAPPDATA%\Ollama\server.log" | tail -20
```

判定：

- `size_vram < size` 或日志有 `CUDA_Host` 大量占比 → 真的显存不够，考虑降量化/减上下文
- `prompt eval` 很快、`eval` 很慢 → **吞吐问题**，只能减输出 token 或换硬件
- `truncated = 1` → 上下文不够，加 `num_ctx`
- 日志是 `200` 但客户端报错 → **超时配置问题**，与模型无关

## 附录 B：能不能用 NPU 和 4060 一起加速

本机确有 NPU：Intel AI Boost（Meteor Lake，PCI `VEN_8086&DEV_7D1D`，驱动 32.0.100.2540），约 11 TOPS INT8。

**结论：不能给同一个模型叠加加速。** 三个层面的原因，从硬到软：

1. **没有任何运行时支持异构拆分**。把一个模型的 32 层切一半给 CUDA、一半给 NPU，需要统一的调度器和共享张量空间。NPU 和独显是两个独立设备、两套显存（VRAM vs 系统内存），连张量都无法零拷贝共享。llama.cpp / Ollama / vLLM 都不做这件事。
2. **NPU 的算力和带宽都远低于 4060**。11 TOPS vs 4060 的百 TOPS 级；带宽更致命——NPU 只能吃 DDR5-5600 双通道的 **89.6 GB/s**，而 4060 显存是 **272 GB/s**。LLM 解码是访存密集型，NPU 的理论上限大约只有 4060 的 1/3。
3. **GGUF 生态和 NPU 是两条路**。Ollama/llama.cpp 吃 GGUF，需要 ONNX/OpenVINO 转换 + 厂商执行提供器（Intel 走 OpenVINO，Qualcomm 走 QNN）才能在 NPU 上跑。截至 2026 年中，Ollama 与 LM Studio 均未把 LLM 推理路由到 NPU；llama.cpp 的 OpenVINO 后端（2026.1 起为预览）可以让**单个模型**跑在 Intel CPU/GPU/NPU 之一——仍然是三选一，不是叠加。

**能做的替代方案**（如果确实想利用 NPU）：

- **分工而非叠加**：4060 跑 Ollama 的 9B 做语义判断；NPU 用 OpenVINO / Windows ML 跑完全不同的轻任务（OCR、图片预处理、embedding、常驻低功耗小模型）。两者可以同时运行，互不抢同一份算力。
- 代价：NPU 路线需要单独安装 OpenVINO、单独把模型转成 ONNX/IR，是**另一条工具链**，不能复用现有 GGUF 资产。

**所以对你的归档场景**：加速现有归并任务只能靠「减输出 / 减候选 / 换硬件」，NPU 不在这条路径上。

---

## 附：本文数据来源

- `app/test/stress-merge.ts`（真实 prompt 压测，本文第四节三组数据）
- `tools/merge_stress_test.py`（独立对照脚本，不依赖项目代码）
- `%LOCALAPPDATA%\Ollama\server.log`（第五节原始日志，2026-11-17 23:14 那次请求）
- `GET /api/ps` / `nvidia-smi`（显存归属）
- 硬件信息：`Win32_Processor` / `Win32_PhysicalMemory` / `Get-PnpDevice`
