//! 目录建议：抽样 → prompt → 模型 → 解析 → 五条门槛（详设 §7.6）
//!
//! 这里全是纯函数（除 `run`），不碰数据库、不发事件。
//! 命令层（`main.rs`）负责取数据、发心跳；诊断入口 `--suggest` 复用 `run`，
//! 所以「验收跑出来的东西」和「界面上跑出来的东西」是同一条管线。

use crate::error::{CmdError, MODEL_BAD_OUTPUT, TOO_FEW_FILES};
use crate::llm;
use crate::plan::clean_dir_name;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// 少于这么多份就不值得问模型，直接让用户手写（详设 §7.6 第 4 步）
pub const MIN_FILES: usize = 10;
/// 送模型的最大抽样数
pub const MAX_SAMPLES: usize = 60;
/// 每种 kind 最多取几份 —— 防止 PPT 一百多份把 word/pdf 挤掉
pub const PER_KIND: usize = 15;
/// 已有目录最多带几行，防止 prompt 里目录清单比样本还长
pub const MAX_EXISTING: usize = 30;

/// 泛词表：单独成一个目录名就判违规（V3）
pub const FAN_WORDS: &[&str] = &[
    "文件", "资料", "文档", "其他", "杂项", "工作", "个人", "部门", "公司", "项目", "日常", "综合",
    "未分类",
];

// ---------------------------------------------------------------- 入参/出参

/// 抽样输入（命令层从 files 表取，诊断入口从 fixture 取）
#[derive(Clone, Debug)]
pub struct SampleInput {
    pub name: String,
    pub kind: String,
    pub source: String,
    pub mtime: i64,
}

/// 一条抽样（`no` 是 prompt 里的序号，从 1 开始）
#[derive(Clone, Debug)]
pub struct Sample {
    pub no: usize,
    pub name: String,
    pub kind: String,
    pub source: String,
    pub mtime: i64,
}

/// 模型原始输出
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawSchemes {
    #[serde(default)]
    pub schemes: Vec<RawScheme>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawScheme {
    #[serde(default)]
    pub dimension: String,
    #[serde(default, alias = "folders", alias = "directories")]
    pub dirs: Vec<RawDir>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawDir {
    /// 有的模型会写成 "dir"
    #[serde(default, alias = "dir")]
    pub name: String,
    #[serde(default, alias = "why", alias = "reason")]
    pub basis: String,
    #[serde(default, deserialize_with = "de_samples")]
    pub samples: Vec<usize>,
}

/// 一个建议目录（前端 `SuggestedDir`）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedDir {
    pub name: String,
    /// 依据文案，非空（模型没写就兜一句）
    pub basis: String,
    /// 支撑文件数 = samples 长度
    pub file_count: usize,
}

/// 校验后的两套方案（前端 `DirSuggestion`）
///
/// `model`/`elapsed_ms` 由 `run` 回填，纯校验阶段留空 —— 这样 `validate` 不必知道
/// 是谁在跑模型，单测才能一行构造出来。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirSuggestion {
    pub option_a: Vec<SuggestedDir>,
    pub option_b: Vec<SuggestedDir>,
    /// 方案 A 的维度名（如「按事项」），前端拿它当列标题
    pub dimension_a: String,
    pub dimension_b: String,
    pub model: String,
    pub elapsed_ms: u64,
    pub sample_count: usize,
}

// ---------------------------------------------------------------- 抽样

fn stem_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    }
}

/// 剥掉版本标记后的主名（版本族去重用）
///
/// 认这几种尾巴：`_初版` `_草稿` `_完` `_定稿` `_终版` `_v3` `(2)` `（2）` `副本` `-最新`，
/// 而且**循环剥**：`xx_初版草稿_完` 要一路剥到 `xx`。
/// 只剥尾巴，不动中间 —— `v2接口设计` 这种名字不能被吃掉。
pub fn strip_version_suffix(stem: &str) -> String {
    let mut s = stem.trim().to_string();
    loop {
        let next = strip_one(&s);
        match next {
            Some(n) if !n.trim().is_empty() => s = n,
            _ => break,
        }
    }
    s.trim_end_matches(['_', '-', ' ', '·']).trim().to_string()
}

