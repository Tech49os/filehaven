//! P1 全部 Rust 单测。
//!
//! 分层放在子模块里，便于按域跑：
//!   cargo test index::   —— 数据层（建表、CRUD、状态机）
//!   cargo test tag::     —— 分词 / kind / 基础标签
//!   cargo test plan::    —— 目录名清洗 / 同名
//!   cargo test error::   —— 错误码

#[test]
fn smoke() {
    assert_eq!(2 + 2, 4);
}

// ================================================================ 数据层

mod index {
    use crate::index as db;
    use crate::model::{NewAssignment, ScannedFile};

    pub fn fresh_db() -> (tempfile::TempDir, rusqlite::Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open(&dir.path().join("t.db")).unwrap();
        (dir, conn)
    }

    fn mk(i: usize, size: u64) -> ScannedFile {
        ScannedFile {
            name: format!("f{i}.docx"),
            path: format!("C:/scanroot/f{i}.docx"),
            ext: "docx".into(),
            kind: "word".into(),
            size,
            mtime: 1_700_000_000_000 + i as i64,
        }
    }

    // T1.2 建库与幂等
    #[test]
    fn open_creates_ten_tables_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let conn = db::open(&path).unwrap();
        let names = db::table_names(&conn).unwrap();
        for t in db::TABLES {
            assert!(names.contains(&t.to_string()), "缺表：{t}");
        }
        assert_eq!(names.len(), db::TABLES.len(), "多了计划外的表：{names:?}");
        drop(conn);

        // 再开一次不许报错、不许重复建表
        let conn2 = db::open(&path).unwrap();
        assert_eq!(db::table_names(&conn2).unwrap().len(), db::TABLES.len());
    }

    // T1.3 settings 读写
    #[test]
    fn settings_set_get() {
        let (_d, conn) = fresh_db();
        assert_eq!(db::get_setting(&conn, "archive_root").unwrap(), None);
        db::set_setting(&conn, "archive_root", "D:/归档").unwrap();
        assert_eq!(
            db::get_setting(&conn, "archive_root").unwrap().as_deref(),
            Some("D:/归档")
        );
        db::set_setting(&conn, "archive_root", "D:/归档2").unwrap();
        assert_eq!(
            db::get_setting(&conn, "archive_root").unwrap().as_deref(),
            Some("D:/归档2")
        );
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM settings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "同一个 key 不应出现两行");
    }

    // T1.4 source_roots CRUD
    #[test]
    fn source_roots_add_list_remove() {
        let (_d, mut conn) = fresh_db();
        let a = db::add_source_root(&conn, "C:/Users/kingb/Desktop").unwrap();
        let b = db::add_source_root(&conn, "C:/Users/kingb/Desktop").unwrap();
        assert_eq!(a.id, b.id, "重复路径必须返回已有记录");
        db::add_source_root(&conn, "D:/工作").unwrap();
        assert_eq!(db::list_source_roots(&conn).unwrap().len(), 2);

        let fid = db::insert_test_file(&conn, a.id, "x.docx", db::Status::Labeled).unwrap();
        let n = db::remove_source_root(&mut conn, a.id).unwrap();
        assert_eq!(n, 1);
        assert_eq!(
            db::get_file_status(&conn, fid).unwrap(),
            Some(db::Status::Missing)
        );
        assert_eq!(db::list_source_roots(&conn).unwrap().len(), 1);
    }

    // T1.5 archive_dirs 树
    #[test]
    fn dirs_tree_rel_path_and_depth() {
        let (_d, conn) = fresh_db();
        let a = db::insert_dir(&conn, None, "A", "user").unwrap();
        assert_eq!((a.rel_path.as_str(), a.depth), ("A", 1));
        let ab = db::insert_dir(&conn, Some(a.id), "B", "user").unwrap();
        let ac = db::insert_dir(&conn, Some(a.id), "C", "user").unwrap();
        assert_eq!((ab.rel_path.as_str(), ab.depth), ("A/B", 2));
        assert_eq!((ac.rel_path.as_str(), ac.depth), ("A/C", 2));

        let err = db::insert_dir(&conn, Some(a.id), "B", "user").unwrap_err();
        assert_eq!(err.code, "DIR_EXISTS");

        let nodes = db::list_dir_nodes(&conn).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].children.len(), 2);
        assert_eq!(nodes[0].file_count, 0);

        assert!(db::dir_by_rel_path(&conn, "A/B").unwrap().is_some());
        assert!(db::dir_by_rel_path(&conn, "A/Z").unwrap().is_none());
    }

    // T1.6 upsert 与批量写
    #[test]
    fn upsert_updates_and_marks_missing() {
        let (_d, mut conn) = fresh_db();
        let root = db::add_source_root(&conn, "C:/scanroot").unwrap();
        let all: Vec<_> = (0..10).map(|i| mk(i, 100)).collect();
        let s1 = db::upsert_files(&mut conn, root.id, &all).unwrap();
        assert_eq!((s1.inserted, s1.updated, s1.missing), (10, 0, 0));

        let eight: Vec<_> = (0..8).map(|i| mk(i, 200)).collect();
        let s2 = db::upsert_files(&mut conn, root.id, &eight).unwrap();
        assert_eq!((s2.inserted, s2.updated, s2.missing), (0, 8, 2));

        assert_eq!(db::file_size(&conn, "C:/scanroot/f0.docx").unwrap(), 200);
        let counts = db::count_by_status(&conn).unwrap();
        let get = |k: &str| {
            counts
                .iter()
                .find(|(s, _)| s == k)
                .map(|(_, n)| *n)
                .unwrap_or(0)
        };
        assert_eq!(get("queued"), 8);
        assert_eq!(get("missing"), 2);

        // 文件回来了：missing → queued
        let s3 = db::upsert_files(&mut conn, root.id, &all).unwrap();
        assert_eq!((s3.inserted, s3.updated, s3.missing), (0, 10, 0));
        assert_eq!(db::count_by_status(&conn).unwrap().len(), 1);
    }

    // T1.7 状态机守卫：8 × 8 = 64 组全覆盖（P1 最重要的单测，不许抽查）
    #[test]
    fn status_machine_all_64_transitions() {
        let (_d, conn) = fresh_db();
        let root = db::add_source_root(&conn, "C:/r").unwrap();
        let fid = db::insert_test_file(&conn, root.id, "x.docx", db::Status::Queued).unwrap();

        let mut ok = 0;
        let mut bad = 0;
        for from in db::Status::ALL {
            for to in db::Status::ALL {
                db::force_status(&conn, fid, from).unwrap();
                let r = db::set_status(&conn, fid, to);
                let expect = db::allowed(from, to);
                assert_eq!(
                    r.is_ok(),
                    expect,
                    "{:?} → {:?} 期望 {}",
                    from,
                    to,
                    if expect { "合法" } else { "非法" }
                );
                if expect {
                    ok += 1;
                    assert_eq!(db::get_file_status(&conn, fid).unwrap(), Some(to));
                } else {
                    bad += 1;
                    assert_eq!(r.unwrap_err().code, "BAD_TRANSITION");
                    assert_eq!(
                        db::get_file_status(&conn, fid).unwrap(),
                        Some(from),
                        "非法转换不许改状态"
                    );
                }
            }
        }
        assert_eq!((ok, bad), (17, 47), "转移表条目数变了？");
    }

    // T4.6 批次落库：关批次把未确认的退回池，且暂缓过的文件必须能再进池
    #[test]
    fn batches_close_and_return_unconfirmed_to_pool() {
        let (_d, mut conn) = fresh_db();
        let root = db::add_source_root(&conn, "C:/r").unwrap();
        let a = db::insert_test_file(&conn, root.id, "a.docx", db::Status::Labeled).unwrap();
        let b = db::insert_test_file(&conn, root.id, "b.docx", db::Status::Labeled).unwrap();
        let c = db::insert_test_file(&conn, root.id, "c.docx", db::Status::Labeled).unwrap();

        let seq1 = db::next_batch_seq(&conn).unwrap();
        let batch1 = db::insert_batch(&conn, seq1, 12).unwrap();
        for id in [a, b] {
            db::set_status(&conn, id, db::Status::PendingConfirm).unwrap();
        }
        assert_eq!(db::pool_rows(&conn).unwrap().len(), 3);

        let back = db::close_batch(&mut conn, batch1).unwrap();
        assert_eq!(back, 2, "未确认的两份应回池");

        let open = db::open_batch(&conn).unwrap();
        assert!(open.is_none(), "关掉之后不该还有未关闭批次");
        let closed_at: Option<i64> = conn
            .query_row("SELECT closed_at FROM batches WHERE id=?1", [batch1], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(closed_at.is_some());

        // 暂缓回来的文件（queued + 已打过 basic 标签）必须重新出现在池里，
        // 否则「S 暂缓」等于把文件扔了（与 §8.4 矛盾）
        let pool = db::pool_rows(&conn).unwrap();
        assert!(pool.iter().any(|f| f.id == a), "暂缓的文件没回到池：{pool:?}");

        let seq2 = db::next_batch_seq(&conn).unwrap();
        db::insert_batch(&conn, seq2, 12).unwrap();
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM batches", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 2);
        assert_eq!(seq2, seq1 + 1);
        let _ = c;
    }

    // T1.8 assignments 写与读
    #[test]
    fn assignments_last_wins() {
        let (_d, conn) = fresh_db();
        let root = db::add_source_root(&conn, "C:/r").unwrap();
        let fid = db::insert_test_file(&conn, root.id, "x.docx", db::Status::PendingConfirm).unwrap();
        let dir = db::insert_dir(&conn, None, "安全检查", "user").unwrap();

        let first = NewAssignment {
            file_id: fid,
            batch_id: None,
            target_dir_id: Some(dir.id),
            origin: "recommended".into(),
            confidence: Some(0.6),
            reason: Some("文件名含「安全」".into()),
            decision: "accepted".into(),
            decided_at: 1000,
        };
        let id1 = db::insert_assignment(&conn, &first).unwrap();
        let mut second = first.clone();
        second.decision = "changed".into();
        second.decided_at = 2000;
        let id2 = db::insert_assignment(&conn, &second).unwrap();

        let last = db::last_assignment(&conn, fid).unwrap().unwrap();
        assert_eq!(last.id, id2);
        assert_ne!(id1, id2);
        assert_eq!(last.decision, "changed");
        assert_eq!(db::count_assigned(&conn, 0).unwrap(), 0);
    }
}

// ================================================================ 分词

mod tag {
    use crate::tag as tk;

    #[derive(serde::Deserialize)]
    struct Raw {
        name: String,
        kind: String,
        size: u64,
        mtime: i64,
    }

    fn stem_of(name: &str) -> &str {
        match name.rfind('.') {
            Some(i) if i > 0 => &name[..i],
            _ => name,
        }
    }

