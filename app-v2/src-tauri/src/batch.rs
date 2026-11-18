//! 批次挑选（纯函数，简化版 P1 算法，保留 D10 的 70/30 精神）
//!
//! 依据详设 §7.5：不做完整簇聚类，只做「同主推目录聚合 + 无推荐补齐」。

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct PoolItem {
    pub file_id: i64,
    pub primary_dir: Option<i64>,
    #[allow(dead_code)]
    pub score: f64,
    pub defer_count: i32,
    pub mtime: i64,
    #[allow(dead_code)]
    pub has_deep_tag: bool,
}

/// 无推荐文件占批次的比例
const HARD_RATIO: f64 = 0.3;
/// 暂缓次数达到这个值就排到批次最后
pub const DEFER_BADGE_AT: i32 = 3;

/// 挑选一批：难题在前、同类在后；`defer_count ≥ 3` 的最后并打标
///
/// 入参约定：调用方负责从 SQL 层排除 `parked`（本函数不做状态过滤，
/// 这样它才能是纯函数、可单测）。
pub fn pick(pool: &[PoolItem], size: usize) -> Vec<i64> {
    if pool.is_empty() || size == 0 {
        return Vec::new();
    }
    let target = size.min(pool.len());

    // 1. 难题：无主推目录 → defer 降序 → mtime 降序
    let mut hard: Vec<&PoolItem> = pool.iter().filter(|p| p.primary_dir.is_none()).collect();
    hard.sort_by(|a, b| {
        b.defer_count
            .cmp(&a.defer_count)
            .then(b.mtime.cmp(&a.mtime))
            .then(a.file_id.cmp(&b.file_id))
    });
    let hard_quota = ((size as f64) * HARD_RATIO).round() as usize;
    let mut picked: Vec<i64> = hard
        .iter()
        .take(hard_quota.min(hard.len()))
        .map(|p| p.file_id)
        .collect();

    // 2. 同类：按主推目录分组，组大的先填
    let mut groups: HashMap<i64, Vec<&PoolItem>> = HashMap::new();
    for p in pool.iter() {
        if let Some(d) = p.primary_dir {
            if !picked.contains(&p.file_id) {
                groups.entry(d).or_default().push(p);
            }
        }
    }
    let mut group_list: Vec<(i64, Vec<&PoolItem>)> = groups.into_iter().collect();
    for (_, g) in group_list.iter_mut() {
        g.sort_by(|a, b| b.mtime.cmp(&a.mtime).then(a.file_id.cmp(&b.file_id)));
    }
    group_list.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));

    for (_, g) in &group_list {
        for it in g {
            if picked.len() >= target {
                break;
            }
            picked.push(it.file_id);
        }
        if picked.len() >= target {
            break;
        }
    }

    // 3. 同类不够：先用剩下的难题补，再用池里剩下的
    if picked.len() < target {
        for p in &hard {
            if picked.len() >= target {
                break;
            }
            if !picked.contains(&p.file_id) {
                picked.push(p.file_id);
            }
        }
    }
    if picked.len() < target {
        for p in pool {
            if picked.len() >= target {
                break;
            }
            if !picked.contains(&p.file_id) {
                picked.push(p.file_id);
            }
        }
    }

    // 4. defer_count ≥ 3 的挪到最后（保持相对顺序）
    let mut normal: Vec<i64> = Vec::new();
    let mut deferred: Vec<i64> = Vec::new();
    for id in picked {
        let d = pool
            .iter()
            .find(|p| p.file_id == id)
            .map(|p| p.defer_count)
            .unwrap_or(0);
        if d >= DEFER_BADGE_AT {
            deferred.push(id);
        } else {
            normal.push(id);
        }
    }
    normal.extend(deferred);
    normal
}