const SUFFIXES: &[&str] = &[
    "_初版", "_初稿", "_草稿", "_定稿", "_终版", "_完", "_最终", "_最新", "-最新", "-最终",
    "-定稿", "最终版", "最新版", "打印版", "副本",
    // 不带下划线的写法：`xx_初版草稿` 剥掉 `_完` 之后尾巴就只剩「草稿」
    "初版", "初稿", "草稿", "定稿", "终版",
];

fn strip_one(s: &str) -> Option<String> {
    // 1) 尾部 (1) / （1）
    for (open, close) in [('(', ')'), ('（', '）')] {
        if let Some(pos) = s.rfind(open) {
            let (head, tail) = s.split_at(pos);
            let inner = tail.trim_start_matches(open).trim_end_matches(close);
            if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
                return Some(head.trim_end_matches(['_', '-', ' ']).to_string());
            }
        }
    }

    // 2) 尾部 vN / _vN / -vN
    let chars: Vec<char> = s.chars().collect();
    let mut i = chars.len();
    while i > 0 && chars[i - 1].is_ascii_digit() {
        i -= 1;
    }
    if i < chars.len() && i > 0 && (chars[i - 1] == 'v' || chars[i - 1] == 'V') {
        let mut h = i - 1;
        if h > 0 && (chars[h - 1] == '_' || chars[h - 1] == '-') {
            h -= 1;
        }
        if h > 0 {
            return Some(chars[..h].iter().collect());
        }
    }

    // 3) 固定尾巴
    for suf in SUFFIXES {
        if let Some(head) = s.strip_suffix(suf) {
            let head = head.trim_end_matches(['_', '-', ' ']);
            if !head.is_empty() {
                return Some(head.to_string());
            }
        }
    }
    None
}

/// 抽样（详设 §7.6 四步）
///
/// `oldest_first` = 「换一批」：默认按 mtime 降序取最近的一批，
/// 换一批改成升序取最旧的一批 —— 比随机洗牌更可解释，也必然换掉大部分文件。
pub fn sample(files: &[SampleInput], oldest_first: bool) -> Vec<Sample> {
    if files.len() < MIN_FILES {
        return Vec::new();
    }

    // 1. 版本族去重：同 stripped 只留 mtime 最大的一份
    let mut newest: HashMap<String, &SampleInput> = HashMap::new();
    for f in files {
        let key = strip_version_suffix(stem_of(&f.name)).to_lowercase();
        match newest.get(&key) {
            Some(prev) if prev.mtime >= f.mtime => {}
            _ => {
                newest.insert(key, f);
            }
        }
    }

    // 2. 按 kind 分桶，每桶按 mtime 排序后取 ≤15
    let mut buckets: HashMap<&str, Vec<&SampleInput>> = HashMap::new();
    for f in newest.values() {
        buckets.entry(f.kind.as_str()).or_default().push(f);
    }
    let mut per_kind: Vec<&SampleInput> = Vec::new();
    for (_, mut items) in buckets {
        sort_by_mtime(&mut items, oldest_first);
        per_kind.extend(items.into_iter().take(PER_KIND));
    }

    // 3. 合并再排一次，取 60（不足就全取）
    sort_by_mtime(&mut per_kind, oldest_first);
    per_kind.truncate(MAX_SAMPLES);

    per_kind
        .into_iter()
        .enumerate()
        .map(|(i, f)| Sample {
            no: i + 1,
            name: f.name.clone(),
            kind: f.kind.clone(),
            source: f.source.clone(),
            mtime: f.mtime,
        })
        .collect()
}