    // T2.1 五条正向
    #[test]
    fn tokenize_basic_cases() {
        // ① 2~3 字段直接保留整段；4 字段是整段 + 2-gram（§7.1 规则 4）
        let t = tk::tokenize("安全检查");
        assert_eq!(t.first().map(|s| s.as_str()), Some("安全检查"), "{t:?}");
        assert!(t.contains(&"安全".to_string()), "{t:?}");

        // ② 长段：整段 + 2-gram（详设 §7.1 规则 4）。
        //    注意 `安全检查通知` 是 6 字段，产出里没有 3 字整词 `安全检查`，
        //    但有 `安全`/`检查`；推荐是双向对称分词，匹配不受影响。
        let t = tk::tokenize("安全检查通知");
        for want in ["安全", "检查", "通知", "安全检查通知"] {
            assert!(t.contains(&want.to_string()), "缺 {want}：{t:?}");
        }

        // ③ 相机默认名全丢（T2.1 要求的「空」）
        assert!(tk::tokenize("IMG_2034").is_empty(), "{:?}", tk::tokenize("IMG_2034"));

        // ④ 停用词与纯数字段不进词元
        let t = tk::tokenize("关于2026年度安全检查的通知");
        for want in ["安全", "检查", "通知"] {
            assert!(t.contains(&want.to_string()), "缺 {want}：{t:?}");
        }
        assert!(!t.iter().any(|x| x == "的"), "{t:?}");
        assert!(!t.iter().any(|x| x.contains("2026")), "{t:?}");

        // ⑤ 长短混合：2~3 字整段保留，噪声段丢弃
        let t = tk::tokenize("AB测试_v2");
        assert!(t.contains(&"测试".to_string()), "{t:?}");
        assert!(!t.iter().any(|x| x == "ab" || x == "v2"), "{t:?}");
    }

    // T2.2 真实语料冒烟
    #[test]
    fn tokenize_real_corpus() {
        let raw = include_str!("../../test/fixtures/real-scan.json");
        let files: Vec<Raw> = serde_json::from_str(raw).unwrap();
        assert_eq!(files.len(), 455, "真实语料条数不对");

        let mut total = 0usize;
        let mut empty = 0usize;
        for f in &files {
            let t = tk::tokenize(stem_of(&f.name));
            if t.is_empty() {
                empty += 1;
            }
            total += t.len();
        }
        let avg = total as f64 / files.len() as f64;
        println!("平均词元数 {avg:.2}，空词元文件 {empty} 份");

        // 详设写「平均 2~8」。2-gram 展开后长文件名必然超过 8，
        // 实测见输出；这里的上限是「别爆炸」的护栏，不是原 8 的硬指标。
        assert!(avg >= 2.0, "平均词元太少：{avg}");
        assert!(avg <= 15.0, "平均词元太多：{avg}");

        // 人工核对样本：必须认出业务词，必须扔掉版本噪声
        let sample = stem_of("04生产工艺设备服务器等硬件设备支撑（二期）_初版草稿_完.docx");
        let t = tk::tokenize(sample);
        for want in ["生产", "工艺", "硬件", "设备", "二期"] {
            assert!(t.contains(&want.to_string()), "缺 {want}：{t:?}");
        }
        for bad in ["初版", "草稿", "完", "04"] {
            assert!(!t.iter().any(|x| x == bad), "不该出现 {bad}：{t:?}");
        }
    }

    // T2.3 kind 归并与入池
    #[test]
    fn kind_mapping_and_should_pool() {
        let pairs = [
            ("doc", "word"),
            ("xls", "excel"),
            ("ppt", "ppt"),
            ("pdf", "pdf"),
            ("image", "image"),
            ("drawio", "diagram"),
            ("text", "word"),
            ("code", "other"),
            ("other", "other"),
        ];
        for (old, new) in pairs {
            assert_eq!(tk::normalize_kind(old), new, "旧 kind {old}");
        }
        // 新 kind 原样通过
        for k in ["word", "excel", "ppt", "pdf", "image", "diagram", "other"] {
            assert_eq!(tk::normalize_kind(k), k);
        }
        // 扩展名直接归类
        assert_eq!(tk::kind_of_ext("docx"), "word");
        assert_eq!(tk::kind_of_ext("XLSX"), "excel");
        assert_eq!(tk::kind_of_ext(".pdf"), "pdf");
        assert_eq!(tk::kind_of_ext("drawio"), "diagram");
        assert_eq!(tk::kind_of_ext("zip"), "other");

        assert!(!tk::should_pool("~$报告.docx", "word", 100));
        assert!(!tk::should_pool("报告.docx", "word", 0));
        assert!(tk::should_pool("a.pdf", "pdf", 100));
        assert!(!tk::should_pool("setup.exe", "other", 100));
        assert!(!tk::should_pool("下载.crdownload", "other", 100));
        assert!(tk::should_pool("图片.png", "image", 10));
    }

    // T2.4 基础标签
    #[test]
    fn basic_tags_type_and_quarter() {
        // 2026-09-12 10:00 UTC
        let ms = 1_789_207_200_000;
        let tags = tk::basic_tags("安全检查报告.docx", "word", ms);
        let names: Vec<&str> = tags.iter().map(|(n, _, _)| n.as_str()).collect();
        assert!(names.contains(&"Word"), "{names:?}");
        assert!(names.contains(&"2026"), "{names:?}");
        assert!(names.contains(&"2026-Q3"), "{names:?}");

        let kw: Vec<&str> = tags
            .iter()
            .filter(|(_, k, _)| k == "keyword")
            .map(|(n, _, _)| n.as_str())
            .collect();
        assert!(kw.len() <= 3, "{kw:?}");
        assert!(kw.contains(&"安全"), "{kw:?}");

        let (y, q) = tk::year_quarter(1_768_435_200_000); // 2026-01-15
        assert_eq!((y, q), (2026, 1));
    }
}

// ================================================================ 目录名

mod plan {
    use crate::plan as pl;

    #[derive(serde::Deserialize)]
    struct Case {
        input: String,
        cleaned: String,
        valid: bool,
        #[serde(default)]
        #[allow(dead_code)]
        reason: Option<String>,
    }

    // T2.5 六条规则各 1 个用例
    #[test]
    fn clean_dir_name_six_rules() {
        // 1 非法字符
        assert_eq!(pl::clean_dir_name("安全:报告*终").cleaned, "安全_报告_终");
        // 2 首尾空格 / 尾部点号 / 首部点号
        assert_eq!(pl::clean_dir_name("  报告...  ").cleaned, "报告");
        assert_eq!(pl::clean_dir_name(".hidden").cleaned, "hidden");
        // 3 连续下划线折叠
        assert_eq!(pl::clean_dir_name("a___b").cleaned, "a_b");
        // 4 超长截断 + 哈希
        let long = "安全生产管理制度汇编与应急预案演练记录汇总材料归档说明文件整理清单及附件目录索引与责任分工与考核办法与培训计划与检查记录与整改通知与验收报告";
        let p = pl::clean_dir_name(long);
        assert_eq!(p.cleaned.chars().count(), 60);
        assert!(p.cleaned.ends_with("_fdae6014"), "{}", p.cleaned);
        // 5 保留名
        assert_eq!(pl::clean_dir_name("con").cleaned, "_con");
        assert_eq!(pl::clean_dir_name("COM1.txt").cleaned, "_COM1.txt");
        // 6 空
        let p = pl::clean_dir_name(" ... ");
        assert!(!p.valid);
        assert_eq!(p.reason.as_deref(), Some("目录名不能为空"));
        // 正常中文不受影响
        assert_eq!(pl::clean_dir_name("智能工厂（二期）").cleaned, "智能工厂（二期）");
    }

    // T2.6 双向 fixtures：Rust 侧跑 20 条
    #[test]
    fn clean_dir_name_cases() {
        let raw = include_str!("../../test/fixtures/dirname-cases.json");
        let cases: Vec<Case> = serde_json::from_str(raw).unwrap();
        assert_eq!(cases.len(), 20);
        for c in &cases {
            let p = pl::clean_dir_name(&c.input);
            assert_eq!(p.cleaned, c.cleaned, "input={:?}", c.input);
            assert_eq!(p.valid, c.valid, "input={:?}", c.input);
        }
    }

    // 多段路径：先拆 `/` 再逐段清洗
    #[test]
    fn clean_path_splits_segments() {
        assert_eq!(pl::clean_path("安全/检查:报告").cleaned, "安全/检查_报告");
        assert_eq!(pl::clean_path("  / 安全 // 报告 ").cleaned, "安全/报告");
        assert!(!pl::clean_path(".../报告").valid);
    }

    // T2.8 unique_name
    #[test]
    fn unique_name_starts_at_two_and_respects_ext() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::write(p.join("报告.docx"), b"x").unwrap();
        assert_eq!(pl::unique_name(p, "报告.docx"), "报告_2.docx");
        std::fs::write(p.join("报告_2.docx"), b"x").unwrap();
        assert_eq!(pl::unique_name(p, "报告.docx"), "报告_3.docx");
        // 不同扩展名不冲突
        assert_eq!(pl::unique_name(p, "报告.pdf"), "报告.pdf");
        assert_eq!(pl::unique_name(p, "全新.docx"), "全新.docx");
    }
}

// ================================================================ 扫描

mod scan {
    use crate::scan as sc;

    // T3.2 系统目录强制排除
    #[test]
    fn scan_skips_system_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for name in ["a.docx", "b.pdf", "c.pptx", "d.xlsx", "e.png"] {
            std::fs::write(root.join(name), b"x").unwrap();
        }
        let nm = root.join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("x.js"), b"x").unwrap();
        std::fs::write(root.join("~$锁文件.docx"), b"x").unwrap();

        let files = sc::scan_dir(root.to_str().unwrap()).unwrap();
        assert_eq!(files.len(), 5, "{files:?}");
        assert!(files.iter().all(|f| !f.name.starts_with("~$")));
    }
}

// ================================================================ 真实语料对拍

mod corpus {
    use crate::tag;
    use std::collections::HashMap;

    #[derive(serde::Deserialize)]
    struct Raw {
        name: String,
        kind: String,
        size: u64,
        mtime: i64,
    }

    fn load() -> Vec<Raw> {
        serde_json::from_str(include_str!("../../test/fixtures/real-scan.json")).unwrap()
    }

    // T3.4 kind 分布与详设 §7.2 的映射表一致
    #[test]
    fn kind_distribution_matches_baseline() {
        let files = load();
        assert_eq!(files.len(), 455);

        let mut dist: HashMap<&'static str, usize> = HashMap::new();
        for f in &files {
            *dist.entry(tag::normalize_kind(&f.kind)).or_default() += 1;
        }
        let get = |k: &str| dist.get(k).copied().unwrap_or(0);
        assert_eq!(get("word"), 116, "doc 109 + text 7");
        assert_eq!(get("excel"), 82);
        assert_eq!(get("ppt"), 142);
        assert_eq!(get("pdf"), 38);
        assert_eq!(get("image"), 17);
        assert_eq!(get("diagram"), 2);
        assert_eq!(get("other"), 58, "code 3 + other 55");

        // R6：跳过的必须能说清是哪类，不许凭空少文件。
        // 58 份 other（压缩包/程序等）。另有 1 份 0 字节的
        // 「新建 文本文档 (2).txt」被判为 word 但按规则不入池 —— 所以是 59，不是 58。
        let not_pooled = files
            .iter()
            .filter(|f| {
                let k = tag::normalize_kind(&f.kind);
                !tag::should_pool(&f.name, k, f.size)
            })
            .count();
        let zero_byte: Vec<&str> = files
            .iter()
            .filter(|f| f.size == 0 && tag::normalize_kind(&f.kind) != "other")
            .map(|f| f.name.as_str())
            .collect();
        println!("不入池 {not_pooled} 份；其中 0 字节非 other：{zero_byte:?}");
        assert_eq!(zero_byte.len(), 1);
        assert_eq!(not_pooled, 58 + zero_byte.len());
    }