fn sort_by_mtime(items: &mut Vec<&SampleInput>, oldest_first: bool) {
    items.sort_by(|a, b| {
        let base = a.mtime.cmp(&b.mtime).then_with(|| a.name.cmp(&b.name));
        if oldest_first {
            base
        } else {
            base.reverse()
        }
    });
}

// ---------------------------------------------------------------- prompt

/// prompt 钉死在详设 §7.6，改了必须重跑 T5.8 实跑
pub fn build_prompt(samples: &[Sample], existing_dirs: &[String]) -> String {
    let mut s = String::new();
    s.push_str("你在帮一位办公人员整理电脑里的文档。下面是他文件的抽样（已去掉旧版本），\n");
    s.push_str("每行格式：序号|类型|来源|主文件名。来源是他电脑上的哪个目录。\n\n");

    if !existing_dirs.is_empty() {
        let listed: Vec<String> = existing_dirs
            .iter()
            .take(MAX_EXISTING)
            .map(|d| d.replace('\\', "/"))
            .collect();
        s.push_str(&format!(
            "他的归档区已有这些目录：{} —— 请沿用这些维度，只补缺的，不要另起同义目录\n\n",
            listed.join("、")
        ));
    }

    for x in samples {
        s.push_str(&format!("{}|{}|{}|{}\n", x.no, x.kind, x.source, x.name));
    }

    s.push_str(
        "\n任务：判断这个人主要在管理哪些方向，给出两套**不同管理维度**的目录方案。\n\n\
要求：\n\
1. 两套方案的维度必须不同（如「按事项」与「按文档用途」、「按业务线」与「按文档形式」）。\n\
2. 每套 3~8 个目录。目录名 2~20 个汉字。\n\
3. 禁止使用这类泛词：文件、资料、文档、其他、杂项、工作、个人、部门、公司、项目、日常、综合、未分类。\n\
4. 每个目录至少在样本里有 2 个支撑文件，把这些文件的序号填进 samples（写数字，不写文件名）。\n\
5. 目录名要具体，一眼能判断什么文件该放进去。\n\n\
只输出 JSON，不要任何解释文字。格式：\n\
{\"schemes\":[{\"dimension\":\"\",\"dirs\":[{\"name\":\"\",\"basis\":\"\",\"samples\":[1,2]}]}]}\n",
    );
    s
}

// ---------------------------------------------------------------- 解析

/// 容错解析：去 ``` 围栏、取第一个 `{` 到最后一个 `}`、容忍尾部逗号
pub fn parse(raw: &str) -> Result<RawSchemes, CmdError> {
    let body = extract_json(raw)?;
    serde_json::from_str::<RawSchemes>(&body)
        .map_err(|e| CmdError::new(MODEL_BAD_OUTPUT, format!("模型输出不是预期 JSON：{e}")))
}

fn extract_json(raw: &str) -> Result<String, CmdError> {
    let mut s = raw.trim();

    // ```json ... ```
    if let Some(i) = s.find("```") {
        let rest = &s[i + 3..];
        let rest = rest
            .trim_start_matches("json")
            .trim_start_matches("JSON")
            .trim_start_matches('\n');
        s = match rest.find("```") {
            Some(j) => &rest[..j],
            None => rest,
        };
    }

    let start = s
        .find('{')
        .ok_or_else(|| CmdError::new(MODEL_BAD_OUTPUT, "模型输出里没有 JSON 对象"))?;
    let end = s
        .rfind('}')
        .ok_or_else(|| CmdError::new(MODEL_BAD_OUTPUT, "模型输出的 JSON 没有收尾（可能被截断）"))?;
    if end <= start {
        return Err(CmdError::new(MODEL_BAD_OUTPUT, "模型输出的大括号不成对"));
    }
    Ok(drop_trailing_commas(&s[start..=end]))
}