    // T3.5 标签落库：queued → labeled，非资料型直接 skipped
    #[test]
    fn labels_move_queued_to_labeled_and_skip_others() {
        use crate::index as db;
        use crate::model::ScannedFile;

        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scanroot").unwrap();

        let files = vec![
            ScannedFile {
                name: "安全检查报告.docx".into(),
                path: "C:/scanroot/安全检查报告.docx".into(),
                ext: "docx".into(),
                kind: "word".into(),
                size: 100,
                mtime: 1_789_207_200_000,
            },
            ScannedFile {
                name: "费率清单.xlsx".into(),
                path: "C:/scanroot/费率清单.xlsx".into(),
                ext: "xlsx".into(),
                kind: "excel".into(),
                size: 100,
                mtime: 1_789_207_200_000,
            },
            ScannedFile {
                name: "安装包.zip".into(),
                path: "C:/scanroot/安装包.zip".into(),
                ext: "zip".into(),
                kind: "other".into(),
                size: 100,
                mtime: 1_789_207_200_000,
            },
        ];
        db::upsert_files(&mut conn, root.id, &files).unwrap();

        let queued = db::list_queued(&conn, Some(root.id)).unwrap();
        assert_eq!(queued.len(), 2, "other 不该进 queued");
        let items: Vec<_> = queued
            .iter()
            .map(|(id, name, kind, mtime)| (*id, tag::basic_tags(name, kind, *mtime)))
            .collect();
        db::apply_basic_labels(&mut conn, &items).unwrap();

        let counts = db::count_by_status(&conn).unwrap();
        let get = |k: &str| {
            counts
                .iter()
                .find(|(s, _)| s == k)
                .map(|(_, n)| *n)
                .unwrap_or(0)
        };
        assert_eq!(get("labeled"), 2);
        assert_eq!(get("skipped"), 1);
        assert_eq!(get("queued"), 0);
    }

    // T5.3 真实语料抽样：六类各留 ≤15 份，合并后正好取满 60
    #[test]
    fn sample_real_corpus_hits_cap() {
        let files: Vec<crate::suggest_dirs::SampleInput> = load()
            .into_iter()
            .map(|f| crate::suggest_dirs::SampleInput {
                name: f.name,
                kind: tag::normalize_kind(&f.kind).to_string(),
                source: "Desktop".into(),
                mtime: f.mtime,
            })
            .collect();
        let picked = crate::suggest_dirs::sample(&files, false);
        println!("真实语料抽样 {}/{}", picked.len(), files.len());
        assert_eq!(picked.len(), crate::suggest_dirs::MAX_SAMPLES);
        // 序号必须从 1 连到 60，prompt 里的 samples 才对得上
        assert_eq!(picked.first().unwrap().no, 1);
        assert_eq!(picked.last().unwrap().no, 60);
    }
}

// ================================================================ 模型

mod llm {
    use crate::error::MODEL_DOWN;
    use crate::llm as m;
    use std::time::Duration;

    // T5.1 前半：连不上就是连不上，不许假装成功
    #[test]
    fn unreachable_base_is_down() {
        let base = "http://127.0.0.1:9";
        assert!(!m::reachable(base, Duration::from_millis(300)));
        let e = m::generate(base, "x", "hi", Duration::from_millis(300)).unwrap_err();
        assert_eq!(e.code, MODEL_DOWN);
    }

    // think 只对思考模型传：给 qwen2.5 传会被 Ollama 400 拒掉
    #[test]
    fn think_only_for_thinking_models() {
        assert!(m::supports_think("qwen3.5:9b"));
        assert!(m::supports_think("qwen3:4b-q4_K_M"));
        assert!(m::supports_think("deepseek-r1:8b"));
        assert!(!m::supports_think("qwen2.5:7b"));
        assert!(!m::supports_think("llama3.1:8b"));
    }
}

// ================================================================ 推荐

mod recommend {
    use crate::model::Candidate;
    use crate::recommend as rc;
    use crate::tag;
    use std::collections::HashMap;

    fn f(tokens: &[&str], stem: &str) -> rc::FileFeature {
        rc::FileFeature {
            tokens: tokens.iter().map(|s| s.to_string()).collect(),
            stem: stem.into(),
            ..Default::default()
        }
    }

    fn d(name: &str, name_tokens: &[&str], confirmed: &[&str], file_count: usize) -> rc::DirFeature {
        rc::DirFeature {
            name_tokens: name_tokens.iter().map(|s| s.to_string()).collect(),
            dir_name: name.into(),
            confirmed_tokens: confirmed.iter().map(|s| s.to_string()).collect(),
            file_count,
        }
    }

    // T4.1 打分：手算期望值
    #[test]
    fn score_hand_computed() {
        let file = f(&["安全检查", "通知"], "安全检查通知.docx");
        let dir = d("安全检查", &["安全检查"], &["验收", "检查"], 3);
        // 目录侧词元 = {安全检查, 验收, 检查} = 3 个，命中 {安全检查} = 1 个
        // → 覆盖率 1/3 = 0.333；stem 含目录名（4 字 ≥3）→ ×1.3 → 0.433
        let s = rc::score(&file, &dir);
        assert!((s - 1.0 / 3.0 * 1.3).abs() < 1e-9, "实际 {s}");

        // 子串命中必须高于同词元的无命中情况
        let no_hit = d("费率清单", &["费率清单"], &[], 0);
        assert!(s > rc::score(&file, &no_hit));
        assert_eq!(rc::score(&file, &no_hit), 0.0);

        // 空集 → 0
        assert_eq!(rc::jaccard(&[], &["a".to_string()]), 0.0);
        assert_eq!(rc::jaccard(&[], &[]), 0.0);
    }

    // T4.2 无理由即作废
    #[test]
    fn candidate_without_reason_is_dropped() {
        let file = f(&["检", "通知"], "检通知.docx");
        // 目录侧只有一个单字词元：能算出分（1/3），但凑不出能读的理由
        let dir = d("检", &["检"], &[], 5);
        assert!(rc::score(&file, &dir) > 0.0);
        assert_eq!(rc::detect_hit(&file, &dir), rc::HitKind::None);
        assert!(rc::reason(&file, &dir, rc::HitKind::None).is_none());
        assert!(rc::rank(&file, &[(1, dir)]).is_empty(), "无理由的候选不该出现在 rank 里");
    }

    // T4.3 0 / 1 目录分支与同分处理
    #[test]
    fn rank_branches_and_tie_break() {
        let file = f(&["安全检查", "通知"], "安全检查通知.docx");
        let paths: HashMap<i64, String> = HashMap::new();

        // 0 目录：不 panic、不报错、needsYou
        let (c, needs) = rc::candidates(&file, &[], &paths);
        assert!(c.is_empty());
        assert!(needs);

        // 1 目录：只有唯一备选，且不给主推
        let (c, needs) = rc::candidates(
            &file,
            &[(7, d("安全检查", &["安全检查"], &[], 0))],
            &paths,
        );
        assert_eq!(c.len(), 1);
        assert!(c[0].uncertain);
        assert!(needs);
        assert_eq!(c[0].dir_id, 7);
        assert!(!c[0].reason.is_empty());

        // 同分按 file_count 降序
        let a = d("安全检查", &["安全检查"], &[], 1);
        let b = d("安全检查", &["安全检查"], &[], 9);
        let ranked = rc::rank(&file, &[(1, a), (2, b)]);
        assert_eq!(ranked[0].0, 2, "同分应先选文件多的目录：{ranked:?}");
    }

    // T4.4 阈值校准（真实语料，跑一遍看覆盖率）
    #[test]
    fn calibrate_thresholds_on_real_corpus() {
        #[derive(serde::Deserialize)]
        struct Raw {
            name: String,
            kind: String,
            size: u64,
            mtime: i64,
        }
        let files: Vec<Raw> =
            serde_json::from_str(include_str!("../../test/fixtures/real-scan.json")).unwrap();

        // 从真实文件名归纳的 5 个目录（人工看过 455 个名字）
        let dir_names = ["安全检查", "智能工厂", "会议纪要", "培训", "报表"];
        let dirs: Vec<(i64, rc::DirFeature)> = dir_names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                (
                    i as i64 + 1,
                    rc::DirFeature {
                        name_tokens: tag::tokenize(n),
                        dir_name: n.to_string(),
                        confirmed_tokens: vec![],
                        file_count: 0,
                    },
                )
            })
            .collect();

        let mut cover = [0usize; 4];
        let mut any_hit = 0usize;
        for raw in &files {
            let k = tag::normalize_kind(&raw.kind);
            if !tag::should_pool(&raw.name, k, raw.size) {
                continue;
            }
            let stem = match raw.name.rfind('.') {
                Some(i) if i > 0 => &raw.name[..i],
                _ => &raw.name,
            };
            let feat = rc::FileFeature {
                tokens: tag::tokenize(stem),
                stem: stem.to_string(),
                kind: k.to_string(),
                mtime: raw.mtime,
            };
            let best = dirs
                .iter()
                .map(|(_, d)| rc::score(&feat, d))
                .fold(0.0f64, f64::max);
            if best > 0.0 {
                any_hit += 1;
            }
            for (i, th) in [0.55f64, 0.35, 0.30, 0.20].iter().enumerate() {
                if best >= *th {
                    cover[i] += 1;
                }
            }
        }
        let total = 396.0f64;
        println!(
            "有分 {} / 396；≥0.55 {:.1}%；≥0.35 {:.1}%；≥0.30 {:.1}%；≥0.20 {:.1}%",
            any_hit,
            cover[0] as f64 / total * 100.0,
            cover[1] as f64 / total * 100.0,
            cover[2] as f64 / total * 100.0,
            cover[3] as f64 / total * 100.0
        );
        assert!(any_hit > 0, "五个目录一个都没命中，分词和打分必有 bug");
    }

    // T4.7 性能：455 × 50 ≤ 300ms
    #[test]
    fn perf_rank_455x50() {
        #[derive(serde::Deserialize)]
        struct Raw {
            name: String,
            mtime: i64,
        }
        let files: Vec<Raw> =
            serde_json::from_str(include_str!("../../test/fixtures/real-scan.json")).unwrap();
        let dirs: Vec<(i64, rc::DirFeature)> = (0..50)
            .map(|i| {
                (
                    i as i64,
                    rc::DirFeature {
                        name_tokens: tag::tokenize(&format!("目录{i}号业务事项")),
                        dir_name: format!("目录{i}号业务事项"),
                        confirmed_tokens: vec!["安全检查".into(), "费率清单".into()],
                        file_count: i as usize,
                    },
                )
            })
            .collect();

        let t0 = std::time::Instant::now();
        let mut acc = 0usize;
        for raw in &files {
            let stem = match raw.name.rfind('.') {
                Some(i) if i > 0 => &raw.name[..i],
                _ => &raw.name,
            };
            let feat = rc::FileFeature {
                tokens: tag::tokenize(stem),
                stem: stem.to_string(),
                ..Default::default()
            };
            acc += rc::rank(&feat, &dirs).len();
        }
        let ms = t0.elapsed().as_millis();
        println!("455 × 50 rank 用时 {ms}ms（命中 {acc}）");
        assert!(ms <= 300, "超预算：{ms}ms");
    }

    // 第 2/3 名备选最多 2 个且不低于 ALT_MIN
    #[test]
    fn alts_are_limited() {
        let file = f(&["安全检查", "通知"], "安全检查通知.docx");
        let dirs: Vec<(i64, rc::DirFeature)> = (1..=5)
            .map(|i| (i, d("安全检查", &["安全检查"], &[], 1)))
            .collect();
        let (c, needs) = rc::candidates(&file, &dirs, &HashMap::new());
        assert!(!needs);
        assert_eq!(c.len(), 1 + rc::MAX_ALTS);
        let _: &Vec<Candidate> = &c;
    }
}