/// 去掉 `,]` / `,}` 里的逗号（字符串里的逗号不动）
fn drop_trailing_commas(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut in_str = false;
    let mut escaped = false;
    for (i, &c) in chars.iter().enumerate() {
        if in_str {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        if c == '"' {
            in_str = true;
            out.push(c);
            continue;
        }
        if c == ',' {
            let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// samples 宽松反序列化：认数字、数字字符串、`{"no":3}` 三种写法
fn de_samples<'de, D>(d: D) -> Result<Vec<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Option::<Vec<serde_json::Value>>::deserialize(d)?;
    let mut out = Vec::new();
    for item in v.unwrap_or_default() {
        let n = match &item {
            serde_json::Value::Number(n) => n.as_u64().map(|x| x as usize),
            serde_json::Value::String(s) => s.trim().parse::<usize>().ok(),
            serde_json::Value::Object(o) => o
                .get("no")
                .or_else(|| o.get("index"))
                .and_then(|x| x.as_u64())
                .map(|x| x as usize),
            _ => None,
        };
        if let Some(n) = n {
            out.push(n);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- 五条门槛

fn reject(gate: &str, detail: impl std::fmt::Display) -> CmdError {
    CmdError::new(MODEL_BAD_OUTPUT, format!("{gate} 不通过：{detail}"))
}

/// V1~V5（详设 §7.6）。剔除型违规逐目录剔除后复审，整份作废返回 `MODEL_BAD_OUTPUT`。
pub fn validate(raw: RawSchemes, sample_count: usize) -> Result<DirSuggestion, CmdError> {
    if raw.schemes.len() < 2 {
        return Err(reject("V1", format!("只给了 {} 套方案", raw.schemes.len())));
    }

    // V1：每套 3~8 个，两套维度不同
    for (i, s) in raw.schemes.iter().take(2).enumerate() {
        let n = s.dirs.len();
        if !(3..=8).contains(&n) {
            return Err(reject(
                "V1",
                format!("第 {} 套有 {} 个目录，要求 3~8 个", i + 1, n),
            ));
        }
    }
    let (a_dim, b_dim) = (
        raw.schemes[0].dimension.trim().to_string(),
        raw.schemes[1].dimension.trim().to_string(),
    );
    if a_dim == b_dim {
        return Err(reject(
            "V1",
            format!("两套方案的维度相同（都是「{a_dim}」）"),
        ));
    }

    // V2/V3/V4：逐目录剔除
    let mut schemes: Vec<&RawScheme> = raw.schemes.iter().take(2).collect();
    let mut cleaned: Vec<Vec<SuggestedDir>> = Vec::new();
    for s in schemes.drain(..) {
        let mut seen: HashSet<String> = HashSet::new();
        let mut kept: Vec<SuggestedDir> = Vec::new();
        for d in &s.dirs {
            let name = d.name.trim().to_string();
            if !passes_gates(&name, &d.samples, sample_count) {
                continue;
            }
            if !seen.insert(name.clone()) {
                continue;
            }
            kept.push(SuggestedDir {
                basis: if d.basis.trim().is_empty() {
                    format!("按文件名归纳（{} 份支撑）", d.samples.len())
                } else {
                    d.basis.trim().to_string()
                },
                name,
                file_count: d.samples.len(),
            });
        }
        cleaned.push(kept);
    }

    // 剔除后复审：还剩几个
    for (i, dirs) in cleaned.iter().enumerate() {
        if dirs.len() < 3 {
            return Err(reject(
                "V1",
                format!("第 {} 套剔除不合格目录后只剩 {} 个", i + 1, dirs.len()),
            ));
        }
    }

    // V5：两套目录名重复率 < 50%
    let a: HashSet<&str> = cleaned[0].iter().map(|d| d.name.as_str()).collect();
    let b: HashSet<&str> = cleaned[1].iter().map(|d| d.name.as_str()).collect();
    let inter = a.intersection(&b).count();
    let smaller = a.len().min(b.len());
    if smaller > 0 && (inter as f64) / (smaller as f64) >= 0.5 {
        return Err(reject(
            "V5",
            format!("两套目录名重复 {inter}/{smaller}，超过 50%"),
        ));
    }

    let option_b = cleaned.pop().unwrap_or_default();
    let option_a = cleaned.pop().unwrap_or_default();
    Ok(DirSuggestion {
        option_a,
        option_b,
        dimension_a: a_dim,
        dimension_b: b_dim,
        model: String::new(),
        elapsed_ms: 0,
        sample_count,
    })
}

/// V2 + V3 + V4：一个目录是否留下
fn passes_gates(name: &str, samples: &[usize], sample_count: usize) -> bool {
    // V2：至少 2 个支撑，且序号都在 1..=sample_count
    if samples.len() < 2 {
        return false;
    }
    if samples.iter().any(|&i| i == 0 || i > sample_count) {
        return false;
    }
    // V3：整名就是泛词
    if FAN_WORDS.contains(&name) {
        return false;
    }
    // V4：2~20 字，且清洗后与原名一致（含非法字符即违规）
    let len = name.chars().count();
    if !(2..=20).contains(&len) {
        return false;
    }
    let cleaned = clean_dir_name(name);
    cleaned.valid && cleaned.cleaned == name
}

// ---------------------------------------------------------------- 一次性跑完

/// 一次目录建议的全部输入（命令层与 `--suggest` 诊断入口共用）
pub struct RunInput<'a> {
    pub base: &'a str,
    pub model: &'a str,
    pub files: &'a [SampleInput],
    pub existing_dirs: &'a [String],
    /// 「换一批」
    pub oldest_first: bool,
    pub timeout: Duration,
}

/// 抽样 → 卸载占用显存的别的模型 → 请求 → 解析 → 校验
///
/// `on_stage` 用来报进度（`sampling` / `waiting` / `validating`），
/// 命令层把它接到 Tauri 事件上，诊断入口接成 stderr。
pub fn run(
    inp: RunInput<'_>,
    on_stage: &mut dyn FnMut(&str, u64),
) -> Result<DirSuggestion, CmdError> {
    let t0 = Instant::now();

    let samples = sample(inp.files, inp.oldest_first);
    if samples.is_empty() {
        return Err(CmdError::new(
            TOO_FEW_FILES,
            format!("只有 {} 份文件，不足 {} 份，先多扫一些或直接自己写目录", inp.files.len(), MIN_FILES),
        ));
    }
    on_stage("sampling", t0.elapsed().as_millis() as u64);

    // 8GB 显存装不下 9b + 4b，请求前先把别的卸掉（历史坑）
    llm::unload_others(inp.base, inp.model, Duration::from_secs(5));

    let prompt = build_prompt(&samples, inp.existing_dirs);
    let raw = llm::generate(inp.base, inp.model, &prompt, inp.timeout)?;

    on_stage("validating", t0.elapsed().as_millis() as u64);
    let parsed = parse(&raw)?;
    let mut s = validate(parsed, samples.len())?;
    s.model = inp.model.to_string();
    s.elapsed_ms = t0.elapsed().as_millis() as u64;
    Ok(s)
}

// ---------------------------------------------------------------- 诊断入口用的小工具

/// 从绝对路径取「来源」：最后两层目录（`C:/Users/x/Desktop/合同` → `Desktop/合同`）
///
/// `C:/Users/<名字>` 这一层要去掉 —— 用户名对这个人的文件分类没有任何信息量，
/// 塞进 prompt 只会浪费 token。
pub fn source_of(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() < 2 {
        return String::new();
    }
    let end = parts.len() - 1; // 去掉文件名
    let mut start = end.saturating_sub(2);
    if start > 0 && parts[start - 1].eq_ignore_ascii_case("users") {
        start += 1;
    }
    parts[start..end].join("/")
}