// ================================================================ 批次

mod batch {
    use crate::batch as bt;

    fn item(id: i64, dir: Option<i64>, defer: i32, mtime: i64) -> bt::PoolItem {
        bt::PoolItem {
            file_id: id,
            primary_dir: dir,
            score: if dir.is_some() { 0.6 } else { 0.0 },
            defer_count: defer,
            mtime,
            has_deep_tag: false,
        }
    }

    // T4.5 12 份池（5 同目录 + 4 无推荐 + 3 分散）
    #[test]
    fn pick_puts_hard_first_then_same_dir_together() {
        let mut pool = Vec::new();
        for i in 0..5 {
            pool.push(item(100 + i, Some(1), 0, 1_000 + i));
        }
        for i in 0..4 {
            pool.push(item(200 + i, None, 0, 1_000 + i));
        }
        for i in 0..3 {
            pool.push(item(300 + i, Some(10 + i), 0, 1_000 + i));
        }
        let picked = bt::pick(&pool, 12);
        assert_eq!(picked.len(), 12);
        // 前 4 份都是无推荐的
        assert!(picked[..4].iter().all(|id| (200..204).contains(id)), "{picked:?}");
        // 同目录的 5 份是连着的
        let pos: Vec<usize> = (0..5)
            .map(|i| picked.iter().position(|x| *x == 100 + i).unwrap())
            .collect();
        let min = *pos.iter().min().unwrap();
        let max = *pos.iter().max().unwrap();
        assert_eq!(max - min, 4, "同目录的 5 份应连续：{pos:?}");
    }

    // parked 不在 SQL 之外过滤：pick 只负责排序，过滤责任在 SQL（注释写清）
    #[test]
    fn pick_does_not_filter_by_status() {
        let pool = vec![item(1, None, 0, 10), item(2, None, 0, 20)];
        // 调用方若把 parked 塞进来，pick 依然会选它 —— 说明过滤必须发生在 SQL 层
        assert_eq!(bt::pick(&pool, 2).len(), 2);
    }

    // defer_count ≥ 3 挪到最后
    #[test]
    fn deferred_goes_last() {
        let pool = vec![
            item(1, Some(1), 5, 900),
            item(2, Some(1), 0, 800),
            item(3, Some(1), 0, 700),
        ];
        let picked = bt::pick(&pool, 3);
        assert_eq!(*picked.last().unwrap(), 1, "{picked:?}");
    }
}

// ================================================================ 判定落库

mod decide {
    use crate::error::{BAD_TRANSITION, DIR_INVALID};
    use crate::index as db;
    use crate::model::DecideReq;
    use crate::tests::index::fresh_db;

    fn req(file_id: i64, batch_id: i64, dir_id: Option<i64>, decision: &str) -> DecideReq {
        DecideReq {
            file_id,
            batch_id,
            dir_id,
            decision: decision.into(),
            origin: "recommended".into(),
            confidence: Some(0.8),
            reason: Some("测试理由".into()),
            note: None,
        }
    }

    /// 建源区 + 一个待确认文件 + 一个目录 + 一个批次
    fn setup() -> (tempfile::TempDir, rusqlite::Connection, i64, i64, i64) {
        let (d, conn) = fresh_db();
        let root = db::add_source_root(&conn, "C:/scanroot").unwrap();
        let f = db::insert_test_file(&conn, root.id, "安全检查通知.docx", db::Status::PendingConfirm)
            .unwrap();
        let dir = db::insert_dir(&conn, None, "安全检查", "user").unwrap();
        let batch = db::insert_batch(&conn, 1, 12).unwrap();
        (d, conn, f, dir.id, batch)
    }

    // T6.14 采纳：pending_confirm → confirmed + 一条 assignment
    #[test]
    fn accept_confirms_and_records_assignment() {
        let (_d, mut conn, f, dir, batch) = setup();
        let ack = db::apply_decide(&mut conn, &req(f, batch, Some(dir), "accepted")).unwrap();
        assert_eq!(ack.status, "confirmed");
        assert_eq!(ack.confirmed, 1);
        assert_eq!(ack.target_rel_path.as_deref(), Some("安全检查"));
        assert_eq!(
            db::get_file_status(&conn, f).unwrap(),
            Some(db::Status::Confirmed)
        );
        let a = db::last_assignment(&conn, f).unwrap().unwrap();
        assert_eq!(a.decision, "accepted");
        assert_eq!(a.target_dir_id, Some(dir));
        assert_eq!(db::count_assigned(&conn, batch).unwrap(), 1);
    }

    // 改判也计入「已确认」，但决策值必须是 changed
    #[test]
    fn change_counts_as_confirmed_with_changed_decision() {
        let (_d, mut conn, f, dir, batch) = setup();
        let mut r = req(f, batch, Some(dir), "changed");
        r.origin = "user_changed".into();
        db::apply_decide(&mut conn, &r).unwrap();
        let a = db::last_assignment(&conn, f).unwrap().unwrap();
        assert_eq!(a.decision, "changed");
        assert_eq!(a.origin, "user_changed");
        assert_eq!(db::count_assigned(&conn, batch).unwrap(), 1);
    }

    // 暂缓：回 queued 且 defer_count +1（回池的关键）
    #[test]
    fn defer_returns_to_pool_and_bumps_count() {
        let (_d, mut conn, f, _dir, batch) = setup();
        let ack = db::apply_decide(&mut conn, &req(f, batch, None, "deferred")).unwrap();
        assert_eq!(ack.status, "queued");
        assert_eq!(ack.confirmed, 0);
        assert_eq!(
            db::get_file_status(&conn, f).unwrap(),
            Some(db::Status::Queued)
        );
        let n: i32 = conn
            .query_row("SELECT defer_count FROM files WHERE id=?1", [f], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(n, 1);
        // 回池：下次挑批还能被选到
        assert!(db::pool_rows(&conn).unwrap().iter().any(|r| r.id == f));
    }

    // 先放着：parked + 记下备注
    #[test]
    fn park_sets_status_and_note() {
        let (_d, mut conn, f, _dir, batch) = setup();
        let mut r = req(f, batch, None, "parked");
        r.note = Some("等财务确认".into());
        let ack = db::apply_decide(&mut conn, &r).unwrap();
        assert_eq!(ack.status, "parked");
        assert_eq!(
            db::get_file_status(&conn, f).unwrap(),
            Some(db::Status::Parked)
        );
        let note: Option<String> = conn
            .query_row("SELECT note FROM files WHERE id=?1", [f], |r| r.get(0))
            .unwrap();
        assert_eq!(note.as_deref(), Some("等财务确认"));
        // parked 不在候选池里
        assert!(!db::pool_rows(&conn).unwrap().iter().any(|r| r.id == f));
    }

    // 采纳却没给目录 → DIR_INVALID（不许静默写一条空目标）
    #[test]
    fn accept_without_dir_is_rejected() {
        let (_d, mut conn, f, _dir, batch) = setup();
        let e = db::apply_decide(&mut conn, &req(f, batch, None, "accepted")).unwrap_err();
        assert_eq!(e.code, DIR_INVALID);
        // 失败不许留下半条记录
        assert!(db::last_assignment(&conn, f).unwrap().is_none());
        assert_eq!(
            db::get_file_status(&conn, f).unwrap(),
            Some(db::Status::PendingConfirm)
        );
    }

    // 已确认再确认 → BAD_TRANSITION
    #[test]
    fn decide_illegal_transition_is_rejected() {
        let (_d, mut conn, f, dir, batch) = setup();
        db::apply_decide(&mut conn, &req(f, batch, Some(dir), "accepted")).unwrap();
        let e = db::apply_decide(&mut conn, &req(f, batch, Some(dir), "accepted")).unwrap_err();
        assert_eq!(e.code, BAD_TRANSITION);
    }

    // Ctrl+Z：删掉最近一条 accepted/changed，文件回 queued
    #[test]
    fn undo_restores_file_and_removes_assignment() {
        let (_d, mut conn, f, dir, batch) = setup();
        db::apply_decide(&mut conn, &req(f, batch, Some(dir), "accepted")).unwrap();
        let back = db::undo_last_decide(&mut conn, batch).unwrap().unwrap();
        assert_eq!(back.id, f);
        assert_eq!(
            db::get_file_status(&conn, f).unwrap(),
            Some(db::Status::Queued)
        );
        assert!(db::last_assignment(&conn, f).unwrap().is_none());
        assert_eq!(db::count_assigned(&conn, batch).unwrap(), 0);
    }

    // 空栈：返回 None，不报错
    #[test]
    fn undo_on_empty_batch_returns_none() {
        let (_d, mut conn, _f, _dir, batch) = setup();
        assert!(db::undo_last_decide(&mut conn, batch).unwrap().is_none());
    }

    // 暂缓不可撤销（它本来就没落确认），撤销只针对 accepted/changed
    #[test]
    fn undo_ignores_deferred() {
        let (_d, mut conn, f, _dir, batch) = setup();
        db::apply_decide(&mut conn, &req(f, batch, None, "deferred")).unwrap();
        assert!(db::undo_last_decide(&mut conn, batch).unwrap().is_none());
        // 只回滚一次，defer_count 不被撤销逻辑改动
        let n: i32 = conn
            .query_row("SELECT defer_count FROM files WHERE id=?1", [f], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(n, 1);
    }

    // 关批次：未确认回池，返回剩余池数与关掉的份数
    #[test]
    fn close_batch_returns_unconfirmed_to_pool() {
        let (_d, mut conn, f, _dir, batch) = setup();
        let ack = db::close_batch_cmd(&mut conn, batch).unwrap();
        assert_eq!(ack.closed, 1);
        assert_eq!(ack.remaining_in_pool, 1);
        assert_eq!(
            db::get_file_status(&conn, f).unwrap(),
            Some(db::Status::Queued)
        );
        assert!(db::open_batch(&conn).unwrap().is_none());
    }

    // 采纳方案：多层一起建，origin 记 model_approved，父目录自动补齐
    #[test]
    fn accept_dirs_builds_parents() {
        let (_d, conn) = fresh_db();
        db::set_setting(&conn, "archive_root", "D:/归档").unwrap();
        let made = db::accept_dirs(&conn, &["合同/采购".to_string(), "安全".to_string()]).unwrap();
        assert_eq!(made.len(), 2);
        assert_eq!(made[0].rel_path, "合同/采购");
        assert_eq!(made[0].depth, 2);
        assert_eq!(made[0].origin, "model_approved");
        let parent = db::dir_by_rel_path(&conn, "合同").unwrap().unwrap();
        assert_eq!(parent.origin, "model_approved");
        assert_eq!(db::list_dirs(&conn).unwrap().len(), 3);
    }

    // 已有目录不重复建（换一批后重叠是常态）
    #[test]
    fn accept_dirs_is_idempotent() {
        let (_d, conn) = fresh_db();
        db::set_setting(&conn, "archive_root", "D:/归档").unwrap();
        db::accept_dirs(&conn, &["安全".to_string()]).unwrap();
        let again = db::accept_dirs(&conn, &["安全".to_string()]).unwrap();
        assert_eq!(again.len(), 1);
        assert_eq!(db::list_dirs(&conn).unwrap().len(), 1);
    }

    // 清洗后为空的名字直接拒（不许建出空目录）
    #[test]
    fn accept_dirs_rejects_invalid_name() {
        let (_d, conn) = fresh_db();
        db::set_setting(&conn, "archive_root", "D:/归档").unwrap();
        let e = db::accept_dirs(&conn, &["...".to_string()]).unwrap_err();
        assert_eq!(e.code, DIR_INVALID);
    }
}

// ================================================================ 目录建议

mod suggest_dirs {
    use crate::error::MODEL_BAD_OUTPUT;
    use crate::suggest_dirs as sd;

    fn one(name: &str, kind: &str, mtime: i64) -> sd::SampleInput {
        sd::SampleInput {
            name: name.to_string(),
            kind: kind.to_string(),
            source: "桌面".to_string(),
            mtime,
        }
    }

    fn raw(json: &str) -> sd::RawSchemes {
        sd::parse(json).unwrap()
    }

    // ---------------- 门槛总则

    // M8 收尾：界面上的模型超时上限从 180s 提到 240s
    //
    // 依据：T5.8 三次实跑 81.6s / 164.4s / 114.3s，164s 那次离 180s 只剩 16 秒；
    // 同一 prompt 同一模型耗时抖动来自模型加载与显存分层，不是 prompt 变长。
    // 180s 会让「本来能成」的一次跑成 MODEL_TIMEOUT，白等 3 分钟还给用户降级。
    #[test]
    fn suggest_timeout_has_headroom() {
        assert!(
            crate::SUGGEST_TIMEOUT_SECS >= 240,
            "上限需 ≥240s（实测峰值 164s），实为 {}s",
            crate::SUGGEST_TIMEOUT_SECS
        );
    }

    // ---------------- T5.3 版本族去重

    // T5.3 版本标记剥离：四种常见写法都要认得
    #[test]
    fn strip_version_suffix_handles_common_marks() {
        assert_eq!(sd::strip_version_suffix("04生产工艺设备_初版草稿_完"), "04生产工艺设备");
        assert_eq!(sd::strip_version_suffix("设计方案_v2"), "设计方案");
        assert_eq!(sd::strip_version_suffix("采购合同(1)"), "采购合同");
        assert_eq!(sd::strip_version_suffix("安全检查报告副本"), "安全检查报告");
        assert_eq!(sd::strip_version_suffix("费率清单-最新"), "费率清单");
        // 没有版本标记时原样返回，不许把正常名字吃掉
        assert_eq!(sd::strip_version_suffix("安全检查报告"), "安全检查报告");
    }

    // T5.3 同族只留 mtime 最大的一份
    #[test]
    fn sample_keeps_newest_of_version_family() {
        let mut files = Vec::new();
        for fam in 0..5 {
            for ver in 0..4 {
                files.push(one(
                    &format!("项目文件{fam}_v{ver}.docx"),
                    "word",
                    1_700_000_000_000 + (fam * 10 + ver) as i64,
                ));
            }
        }
        let picked = sd::sample(&files, false);
        assert_eq!(picked.len(), 5, "5 个版本族各剩 1 份");
        for fam in 0..5 {
            let hits: Vec<_> = picked
                .iter()
                .filter(|s| s.name.contains(&format!("项目文件{fam}")))
                .collect();
            assert_eq!(hits.len(), 1);
            // 该族最大 mtime 是 _v3
            assert!(hits[0].name.contains("_v3"), "留下的是最新版本：{}", hits[0].name);
        }
    }

    // T5.3 上限 60
    #[test]
    fn sample_caps_at_sixty() {
        let files: Vec<_> = (0..200)
            .map(|i| one(&format!("不同名字{i:03}.docx"), "word", 1_700_000_000_000 + i))
            .collect();
        assert!(sd::sample(&files, false).len() <= 60);
    }

    // T5.3 每桶 ≤15：只有 word 一种类型时，60 个槽位也只能取 15
    #[test]
    fn sample_takes_at_most_fifteen_per_kind() {
        let files: Vec<_> = (0..40)
            .map(|i| one(&format!("不同名字{i:03}.docx"), "word", 1_700_000_000_000 + i))
            .collect();
        assert_eq!(sd::sample(&files, false).len(), 15);
    }

    // T5.3 少于 10 份直接返回空
    #[test]
    fn sample_empty_below_ten_files() {
        let files: Vec<_> = (0..9)
            .map(|i| one(&format!("不同名字{i}.docx"), "word", 1_700_000_000_000 + i))
            .collect();
        assert!(sd::sample(&files, false).is_empty());
    }

    // T5.3 换一批：换成最旧的一批，保证两次输入不同
    #[test]
    fn sample_force_takes_oldest_first() {
        let files: Vec<_> = (0..40)
            .map(|i| one(&format!("不同名字{i:03}.docx"), "word", 1_700_000_000_000 + i))
            .collect();
        let a = sd::sample(&files, false);
        let b = sd::sample(&files, true);
        assert_eq!(a[0].name, "不同名字039.docx", "默认取最新");
        assert_eq!(b[0].name, "不同名字000.docx", "换一批取最旧");
    }

    // ---------------- T5.4 prompt

    // T5.4 没有已有目录时，「已有目录」那行整行不输出
    #[test]
    fn prompt_omits_existing_dirs_line_when_empty() {
        let samples = vec![sd::Sample {
            no: 1,
            name: "安全检查报告.docx".into(),
            kind: "word".into(),
            source: "桌面".into(),
            mtime: 0,
        }];
        let p = sd::build_prompt(&samples, &[]);
        assert!(!p.contains("已有"));
        assert!(p.contains("1|word|桌面|安全检查报告.docx"));
        assert!(p.contains("只输出 JSON"));
    }

    // T5.4 有已有目录时带上，不许自造同义目录
    #[test]
    fn prompt_lists_existing_dirs_when_present() {
        let samples = vec![sd::Sample {
            no: 1,
            name: "安全检查报告.docx".into(),
            kind: "word".into(),
            source: "桌面".into(),
            mtime: 0,
        }];
        let p = sd::build_prompt(&samples, &["安全检查/年度".to_string(), "合同".to_string()]);
        assert!(p.contains("安全检查/年度"));
        assert!(p.contains("沿用这些维度"));
    }

    // ---------------- T5.4 解析容错

    // T5.4 纯 JSON
    #[test]
    fn parse_plain_json() {
        let r = raw(r#"{"schemes":[{"dimension":"按事项","dirs":[{"name":"安全检查","basis":"b","samples":[1,2]}]}]}"#);
        assert_eq!(r.schemes.len(), 1);
        assert_eq!(r.schemes[0].dirs[0].name, "安全检查");
        assert_eq!(r.schemes[0].dirs[0].samples, vec![1, 2]);
    }

    // T5.4 带 ```json 围栏
    #[test]
    fn parse_strips_code_fence() {
        let r = raw("好的，结果如下：\n```json\n{\"schemes\":[]}\n```\n希望有帮助");
        assert!(r.schemes.is_empty());
    }

    // T5.4 尾部逗号
    #[test]
    fn parse_tolerates_trailing_comma() {
        let r = raw(r#"{"schemes":[{"dimension":"按用途","dirs":[],},],}"#);
        assert_eq!(r.schemes.len(), 1);
    }

    // T5.4 被截断：给 MODEL_BAD_OUTPUT，不许 panic
    #[test]
    fn parse_truncated_is_bad_output() {
        let e = sd::parse(r#"{"schemes":[{"dimension":"按事项","dirs":[{"name":"安全"#).unwrap_err();
        assert_eq!(e.code, MODEL_BAD_OUTPUT);
    }

    // T5.4 模型把 samples 写成字符串也要认（9b 实测高发）
    #[test]
    fn parse_accepts_string_samples() {
        let r = raw(r#"{"schemes":[{"dimension":"按事项","dirs":[{"name":"安全检查","basis":"b","samples":["3","7"]}]}]}"#);
        assert_eq!(r.schemes[0].dirs[0].samples, vec![3, 7]);
    }

    // ---------------- T5.5 五条门槛
    //
    // 基线：两套各 4 个目录。为什么是 4 而不是 3 —— V2/V3/V4 都是「剔掉一个」，
    // 若基线只有 3 个，剔一个就掉到 2、触发整份作废，就测不出「只剔该目录」本身。

    const OK: &str = r#"{"schemes":[
        {"dimension":"按事项","dirs":[
            {"name":"安全检查","basis":"安全类检查记录","samples":[1,2]},
            {"name":"采购合同","basis":"合同与采购单据","samples":[3,4]},
            {"name":"培训台账","basis":"培训签到与计划","samples":[5,6]},
            {"name":"设备维保","basis":"设备台账与保养单","samples":[13,14]}]},
        {"dimension":"按文档用途","dirs":[
            {"name":"制度发布","basis":"正式发布的制度","samples":[7,8]},
            {"name":"汇报材料","basis":"对上汇报的 PPT","samples":[9,10]},
            {"name":"评审记录","basis":"评审会纪要","samples":[11,12]},
            {"name":"台账报表","basis":"周期性报表","samples":[15,16]}]}]}"#;

    // V1 正例：两套各 4 个、维度不同 → 通过
    #[test]
    fn v1_accepts_two_distinct_dimensions() {
        let s = sd::validate(raw(OK), 60).unwrap();
        assert_eq!(s.option_a.len(), 4);
        assert_eq!(s.option_b.len(), 4);
        assert_eq!(s.dimension_a, "按事项");
        assert_eq!(s.option_a[0].file_count, 2);
    }

    // V1 负例：两套维度相同 → 整份作废
    #[test]
    fn v1_rejects_same_dimension() {
        let json = OK.replace("按文档用途", "按事项");
        let e = sd::validate(raw(&json), 60).unwrap_err();
        assert_eq!(e.code, MODEL_BAD_OUTPUT);
        assert!(e.message.contains("V1"), "{}", e.message);
    }

    // V1 负例：一套只有 2 个目录 → 整份作废
    #[test]
    fn v1_rejects_too_few_dirs() {
        let json = r#"{"schemes":[
            {"dimension":"按事项","dirs":[
                {"name":"安全检查","basis":"a","samples":[1,2]},
                {"name":"采购合同","basis":"b","samples":[3,4]}]},
            {"dimension":"按用途","dirs":[
                {"name":"制度发布","basis":"a","samples":[7,8]},
                {"name":"汇报材料","basis":"b","samples":[9,10]},
                {"name":"评审记录","basis":"c","samples":[11,12]}]}]}"#;
        let e = sd::validate(raw(json), 60).unwrap_err();
        assert_eq!(e.code, MODEL_BAD_OUTPUT);
        assert!(e.message.contains("V1"), "{}", e.message);
    }

    // V2 负例：samples 只有 1 个 → 剔掉该目录，首套余 3 个仍可用
    #[test]
    fn v2_drops_dir_with_single_sample() {
        let json = OK.replace(r#""samples":[5,6]"#, r#""samples":[5]"#);
        let s = sd::validate(raw(&json), 60).unwrap();
        assert_eq!(s.option_a.len(), 3);
        assert!(s.option_a.iter().all(|d| d.name != "培训台账"));
    }

    // V2 负例2：序号越界（样本只有 20 份却引用 99）→ 剔掉
    #[test]
    fn v2_drops_out_of_range_index() {
        let json = OK.replace(r#""samples":[7,8]"#, r#""samples":[99,100]"#);
        let s = sd::validate(raw(&json), 20).unwrap();
        assert_eq!(s.option_b.len(), 3, "越界那条被剔掉，余 3 条仍可保留");
        assert!(s.option_b.iter().all(|d| d.name != "制度发布"));
    }

    // V2 负例3：序号 0（模型从 0 开始数）也算越界 —— 提示词要求从 1 开始
    #[test]
    fn v2_drops_zero_index() {
        let json = OK.replace(r#""samples":[5,6]"#, r#""samples":[0,0]"#);
        let s = sd::validate(raw(&json), 60).unwrap();
        assert!(s.option_a.iter().all(|d| d.name != "培训台账"));
    }

    // V3 负例：目录名就是泛词 → 剔除
    #[test]
    fn v3_drops_generic_dir_name() {
        let json = OK.replace(r#""name":"培训台账""#, r#""name":"其他""#);
        let s = sd::validate(raw(&json), 60).unwrap();
        assert_eq!(s.option_a.len(), 3);
        assert!(s.option_a.iter().all(|d| d.name != "其他"));
    }

    // V3 正例：泛词只是名字的一部分（"其他费用凭证"）应当保留
    #[test]
    fn v3_keeps_names_containing_generic_word() {
        let json = OK.replace(r#""name":"培训台账""#, r#""name":"其他费用凭证""#);
        let s = sd::validate(raw(&json), 60).unwrap();
        assert_eq!(s.option_a.len(), 4);
    }

    // V3 增强（M8 收尾）：整名由泛词/版本词拼成 → 剔除
    //
    // 为什么补这条：T5.8 第 3 次实跑给出目录「临时草稿副本」，V3 只看「整名等于泛词」，
    // 「临时」不在泛词表、整名又是拼出来的，于是漏进界面。判定口径改为「摘空即泛」：
    // 把泛词、版本词、修饰词反复从名字里摘掉，摘到空说明这个名字没有任何实义。
    #[test]
    fn v3_drops_composed_filler_name() {
        let json = OK.replace(r#""name":"培训台账""#, r#""name":"临时草稿副本""#);
        let s = sd::validate(raw(&json), 60).unwrap();
        assert!(
            s.option_a.iter().all(|d| d.name != "临时草稿副本"),
            "拼词型泛名必须剔掉，实得 {:?}",
            s.option_a.iter().map(|d| &d.name).collect::<Vec<_>>()
        );
    }

    // V3 增强正例：修饰词 + 实词不能误杀 ——「临时」在「临时用电方案」里只是定语
    #[test]
    fn v3_keeps_filler_word_with_real_noun() {
        let json = OK.replace(r#""name":"培训台账""#, r#""name":"临时用电方案""#);
        let s = sd::validate(raw(&json), 60).unwrap();
        assert_eq!(s.option_a.len(), 4);
        assert!(s.option_a.iter().any(|d| d.name == "临时用电方案"));
    }

    // V3 增强正例2：两个泛词拼起来仍然有实义（「资料归档制度」不是泛名）
    #[test]
    fn v3_keeps_generic_words_plus_noun() {
        let json = OK.replace(r#""name":"培训台账""#, r#""name":"资料归档制度""#);
        let s = sd::validate(raw(&json), 60).unwrap();
        assert_eq!(s.option_a.len(), 4);
        assert!(s.option_a.iter().any(|d| d.name == "资料归档制度"));
    }

    // V4 负例：名字含非法字符（清洗后与原名不一致）→ 剔除
    #[test]
    fn v4_drops_name_needing_clean() {
        let json = OK.replace(r#""name":"培训台账""#, r#""name":"培训/台账""#);
        let s = sd::validate(raw(&json), 60).unwrap();
        assert!(s.option_a.iter().all(|d| d.name != "培训/台账"));
        assert_eq!(s.option_a.len(), 3);
    }

    // V4 负例2：名字 1 个字 → 剔除
    #[test]
    fn v4_drops_too_short_name() {
        let json = OK.replace(r#""name":"培训台账""#, r#""name":"训""#);
        let s = sd::validate(raw(&json), 60).unwrap();
        assert_eq!(s.option_a.len(), 3);
    }

    // V4 负例3：超过 20 字 → 剔除
    #[test]
    fn v4_drops_too_long_name() {
        let long = "超".repeat(21);
        let json = OK.replace(r#""name":"培训台账""#, &format!(r#""name":"{long}""#));
        let s = sd::validate(raw(&json), 60).unwrap();
        assert_eq!(s.option_a.len(), 3);
    }

    // V5 负例：两套目录名重复率 ≥50% → 整份作废
    #[test]
    fn v5_rejects_overlapping_schemes() {
        let json = r#"{"schemes":[
            {"dimension":"按事项","dirs":[
                {"name":"安全检查","basis":"a","samples":[1,2]},
                {"name":"采购合同","basis":"b","samples":[3,4]},
                {"name":"培训台账","basis":"c","samples":[5,6]}]},
            {"dimension":"按用途","dirs":[
                {"name":"安全检查","basis":"a","samples":[7,8]},
                {"name":"采购合同","basis":"b","samples":[9,10]},
                {"name":"评审记录","basis":"c","samples":[11,12]}]}]}"#;
        let e = sd::validate(raw(json), 60).unwrap_err();
        assert_eq!(e.code, MODEL_BAD_OUTPUT);
        assert!(e.message.contains("V5"), "{}", e.message);
    }

    // V5 正例：重复 1/4 = 25% < 50% 通过
    #[test]
    fn v5_accepts_low_overlap() {
        let json = OK.replace(r#""name":"制度发布""#, r#""name":"安全检查""#);
        let s = sd::validate(raw(&json), 60).unwrap();
        assert_eq!(s.option_b.len(), 4);
    }

    // 组合：V3 把一套剔到只剩 2 个 → 整份作废
    #[test]
    fn removing_to_two_dirs_voids_whole_suggestion() {
        let json = OK
            .replace(r#""name":"培训台账""#, r#""name":"资料""#)
            .replace(r#""name":"设备维保""#, r#""name":"杂项""#);
        let e = sd::validate(raw(&json), 60).unwrap_err();
        assert_eq!(e.code, MODEL_BAD_OUTPUT);
        assert!(e.message.contains("V1"), "剔完剩 2 个按 V1 复审作废：{}", e.message);
    }

    // 依据为空时补默认文案，不让前端出现空「依据」
    #[test]
    fn empty_basis_gets_fallback() {
        let json = OK.replace(r#""basis":"安全类检查记录""#, r#""basis":"""#);
        let s = sd::validate(raw(&json), 60).unwrap();
        assert!(!s.option_a[0].basis.is_empty());
    }

    // 少于 10 份直接报 TOO_FEW_FILES，绝不发网络请求（base 指向死端口）
    #[test]
    fn run_rejects_too_few_files_before_network() {
        let files: Vec<_> = (0..9)
            .map(|i| one(&format!("不同名字{i}.docx"), "word", 1_700_000_000_000 + i))
            .collect();
        let mut stages: Vec<String> = Vec::new();
        let e = sd::run(
            sd::RunInput {
                base: "http://127.0.0.1:9",
                model: "x",
                files: &files,
                existing_dirs: &[],
                oldest_first: false,
                timeout: std::time::Duration::from_millis(100),
            },
            &mut |s, _| stages.push(s.to_string()),
        )
        .unwrap_err();
        assert_eq!(e.code, crate::error::TOO_FEW_FILES);
        assert!(stages.is_empty(), "连 sampling 阶段都不该报：{stages:?}");
    }
}

// ================================================================ 错误

mod error {
    use crate::error::{CmdError, IO};

    // T1.10 IO 错误转换
    #[test]
    fn io_error_maps_to_code() {
        let e = std::fs::read("Z:/绝对不存在的路径/x.txt").unwrap_err();
        let ce: CmdError = e.into();
        assert_eq!(ce.code, IO);
        assert!(!ce.message.is_empty());
    }
}

// ================================================================ 迁移计划（T7.1）

mod migration_plan {
    use crate::plan::{self as pl, PlanInput};
    use std::path::Path;

    fn write(p: &Path, bytes: &[u8]) {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(p, bytes).unwrap();
    }

    fn stat(p: &Path) -> (i64, i64) {
        let m = std::fs::metadata(p).unwrap();
        (m.len() as i64, pl::mtime_ms(&m))
    }

    fn input(id: i64, name: &str, src: &Path) -> PlanInput {
        let (size, mtime) = stat(src);
        PlanInput {
            file_id: id,
            name: name.to_string(),
            src_path: src.to_string_lossy().to_string(),
            dir_id: 7,
            dir_rel_path: "安全检查".into(),
            rel_in_source: name.to_string(),
            size,
            mtime,
        }
    }

    // T7.1 三份同目录 + 一份目标已同名 → 1 条 nameConflicts，且第二份落到 _2
    #[test]
    fn build_plans_flags_name_conflict() {
        let src = tempfile::tempdir().unwrap();
        let arc = tempfile::tempdir().unwrap();
        // 归档目标目录里已经有一份 b.docx（上次留下的）
        write(&arc.path().join("安全检查/b.docx"), b"old");

        let mut inputs = Vec::new();
        for (i, n) in ["a.docx", "b.docx", "c.docx"].iter().enumerate() {
            let p = src.path().join(n);
            write(&p, format!("content-{i}").as_bytes());
            inputs.push(input(i as i64 + 1, n, &p));
        }

        let out = pl::build_plans(&inputs, arc.path());
        assert_eq!(out.plans.len(), 3, "不该丢计划：{:?}", out.changed);
        assert_eq!(out.conflicts.len(), 1);
        assert_eq!(out.conflicts[0].target_name, "b_2.docx");
        let b = out.plans.iter().find(|p| p.name == "b.docx").unwrap();
        assert!(b.dst_path.ends_with("b_2.docx"), "{}", b.dst_path);
        assert!(!b.is_new_dir, "目录已在磁盘上，不该标「新建」");
    }

    // 同一批里两份同名（来自不同源目录）也必须错开，不许互相覆盖
    #[test]
    fn build_plans_separates_duplicate_names_inside_one_batch() {
        let src = tempfile::tempdir().unwrap();
        let arc = tempfile::tempdir().unwrap();
        let p1 = src.path().join("甲/报告.docx");
        let p2 = src.path().join("乙/报告.docx");
        write(&p1, b"one");
        write(&p2, b"two");

        let mut a = input(1, "报告.docx", &p1);
        a.rel_in_source = "甲/报告.docx".into();
        let mut b = input(2, "报告.docx", &p2);
        b.rel_in_source = "乙/报告.docx".into();

        let out = pl::build_plans(&[a, b], arc.path());
        assert_eq!(out.plans.len(), 2);
        assert_eq!(out.conflicts.len(), 1);
        let names: Vec<String> = out
            .plans
            .iter()
            .map(|p| Path::new(&p.dst_path).file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"报告.docx".to_string()), "{names:?}");
        assert!(names.contains(&"报告_2.docx".to_string()), "{names:?}");
    }

    // T7.1 标注后被改 / 被删的文件必须拦下，且不生成计划
    #[test]
    fn build_plans_reports_changed_files() {
        let src = tempfile::tempdir().unwrap();
        let arc = tempfile::tempdir().unwrap();
        let p = src.path().join("a.docx");
        write(&p, b"1234");
        let (size, mtime) = stat(&p);

        let mut by_size = input(1, "a.docx", &p);
        by_size.size = size + 1;
        let mut by_mtime = input(2, "a.docx", &p);
        by_mtime.mtime = mtime - 5_000;
        let mut gone = input(3, "已删除.docx", &p);
        gone.src_path = src.path().join("已删除.docx").to_string_lossy().to_string();

        let out = pl::build_plans(&[by_size, by_mtime, gone], arc.path());
        assert!(out.plans.is_empty());
        assert_eq!(out.changed.len(), 3);
        let whys: Vec<&str> = out.changed.iter().map(|c| c.why.as_str()).collect();
        assert!(whys.contains(&"size"));
        assert!(whys.contains(&"mtime"));
        assert!(whys.contains(&"missing"));
    }

    // 隔离区路径保留源目录结构（C3）
    #[test]
    fn quarantine_path_keeps_source_structure() {
        let p = pl::quarantine_path(Path::new("D:/归档"), 12, "子目录/报告.docx");
        let s = p.to_string_lossy().replace('\\', "/");
        assert_eq!(s, "D:/归档/.filebutler/quarantine/12/子目录/报告.docx");
    }

    // R7 磁盘空间判定：能算出剩余就按字节比，算不出就不拦
    #[test]
    fn space_check() {
        use crate::migration::space_ok;
        assert!(space_ok(None, 10 * 1024 * 1024 * 1024));
        assert!(space_ok(Some(2.0), 1_000_000));
        assert!(!space_ok(Some(0.1), 500 * 1024 * 1024));
    }
}

// ================================================================ 迁移执行（T7.3）

mod migrate {
    use crate::migrate::{self as mg, ItemResult};
    use crate::plan::{self as pl, PlanItem};
    use std::path::Path;

    fn write(p: &Path, bytes: &[u8]) {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(p, bytes).unwrap();
    }

    fn plan_of(id: i64, src: &Path, arc: &Path, name: &str, rel_in_source: &str) -> PlanItem {
        let m = std::fs::metadata(src).unwrap();
        PlanItem {
            file_id: id,
            name: name.to_string(),
            src_path: src.to_string_lossy().to_string(),
            dst_path: arc.join("安全检查").join(name).to_string_lossy().to_string(),
            dir_id: 7,
            dir_rel_path: "安全检查".into(),
            rel_in_source: rel_in_source.to_string(),
            size: m.len() as i64,
            mtime: pl::mtime_ms(&m),
            is_new_dir: true,
        }
    }

    // T7.3 三段式：复制 → 校验 → 源文件进隔离区（保留相对结构）
    #[test]
    fn execute_copies_verifies_and_quarantines() {
        let src = tempfile::tempdir().unwrap();
        let arc = tempfile::tempdir().unwrap();
        let p = src.path().join("子目录/报告.docx");
        write(&p, b"hello-migration");

        let mut stages: Vec<String> = Vec::new();
        let plan = plan_of(1, &p, arc.path(), "报告.docx", "子目录/报告.docx");
        let item = mg::execute_one(arc.path(), 12, &plan, &mut |s| stages.push(s.to_string()));

        assert_eq!(item.result, ItemResult::Ok, "{:?}", item.error);
        assert!(item.quarantined);
        assert_eq!(item.sha256.as_deref().map(str::len), Some(64));
        assert!(Path::new(&item.dst_path).is_file());
        assert!(!p.exists(), "源文件必须离开原位置");
        let q = mg::quarantine_path(arc.path(), 12, "子目录/报告.docx");
        assert!(q.is_file(), "隔离区要保留相对结构：{}", q.display());
        assert_eq!(stages, vec!["copy", "verify", "quarantine"]);
        // 归档副本内容一致
        assert_eq!(std::fs::read(&item.dst_path).unwrap(), b"hello-migration");
    }

    // T7.3 标注后源文件被改 → failed(FILE_CHANGED)，源文件不动、目标无残留
    #[test]
    fn execute_refuses_changed_file() {
        let src = tempfile::tempdir().unwrap();
        let arc = tempfile::tempdir().unwrap();
        let p = src.path().join("报告.docx");
        write(&p, b"old");
        let mut plan = plan_of(1, &p, arc.path(), "报告.docx", "报告.docx");

        // 模拟「标注之后用户又改了文件」
        write(&p, b"changed-after-label");
        plan.size = 3;

        let item = mg::execute_one(arc.path(), 12, &plan, &mut |_| {});
        assert_eq!(item.result, ItemResult::Failed);
        assert_eq!(item.code, Some(crate::error::FILE_CHANGED));
        assert!(p.is_file(), "源文件必须留在原地");
        let dst = arc.path().join("安全检查/报告.docx");
        assert!(!dst.exists(), "不许留下半份副本");
        assert!(!mg::quarantine_path(arc.path(), 12, "报告.docx").exists());
    }

    // 目标已存在 → 自动 _2，绝不覆盖
    #[test]
    fn execute_never_overwrites_existing_target() {
        let src = tempfile::tempdir().unwrap();
        let arc = tempfile::tempdir().unwrap();
        write(&arc.path().join("安全检查/报告.docx"), b"existing");
        let p = src.path().join("报告.docx");
        write(&p, b"new-one");

        let plan = plan_of(1, &p, arc.path(), "报告.docx", "报告.docx");
        let item = mg::execute_one(arc.path(), 12, &plan, &mut |_| {});
        assert_eq!(item.result, ItemResult::Ok, "{:?}", item.error);
        assert!(item.dst_path.ends_with("报告_2.docx"), "{}", item.dst_path);
        assert_eq!(
            std::fs::read(arc.path().join("安全检查/报告.docx")).unwrap(),
            b"existing"
        );
    }

    // 源文件已消失 → 失败但不 panic，也不碰归档区
    #[test]
    fn execute_fails_when_source_missing() {
        let src = tempfile::tempdir().unwrap();
        let arc = tempfile::tempdir().unwrap();
        let p = src.path().join("跑掉了.docx");
        write(&p, b"x");
        let plan = plan_of(1, &p, arc.path(), "跑掉了.docx", "跑掉了.docx");
        std::fs::remove_file(&p).unwrap();

        let item = mg::execute_one(arc.path(), 12, &plan, &mut |_| {});
        assert_eq!(item.result, ItemResult::Failed);
        assert!(!arc.path().join("安全检查/跑掉了.docx").exists());
    }
}

// ================================================================ 迁移全链路（T7.2 / T7.4 / T7.5）

mod migration_flow {
    use crate::index as db;
    use crate::migration as mig;
    use crate::model::{NewAssignment, ScannedFile};
    use crate::plan::mtime_ms;
    use crate::tests::index::fresh_db;
    use std::path::Path;

    struct Seeded {
        _src: tempfile::TempDir,
        _arc: tempfile::TempDir,
        conn: rusqlite::Connection,
        dir_id: i64,
        ids: Vec<i64>,
        arc_path: std::path::PathBuf,
        src_path: std::path::PathBuf,
    }

    /// 造一个真实目录 + 库里 3 份已确认文件（size/mtime 与磁盘一致）
    fn seed(extra: &[(&str, &[u8])]) -> Seeded {
        let src = tempfile::tempdir().unwrap();
        let arc = tempfile::tempdir().unwrap();
        let (_d, mut conn) = fresh_db();
        let root = db::add_source_root(&conn, &src.path().to_string_lossy()).unwrap();

        let mut scanned = Vec::new();
        let mut names: Vec<(String, Vec<u8>)> = vec![
            ("安全检查通知.docx".into(), b"aaa".to_vec()),
            ("检查表.xlsx".into(), b"bbbb".to_vec()),
            ("整改报告.pdf".into(), b"ccccc".to_vec()),
        ];
        for (n, b) in extra {
            names.push((n.to_string(), b.to_vec()));
        }
        for (name, bytes) in &names {
            let p = src.path().join(name);
            std::fs::write(&p, bytes).unwrap();
            let m = std::fs::metadata(&p).unwrap();
            scanned.push(ScannedFile {
                name: name.clone(),
                path: p.to_string_lossy().to_string(),
                ext: name.rsplit('.').next().unwrap_or("").to_string(),
                kind: "word".into(),
                size: m.len(),
                mtime: mtime_ms(&m),
            });
        }
        db::upsert_files(&mut conn, root.id, &scanned).unwrap();

        let dir = db::insert_dir(&conn, None, "安全检查", "user").unwrap();
        let ids: Vec<i64> = db::list_queued(&conn, Some(root.id))
            .unwrap()
            .into_iter()
            .map(|(id, ..)| id)
            .collect();
        for id in &ids {
            db::force_status(&conn, *id, db::Status::Confirmed).unwrap();
            db::insert_assignment(
                &conn,
                &NewAssignment {
                    file_id: *id,
                    batch_id: None,
                    target_dir_id: Some(dir.id),
                    origin: "user_picked".into(),
                    confidence: None,
                    reason: None,
                    decision: "accepted".into(),
                    decided_at: db::now_ms(),
                },
            )
            .unwrap();
        }

        Seeded {
            src_path: src.path().to_path_buf(),
            arc_path: arc.path().to_path_buf(),
            _src: src,
            _arc: arc,
            conn,
            dir_id: dir.id,
            ids,
        }
    }

    /// 递归数隔离区里的文件数
    fn files_in(root: &Path) -> usize {
        walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .count()
    }

    // T7.2 预览：分组 / 新建目录标记 / 字节合计 / 空间不足
    #[test]
    fn preview_groups_bytes_and_space() {
        let s = seed(&[]);
        let p = mig::preview(&s.conn, &s.arc_path, Some(10.0)).unwrap();
        assert_eq!(p.total_files, 3);
        assert_eq!(p.groups.len(), 1, "同一目录要合成一组");
        assert_eq!(p.groups[0].dir_id, s.dir_id);
        assert_eq!(p.groups[0].rel_path, "安全检查");
        assert!(p.groups[0].is_new, "磁盘上还没有这个目录");
        assert_eq!(p.total_bytes, 3 + 4 + 5);
        assert!(p.enough_space);

        // 1e-9 GB ≈ 1 字节，装不下 12 字节
        let tight = mig::preview(&s.conn, &s.arc_path, Some(1e-9)).unwrap();
        assert!(!tight.enough_space, "0.000000001GB 装不下 12 字节，必须拦");
    }

    // T7.4 应用迁移：3 份进归档 + 3 份进隔离区 + 台账齐全
    #[test]
    fn apply_migrates_all_and_records_items() {
        let mut s = seed(&[]);
        let mut calls: Vec<(usize, usize, String)> = Vec::new();
        let res = mig::apply(&mut s.conn, &s.arc_path, &[], &mut |done, total, name, _stage| {
            calls.push((done, total, name.to_string()));
        })
        .unwrap();

        assert_eq!(res.copied, 3);
        assert_eq!(res.verified, 3);
        assert_eq!(res.quarantined, 3);
        assert!(res.failed.is_empty(), "{:?}", res.failed);
        assert_eq!(files_in(&s.arc_path.join("安全检查")), 3);
        assert_eq!(files_in(&s.arc_path.join(".filebutler/quarantine")), 3);
        // 进度事件：每文件三次
        assert_eq!(calls.len(), 9);
        assert_eq!(
            db::latest_migration(&s.conn).unwrap().unwrap().status,
            "done"
        );
        assert_eq!(
            db::migration_items(&s.conn, res.migration_id).unwrap().len(),
            3
        );
        for id in &s.ids {
            assert_eq!(
                db::get_file_status(&s.conn, *id).unwrap(),
                Some(db::Status::Migrated)
            );
        }
        // 源目录只剩空壳
        assert_eq!(files_in(&s.src_path), 0);
    }

    // T7.4 取消勾选的回池，不参与这次迁移
    #[test]
    fn apply_sends_excluded_files_back_to_pool() {
        let mut s = seed(&[]);
        let excluded = s.ids[0];
        let res = mig::apply(&mut s.conn, &s.arc_path, &[excluded], &mut |_, _, _, _| {}).unwrap();
        assert_eq!(res.copied, 2);
        assert_eq!(
            db::get_file_status(&s.conn, excluded).unwrap(),
            Some(db::Status::Queued)
        );
        assert!(s.src_path.join("安全检查通知.docx").is_file(), "没被搬走");
    }

    // T7.5 撤销：回到原路径、归档副本删除、隔离区清空、状态回 confirmed
    #[test]
    fn undo_restores_everything() {
        let mut s = seed(&[]);
        let originals: Vec<Vec<u8>> = std::fs::read_dir(&s.src_path)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .map(|e| std::fs::read(e.path()).unwrap())
            .collect();

        let res = mig::apply(&mut s.conn, &s.arc_path, &[], &mut |_, _, _, _| {}).unwrap();
        let undo = mig::undo(&mut s.conn, &s.arc_path, res.migration_id, &mut |_, _, _, _| {}).unwrap();
        assert_eq!(undo.restored, 3, "{:?}", undo.failed);
        assert!(undo.failed.is_empty());
        assert_eq!(files_in(&s.arc_path.join("安全检查")), 0, "归档副本要删掉");
        assert_eq!(
            files_in(&s.arc_path.join(".filebutler/quarantine")),
            0,
            "隔离区要清空（目录保留）"
        );

        let mut now: Vec<Vec<u8>> = std::fs::read_dir(&s.src_path)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .map(|e| std::fs::read(e.path()).unwrap())
            .collect();
        let mut a = originals;
        a.sort();
        now.sort();
        assert_eq!(a, now, "撤销后必须逐字节一致");
        for id in &s.ids {
            assert_eq!(
                db::get_file_status(&s.conn, *id).unwrap(),
                Some(db::Status::Confirmed)
            );
        }
        // 再撤一次：明确拒绝，不重复搬
        let again = mig::undo(&mut s.conn, &s.arc_path, res.migration_id, &mut |_, _, _, _| {});
        assert!(again.is_err());
    }

    // T7.5 / R8 隔离区缺一份 → 4 份里 1 失败，其余照常还原
    #[test]
    fn undo_continues_when_one_quarantine_file_is_gone() {
        let mut s = seed(&[]);
        let res = mig::apply(&mut s.conn, &s.arc_path, &[], &mut |_, _, _, _| {}).unwrap();
        let q = s.arc_path.join(".filebutler/quarantine");
        let victim = walkdir::WalkDir::new(&q)
            .into_iter()
            .filter_map(|e| e.ok())
            .find(|e| e.file_type().is_file())
            .unwrap()
            .into_path();
        std::fs::remove_file(&victim).unwrap();

        let undo = mig::undo(&mut s.conn, &s.arc_path, res.migration_id, &mut |_, _, _, _| {}).unwrap();
        assert_eq!(undo.restored, 2);
        assert_eq!(undo.failed.len(), 1);
    }

    // 没有归档根时不许瞎跑（NOT_CONFIGURED 由命令层挡，这里验空列表不 panic）
    #[test]
    fn preview_with_no_confirmed_files_is_empty() {
        let (_d, conn) = fresh_db();
        let arc = tempfile::tempdir().unwrap();
        let p = mig::preview(&conn, arc.path(), None).unwrap();
        assert_eq!(p.total_files, 0);
        assert!(p.groups.is_empty());
    }

    // 重新确认：把改过的文件退回池里，并按现在的磁盘状态刷新 size/mtime
    #[test]
    fn requeue_refreshes_stat() {
        let s = seed(&[]);
        let id = s.ids[0];
        let p = s.src_path.join("安全检查通知.docx");
        std::fs::write(&p, "用户在标注之后改过内容".as_bytes()).unwrap();
        let m = std::fs::metadata(&p).unwrap();

        db::requeue_confirmed(&s.conn, id, m.len() as i64, mtime_ms(&m)).unwrap();
        assert_eq!(
            db::get_file_status(&s.conn, id).unwrap(),
            Some(db::Status::Queued)
        );
        let row = db::file_row(&s.conn, id).unwrap().unwrap();
        assert_eq!(row.size, m.len() as i64);
        assert_eq!(row.mtime, mtime_ms(&m));
        // 刷新后就不该再被判为「标注后已改」
        let p2 = mig::preview(&s.conn, &s.arc_path, None).unwrap();
        assert_eq!(p2.total_files, 2);
        assert!(p2.changed_since_label.is_empty(), "{:?}", p2.changed_since_label);
    }
}

// ================================================================ 系统调用
//
// 回归：`free_gb` 曾用 `Get-PSDrive` 脚本查剩余空间。GUI 进程（windows_subsystem
// = "windows"）起控制台子进程会**闪一个黑框** —— 用户反馈「点扫描弹出一个命令框」
// 就是它。改成直接问内核后，必须快到不可能是起进程，这条测试就是那道护栏。

mod system {
    use std::time::{Duration, Instant};

    #[test]
    fn free_gb_does_not_spawn_a_process() {
        let t0 = Instant::now();
        let g = crate::free_gb("C:\\");
        let dt = t0.elapsed();
        assert!(g.is_some(), "读不到 C 盘剩余空间");
        assert!(g.unwrap() > 0.0, "剩余空间不可能是 0");
        // 子进程冷启动 250ms 起，直接问内核是微秒级；100ms 两头都有大余量
        assert!(
            dt < Duration::from_millis(100),
            "耗时 {dt:?}，疑似仍在起子进程（黑框会再出现）"
        );
    }

    #[test]
    fn free_gb_walks_up_to_an_existing_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        assert!(crate::free_gb(&nested.to_string_lossy()).is_some());
        // 叶子不存在时也要能沿祖先找到盘符
        let ghost = nested.join("nope").join("deeper");
        assert!(crate::free_gb(&ghost.to_string_lossy()).is_some());
    }
}

// ================================================================ 备注
//
// 整理台「先放着 → 双击写备注」曾经调了一个后端没注册的 `save_note`，
// 点下去报「命令不存在」。数据层的护栏放这儿。

mod note {
    use crate::index as db;
    use crate::model::ScannedFile;

    fn seeded() -> (tempfile::TempDir, tempfile::TempDir, rusqlite::Connection, i64) {
        let src = tempfile::tempdir().unwrap();
        let dbdir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dbdir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, &src.path().to_string_lossy()).unwrap();
        let p = src.path().join("合同.docx");
        std::fs::write(&p, b"xxx").unwrap();
        db::upsert_files(
            &mut conn,
            root.id,
            &[ScannedFile {
                name: "合同.docx".into(),
                path: p.to_string_lossy().to_string(),
                ext: "docx".into(),
                kind: "word".into(),
                size: 3,
                mtime: 1_700_000_000_000,
            }],
        )
        .unwrap();
        let id = db::list_queued(&conn, Some(root.id)).unwrap()[0].0;
        (src, dbdir, conn, id)
    }

    #[test]
    fn set_note_writes_trimmed_value() {
        let (_s, _d, conn, id) = seeded();
        db::set_note(&conn, id, "  等法务确认  ").unwrap();
        let row = db::file_row(&conn, id).unwrap().unwrap();
        assert_eq!(row.note.as_deref(), Some("等法务确认"));
    }

    #[test]
    fn set_note_blank_clears() {
        let (_s, _d, conn, id) = seeded();
        db::set_note(&conn, id, "先放着").unwrap();
        db::set_note(&conn, id, "   ").unwrap();
        assert_eq!(db::file_row(&conn, id).unwrap().unwrap().note, None);
    }
}
