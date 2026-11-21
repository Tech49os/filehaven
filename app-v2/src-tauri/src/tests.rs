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

    // 归档前改名：可以只写主干，扩展名不能丢也不能叠
    #[test]
    fn clean_file_name_keeps_exactly_one_ext() {
        assert_eq!(
            pl::clean_file_name("生产智能化项目组汇报", "pptx").cleaned,
            "生产智能化项目组汇报.pptx"
        );
        assert_eq!(
            pl::clean_file_name("生产智能化项目组汇报.pptx", "pptx").cleaned,
            "生产智能化项目组汇报.pptx"
        );
        // 大小写不同也算同一个扩展名
        assert_eq!(
            pl::clean_file_name("汇报.PPTX", "pptx").cleaned,
            "汇报.PPTX"
        );
        // 名字中间的点保留（最终版、v2 这类写法很常见）
        assert_eq!(
            pl::clean_file_name("汇报.final", "pptx").cleaned,
            "汇报.final.pptx"
        );
        // 没扩展名的文件（比如 README）不硬加
        assert_eq!(pl::clean_file_name("会议记录", "").cleaned, "会议记录");
        // 空白 / 全是非法字符 → 明确报错，不写一个空名字进去
        assert!(!pl::clean_file_name("   ", "pptx").valid);
        assert_eq!(
            pl::clean_file_name("   ", "pptx").reason.as_deref(),
            Some("文件名不能为空")
        );
        // 非法字符照目录那套规则换下划线，绝不让它落盘
        assert_eq!(
            pl::clean_file_name("汇报/2026:终稿", "docx").cleaned,
            "汇报_2026_终稿.docx"
        );
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

    /// 按真实分词器造文件特征（名字 → 词元），避免手写字面量和分词逻辑漂移
    fn ft(stem: &str) -> rc::FileFeature {
        rc::FileFeature {
            tokens: tag::tokenize(stem),
            stem: stem.into(),
            kind: "word".into(),
            ..Default::default()
        }
    }

    fn f(tokens: &[&str], stem: &str) -> rc::FileFeature {
        rc::FileFeature {
            tokens: tokens.iter().map(|s| s.to_string()).collect(),
            stem: stem.into(),
            kind: "word".into(),
            ..Default::default()
        }
    }

    fn seg(name: &str) -> rc::DirSeg {
        rc::DirSeg {
            name: name.into(),
            tokens: tag::tokenize(name),
        }
    }

    /// 单段目录（路径就一层）
    fn d(name: &str) -> rc::DirFeature {
        rc::DirFeature {
            segments: vec![seg(name)],
            dir_name: name.into(),
            ..Default::default()
        }
    }

    /// 多段目录：`"信科/A/B"`，叶子段 = 最后一段
    fn dpath(path: &str) -> rc::DirFeature {
        let segments: Vec<rc::DirSeg> = path.split('/').map(seg).collect();
        let dir_name = path.split('/').last().unwrap_or(path).to_string();
        rc::DirFeature {
            segments,
            dir_name,
            ..Default::default()
        }
    }

    /// 已确认文件名 → 词元（**必须先去扩展名**，线上走 `stem_of`，否则 docx 会变成词元）
    fn stem_of(n: &str) -> &str {
        match n.rfind('.') {
            Some(i) if i > 0 => &n[..i],
            _ => n,
        }
    }

    fn with_confirmed(mut dd: rc::DirFeature, names: &[&str]) -> rc::DirFeature {
        for n in names {
            dd.confirmed_files.push(tag::tokenize(stem_of(n)));
            for t in tag::tokenize(stem_of(n)) {
                if !dd.confirmed_tokens.contains(&t) {
                    dd.confirmed_tokens.push(t);
                }
            }
        }
        dd.file_count = names.len();
        dd
    }

    fn with_kinds(mut dd: rc::DirFeature, kinds: &[&str]) -> rc::DirFeature {
        for k in kinds {
            *dd.kind_hist.entry((*k).to_string()).or_insert(0) += 1;
        }
        dd
    }

    fn with_tags(mut dd: rc::DirFeature, tags: &[&str]) -> rc::DirFeature {
        dd.topic_tags = tags.iter().map(|s| s.to_string()).collect();
        dd
    }

    // ============================================================ T1.5 权重

    // 不变量 1：缺项按比例重分配，权重和恒为 1.0
    #[test]
    fn weights_always_sum_to_one() {
        for has_tag in [false, true] {
            for has_family in [false, true] {
                for has_kind in [false, true] {
                    let w = rc::weights(has_tag, has_family, has_kind);
                    let sum = w.name + w.tag + w.family + w.kind;
                    assert!(
                        (sum - 1.0).abs() < 1e-9,
                        "权重和 {sum}（tag={has_tag} family={has_family} kind={has_kind}）"
                    );
                    // 可用项之间的相对序不许被打乱（缺项是 0，不参与比较）
                    let seq: Vec<f64> = [w.name, w.tag, w.family, w.kind]
                        .into_iter()
                        .filter(|v| *v > 0.0)
                        .collect();
                    for i in 1..seq.len() {
                        assert!(seq[i - 1] >= seq[i], "相对序被打乱：{seq:?}");
                    }
                }
            }
        }

        // 四项齐全 → 设计文档原值
        let w = rc::weights(true, true, true);
        assert!((w.name - 0.40).abs() < 1e-9);
        assert!((w.tag - 0.35).abs() < 1e-9);
        assert!((w.family - 0.20).abs() < 1e-9);
        assert!((w.kind - 0.05).abs() < 1e-9);

        // 无标签 → 0.35 按 0.40:0.20:0.05 摊给其余三项（计划 §3.1 的 0.615/0.308/0.077）
        let w = rc::weights(false, true, true);
        assert!((w.name - 0.615_384_615).abs() < 1e-6, "name={}", w.name);
        assert!((w.family - 0.307_692_307).abs() < 1e-6, "family={}", w.family);
        assert!((w.kind - 0.076_923_076).abs() < 1e-6, "kind={}", w.kind);
        assert_eq!(w.tag, 0.0);

        // 只剩 name + kind → 按 0.40:0.05 归一
        let w = rc::weights(false, false, true);
        assert!((w.name - 0.888_888_888).abs() < 1e-6);
        assert!((w.kind - 0.111_111_111).abs() < 1e-6);
    }

    // ============================================================ T1.2 name

    // 不变量 2：同一个文件、同一个目录名，分数不随已确认文件数变化
    #[test]
    fn name_match_never_grows_with_history() {
        let file = ft("安全检查通知");
        let one = with_confirmed(d("安全检查"), &["检查报告2020.docx"]);
        // 50 份全是无关词元：P1 的 dir_union 会把分母从 4 撑到 50+，分数自己往下掉
        let fifty = with_confirmed(d("安全检查"), &vec!["检查报告2020.docx"; 50]);
        assert_eq!(
            rc::name_match(&file, &one),
            rc::name_match(&file, &fifty),
            "name 的分母只有目录侧，已确认文件再多也不许把分数拉下来"
        );
        assert!(rc::name_match(&file, &one) > 0.0);
    }

    // 手算：目录名 4 个字 → 4 个词元全被文件名覆盖 → 1.0 × 4/6 特异性 ×1.3 子串加成
    #[test]
    fn name_match_hand_computed() {
        let file = ft("安全检查通知");
        let dir = d("安全检查");
        let m = rc::name_match(&file, &dir);
        // 目录名 4 词元里命中 3 个（整段词元「安全检查」不在文件词元里，文件那边只有
        // 它自己的整段「安全检查通知」和 2-gram）→ 3/4 × 特异性 4/6 × 子串加成 1.3
        let want = (3.0 / 4.0) * (4.0 / 6.0) * 1.3;
        assert!((m - want).abs() < 1e-9, "实际 {m}，期望 {want}");
        assert!((m - 0.65).abs() < 1e-6);
        // 空 token 的目录名（纯数字/单字）不许 panic，也不许给分
        assert_eq!(rc::name_match(&file, &d("2026年")), 0.0);
        assert_eq!(rc::name_match(&ft("2026"), &dir), 0.0);
    }

    // 段特异性：命中「真实事项名」必须赢过命中「2 个字的浅段」
    #[test]
    fn specific_segment_beats_generic_one() {
        let file = ft("油田化工智能工厂项目进展情况汇报");
        let shallow = dpath("信科/生产智能化项目组/汇报");
        let deep = dpath("信科/生产智能化项目组/智能工厂项目/油田化学项目/验收");
        let a = rc::name_match(&file, &shallow);
        let b = rc::name_match(&file, &deep);
        assert!(b > a, "真实目录应胜过只叫「汇报」的浅目录：deep={b} shallow={a}");
    }

    // ============================================================ T1.3 family

    // ① 3 份同族 → 明显高于无关；③ 全无交集 → 0 且不可用
    #[test]
    fn family_match_top3_average() {
        let file = ft("安全检查通知");
        let same = with_confirmed(
            d("验收"),
            &["安全检查记录.docx", "安全检查报告.docx", "安全检查表.docx"],
        );
        let (s, has) = rc::family_match(&file, &same);
        assert!(has);
        // 每份已确认文件侧 6 / 6 / 5 个词元，命中 3 / 3 / 3 → 0.5 / 0.5 / 0.6
        assert!((s - 0.533_333_333).abs() < 1e-6, "实际 {s}");

        let none = with_confirmed(d("验收"), &["费率清单.xlsx", "招标公告.docx"]);
        let (s2, has2) = rc::family_match(&file, &none);
        assert_eq!(s2, 0.0);
        assert!(!has2, "全无交集 → 该项视为不可用，交给重分配");

        // 空目录
        let (s3, has3) = rc::family_match(&file, &d("验收"));
        assert_eq!(s3, 0.0);
        assert!(!has3);
    }

    // ② 单份近重名不许把整目录拉满：3 份里 1 份同族只能拿 1/3
    #[test]
    fn one_similar_file_cannot_carry_the_dir() {
        let file = ft("安全检查通知");
        let one = with_confirmed(d("验收"), &["安全检查记录.docx"]);
        let mixed = with_confirmed(
            d("验收"),
            &["安全检查记录.docx", "费率清单.xlsx", "招标公告.docx"],
        );
        let (a, _) = rc::family_match(&file, &one);
        let (b, _) = rc::family_match(&file, &mixed);
        assert!(a > b, "1 份同族应高于「1 份同族 + 2 份无关」：{a} vs {b}");
        assert!(
            (b - a / 3.0).abs() < 1e-9,
            "1/3 的份数应给出 1/3 的分：{b} vs {}",
            a / 3.0
        );
    }

    // ============================================================ T1.4 kind

    #[test]
    fn kind_bias_shrinks_towards_half() {
        let mut feat = ft("预提单");
        // 1 份 xlsx 的目录里来一份 xlsx → (1+2)/(1+4) = 0.6，不是 1.0
        let one = with_kinds(d("材料模板"), &["excel"]);
        feat.kind = "excel".into();
        assert!((rc::kind_bias(&feat, &one).0 - 0.6).abs() < 1e-9);

        // 6 份 word 的目录：word → 0.8，pdf → 0.2
        let six = with_kinds(d("验收"), &["word", "word", "word", "word", "word", "word"]);
        feat.kind = "word".into();
        assert!((rc::kind_bias(&feat, &six).0 - 0.8).abs() < 1e-9);
        feat.kind = "pdf".into();
        assert!((rc::kind_bias(&feat, &six).0 - 0.2).abs() < 1e-9);
        assert!(rc::kind_bias(&feat, &six).1, "有类型分布就算可用");

        // 空目录 → 0.5 且不可用
        let (v, has) = rc::kind_bias(&feat, &d("验收"));
        assert_eq!(v, 0.5);
        assert!(!has);
    }

    // ============================================================ T1.6 合成

    // 只有标签命中、名字完全不沾 → 仍要有主推，且理由说得清「标签」
    #[test]
    fn tag_only_hit_still_gives_a_readable_primary() {
        let mut file = ft("扫描件001");
        file.topic_tags = vec!["人才培训".into(), "继续教育".into()];
        let dir = with_tags(
            with_confirmed(
                dpath("人事/人事培训"),
                &["2026年培训计划.docx", "继续教育学时明细.xlsx"],
            ),
            &["人才培训", "继续教育", "学时"],
        );
        let mut other = dpath("信科/汇报");
        other.topic_tags = vec!["汇报材料".into()];

        let dirs = vec![(1, dir), (2, other)];
        let (c, needs) = rc::candidates(&file, &dirs, &HashMap::new());
        assert!(!needs);
        assert_eq!(c[0].dir_id, 1, "标签是唯一线索时也要选对：{c:?}");
        assert!(c[0].reason.contains("标签"), "理由要写「标签」：{}", c[0].reason);
        assert!(
            c[0].score >= rc::PRIMARY,
            "两个标签全中应到主推线：{}",
            c[0].score
        );
    }

    // 四路各命中一次：命中类型与理由都要对得上
    #[test]
    fn four_hits_have_four_readable_reasons() {
        let file = ft("安全检查通知");

        // Name：叶子段命中（短名，只有共同词元，不构成子串）
        assert_eq!(rc::detect_hit(&file, &d("检查")), rc::HitKind::Name);
        let r = rc::reason(&file, &d("检查"), rc::HitKind::Name).unwrap();
        assert!(r.contains("检查"), "{r}");

        // Substring：文件名整段包含目录名（≥3 字）——必须优先于 Name
        assert_eq!(
            rc::detect_hit(&file, &d("安全检查")),
            rc::HitKind::Substring
        );

        // Family：目录名不沾，但已确认文件同族
        let fam = with_confirmed(d("验收"), &["安全检查记录.docx"]);
        assert_eq!(rc::detect_hit(&file, &fam), rc::HitKind::Family);
        let r = rc::reason(&file, &fam, rc::HitKind::Family).unwrap();
        assert!(r.contains("同类"), "{r}");

        // Empty：空目录
        assert_eq!(rc::detect_hit(&file, &d("费率清单")), rc::HitKind::Empty);

        // 祖先段命中时，理由必须说清「上级目录」，不然用户看不懂为什么推到这儿
        let deep = dpath("信科/生产智能化项目组/智能工厂项目/验收");
        let f2 = ft("智能工厂项目进展汇报");
        assert_eq!(rc::detect_hit(&f2, &deep), rc::HitKind::Name);
        let r = rc::reason(&f2, &deep, rc::HitKind::Name).unwrap();
        assert!(r.contains("上级目录"), "{r}");
    }

    // 所有候选的理由都非空（验收 4 的代码级断言）
    #[test]
    fn every_candidate_has_a_reason() {
        let file = ft("安全检查通知");
        let dirs = vec![
            (1, d("安全检查")),
            (2, dpath("信科/汇报")),
            (3, with_confirmed(d("验收"), &["安全检查记录.docx"])),
            (4, d("费率清单")),
        ];
        let (c, _) = rc::candidates(&file, &dirs, &HashMap::new());
        assert!(!c.is_empty());
        for x in &c {
            assert!(!x.reason.is_empty(), "候选 {} 没有理由", x.dir_id);
        }
    }

    // T4.2 回归：能算出分但写不出理由 → 不进候选
    #[test]
    fn candidate_without_reason_is_dropped() {
        // 名字不沾、已确认文件也不沾，只有「类型分布」像 —— 有分，但说不出理由
        let file = ft("预提单");
        let dir = with_confirmed(with_kinds(d("费率清单"), &["word", "word", "word"]), &[
            "报价单.docx",
            "招标公告.docx",
            "合同条款.docx",
        ]);
        let s = rc::score(&file, &dir);
        assert!(s > 0.0, "类型偏置本身会出分（{s}）");
        assert_eq!(rc::detect_hit(&file, &dir), rc::HitKind::None);
        assert!(rc::reason(&file, &dir, rc::HitKind::None).is_none());
        assert!(
            rc::rank(&file, &[(1, dir)]).is_empty(),
            "无理由的候选不该出现在 rank 里"
        );
    }

    // T4.3 回归：0 / 1 目录分支与同分处理
    #[test]
    fn rank_branches_and_tie_break() {
        let file = ft("安全检查通知");
        let paths: HashMap<i64, String> = HashMap::new();

        // 0 目录：不 panic、不报错、needsYou
        let (c, needs) = rc::candidates(&file, &[], &paths);
        assert!(c.is_empty());
        assert!(needs);

        // 1 目录：只有唯一备选，且不给主推
        let (c, needs) = rc::candidates(&file, &[(7, d("安全检查"))], &paths);
        assert_eq!(c.len(), 1);
        assert!(c[0].uncertain);
        assert!(needs);
        assert_eq!(c[0].dir_id, 7);
        assert!(!c[0].reason.is_empty());

        // 同分按 file_count 降序
        let a = with_confirmed(d("安全检查"), &["x1.docx"]);
        let b = with_confirmed(
            d("安全检查"),
            &["x1.docx", "x2.docx", "x3.docx", "x4.docx", "x5.docx"],
        );
        let ranked = rc::rank(&file, &[(1, a), (2, b)]);
        assert_eq!(ranked[0].0, 2, "同分应先选文件多的目录：{ranked:?}");
    }

    // 第 2/3 名备选最多 2 个且不低于 ALT_MIN
    #[test]
    fn alts_are_limited() {
        let file = ft("安全检查通知");
        let dirs: Vec<(i64, rc::DirFeature)> = (1..=5)
            .map(|i| (i, with_confirmed(d("安全检查"), &["x1.docx"])))
            .collect();
        let (c, needs) = rc::candidates(&file, &dirs, &HashMap::new());
        assert!(!needs);
        assert_eq!(c.len(), 1 + rc::MAX_ALTS);
        let _: &Vec<Candidate> = &c;
    }

    // 阈值校准（真实语料，跑一遍看覆盖率）
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
            .map(|(i, n)| (i as i64 + 1, d(n)))
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
                ..Default::default()
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
                    with_confirmed(
                        with_kinds(d(&format!("目录{i}号业务事项")), &["word"]),
                        &["安全检查记录.docx", "费率清单.xlsx"],
                    ),
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
        let p = sd::build_prompt(&samples, &[], None);
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
        let p = sd::build_prompt(&samples, &["安全检查/年度".to_string(), "合同".to_string()], None);
        assert!(p.contains("安全检查/年度"));
        assert!(p.contains("沿用这些维度"));
    }

    // ---------------- 用户补充要求（2026-11-20 实测反馈）

    // 反馈原话：「无法将我的主观意见给到模型，只能让模型一遍又一遍的生成没什么意义」。
    // 真因是 prompt 里没有任何位置能接收用户的话，只剩「换一批」换样本重跑。
    fn one_sample() -> Vec<sd::Sample> {
        vec![sd::Sample {
            no: 1,
            name: "安全检查报告.docx".into(),
            kind: "word".into(),
            source: "桌面".into(),
            mtime: 0,
        }]
    }

    // 没提要求时，prompt 必须跟以前**一字不差**（T5.8 的实跑结论建立在那个 prompt 上）
    #[test]
    fn prompt_without_hint_is_unchanged() {
        let p = sd::build_prompt(&one_sample(), &[], None);
        assert!(!p.contains("补充要求"));
        assert!(!p.contains("他额外"));
        // 老内容逐段点一下，防止「悄悄改了 prompt 还说没改」
        assert!(p.contains("你在帮一位办公人员整理电脑里的文档"));
        assert!(p.contains("1|word|桌面|安全检查报告.docx"));
        assert!(p.contains("只输出 JSON"));
    }

    // 提了要求就要原样带进 prompt，并且明确「优先满足」
    #[test]
    fn prompt_carries_hint_and_says_follow_it_first() {
        let p = sd::build_prompt(&one_sample(), &[], Some("我们按客户分，主要做招投标"));
        assert!(p.contains("我们按客户分，主要做招投标"), "{p}");
        assert!(p.contains("补充要求"));
        assert!(p.contains("优先满足"), "必须说清和他说的冲突时先听他的");
        // 要求仍然在样本行之前，模型先读要求再读样本
        let hint_at = p.find("补充要求").unwrap();
        let sample_at = p.find("1|word|桌面").unwrap();
        assert!(hint_at < sample_at);
    }

    // 多行 / 超长的话要压成一行并截断：换行会打乱 prompt 的分段，几千字只会拖慢推理
    #[test]
    fn hint_is_squashed_to_one_line_and_capped() {
        let h = sd::normalize_hint("  按客户分\n第二行\r\n  第三行  ").unwrap();
        assert!(!h.contains('\n') && !h.contains('\r'));
        assert!(h.starts_with("按客户分"), "{h}");
        assert!(h.contains("第三行"));

        let long = "啊".repeat(2000);
        let h2 = sd::normalize_hint(&long).unwrap();
        assert!(h2.chars().count() <= sd::MAX_HINT_CHARS);
        assert!(!h2.is_empty());
    }

    // 空白等于没提
    #[test]
    fn blank_hint_is_none() {
        assert!(sd::normalize_hint("").is_none());
        assert!(sd::normalize_hint("   \n\t ").is_none());
        assert!(sd::normalize_hint("\r\n").is_none());
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

    // 小语料兜底（2026-11-20）：每个目录只有 1 份支撑时，严格门槛会整份扔掉。
    // 十来份文件凑不出「6 个目录 × 每个 2 份」，实测连续几次都栽在这儿 ——
    // 退一步按「≥1 份」放行，方案能到手比完美审核更重要。
    #[test]
    fn small_corpus_falls_back_to_single_sample() {
        let json = r#"{"schemes":[
            {"dimension":"按事项","dirs":[
                {"name":"安全检查","basis":"a","samples":[1]},
                {"name":"采购合同","basis":"b","samples":[2]},
                {"name":"费用明细","basis":"c","samples":[3]}]},
            {"dimension":"按用途","dirs":[
                {"name":"通知公告","basis":"a","samples":[4]},
                {"name":"原始记录","basis":"b","samples":[5]},
                {"name":"汇报材料","basis":"c","samples":[6]}]}]}"#;
        let s = sd::validate(raw(json), 12).expect("小语料也要能出方案");
        assert_eq!(s.option_a.len(), 3);
        assert_eq!(s.option_b.len(), 3);
        // 支撑数如实报给界面：用户看到「样本 1 份」自己会判断可信度
        assert!(s.option_a.iter().all(|d| d.file_count == 1));
    }

    // 但兜底不能变成「什么都收」：序号越界/目录名是泛词，照样剔
    #[test]
    fn fallback_still_rejects_bogus_dirs() {
        let json = r#"{"schemes":[
            {"dimension":"按事项","dirs":[
                {"name":"安全检查","basis":"a","samples":[1]},
                {"name":"采购合同","basis":"b","samples":[2]},
                {"name":"费用明细","basis":"c","samples":[3]}]},
            {"dimension":"按用途","dirs":[
                {"name":"通知公告","basis":"a","samples":[99]},
                {"name":"其他","basis":"b","samples":[5]},
                {"name":"汇报材料","basis":"c","samples":[6]}]}]}"#;
        let e = sd::validate(raw(json), 12).unwrap_err();
        assert_eq!(e.code, MODEL_BAD_OUTPUT);
        assert!(e.message.contains("V1"), "{}", e.message);
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
                hint: None,
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

    // 回归：「定位」曾把 `/select,<路径>` 塞进 Command::arg，explorer 认不出这个写法，
    // 不报错、直接回落到文档目录（用户看到「打开的是文档目录，不是文件所在文件夹」）。
    // explorer 只认 `/select,"路径"`：路径必须自己带引号，所以要走 raw_arg。
    #[test]
    fn explorer_select_arg_quotes_the_path() {
        let a = crate::explorer_select_arg(r"C:\Users\kingb\Downloads\汇报.pptx");
        assert_eq!(a, "/select,\"C:\\Users\\kingb\\Downloads\\汇报.pptx\"");
        // 带空格的路径同样只能是「一段引号」，不能被 std 再包一层
        let b = crate::explorer_select_arg(r"D:\文档归档\信息 科\年度 汇报.docx");
        assert_eq!(b, "/select,\"D:\\文档归档\\信息 科\\年度 汇报.docx\"");
        assert!(!b.contains("\"\""), "出现双层引号 explorer 就解析失败了：{b}");
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

// ================================================================ 正文预览
//
// 2026-11-20 反馈：「整理台只能看到文件名称，路径和内容我看不到，也无法快速打开文件」。
// 这里管「读得到什么」：正文摘要、图片交给缩略图、读不出来的要说清为什么。
// 打开文件/定位是纯系统调用，不在这一层的测试范围。

mod preview {
    use crate::preview;
    use std::io::Write;
    use std::path::{Path, PathBuf};

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn docx(dir: &Path, body: &str) -> PathBuf {
        let p = dir.join("t.docx");
        let f = std::fs::File::create(&p).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default();
        z.start_file("word/document.xml", opts).unwrap();
        z.write_all(
            format!("<w:document><w:body><w:p><w:r><w:t>{body}</w:t></w:r></w:p></w:body></w:document>")
                .as_bytes(),
        )
        .unwrap();
        z.finish().unwrap();
        p
    }

    #[test]
    fn plain_text_is_returned_verbatim() {
        let d = tmp();
        let p = d.path().join("note.txt");
        std::fs::write(&p, "安全检查记录：三楼配电箱").unwrap();
        let pv = preview::make(&p, "word", "txt", 40);
        assert_eq!(pv.kind, "text");
        assert!(pv.text.contains("三楼配电箱"), "{:?}", pv.text);
        assert!(!pv.truncated);
        assert!(pv.reason.is_empty());
    }

    // 一屏能读个大概就行，多了会把候选人列表挤走
    #[test]
    fn long_text_is_capped_and_flagged() {
        let d = tmp();
        let p = d.path().join("long.md");
        std::fs::write(&p, "合同条款".repeat(600)).unwrap();
        let pv = preview::make(&p, "word", "md", 10_000);
        assert_eq!(pv.text.chars().count(), preview::PREVIEW_CAP);
        assert!(pv.truncated, "截断了就要说，不然用户以为文件就这么短");
    }

    fn pptx(dir: &Path, slides: &[&str]) -> PathBuf {
        let p = dir.join("deck.pptx");
        let f = std::fs::File::create(&p).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default();
        for (i, s) in slides.iter().enumerate() {
            z.start_file(format!("ppt/slides/slide{}.xml", i + 1), opts)
                .unwrap();
            z.write_all(
                format!("<p:sld><a:p><a:t>{s}</a:t></a:p></p:sld>").as_bytes(),
            )
            .unwrap();
        }
        z.finish().unwrap();
        p
    }

    // 2026-11-20 实测反馈：42MB 的 pptx 显示「读不出正文（可能是扫描件、空文档或加密文件）」。
    // 真因是抽取器按**文件总大小**（30MB）直接放弃，可 zip 里真正的 slide XML 只有几 KB ——
    // 而 42MB 全是被图片/视频/嵌入对象占掉的。体积闸只管「整份读进内存」的路，不能一刀切。
    #[test]
    fn huge_pptx_is_still_readable() {
        let d = tmp();
        let p = pptx(d.path(), &["生产智能化项目组工作汇报", "成本核算"]);
        let pv = preview::make(&p, "ppt", "pptx", 42 * 1024 * 1024);
        assert_eq!(pv.kind, "text", "42MB 的 pptx 也得读，{pv:?}");
        assert!(pv.text.contains("生产智能化"), "{:?}", pv.text);
    }

    // docx 是这批文件里最多的，必须真能读出中文正文
    #[test]
    fn docx_body_is_extracted() {
        let d = tmp();
        let p = docx(d.path(), "2026 年度消防演练方案");
        let pv = preview::make(&p, "word", "docx", std::fs::metadata(&p).unwrap().len());
        assert_eq!(pv.kind, "text");
        assert!(pv.text.contains("消防演练"), "{:?}", pv.text);
    }

    // 假 docx（改了扩展名的 exe）不能 panic，也不能假装有正文
    #[test]
    fn broken_docx_degrades_with_a_reason() {
        let d = tmp();
        let p = d.path().join("fake.docx");
        std::fs::write(&p, b"MZ\x90\x00 not a zip at all").unwrap();
        let pv = preview::make(&p, "word", "docx", 20);
        assert_eq!(pv.kind, "none");
        assert!(pv.text.is_empty());
        assert!(!pv.reason.is_empty(), "读不出正文时一定要说清，不能给个空白框");
    }

    // 文件被移走/删了：说明白，别让用户对着空白怀疑程序坏了
    #[test]
    fn missing_file_explains_instead_of_empty() {
        let d = tmp();
        let pv = preview::make(&d.path().join("gone.docx"), "word", "docx", 10);
        assert_eq!(pv.kind, "none");
        assert!(pv.reason.contains("不在") || pv.reason.contains("找不到"), "{:?}", pv.reason);
    }

    // 图片走缩略图，不抽正文
    #[test]
    fn image_reports_image_kind() {
        let d = tmp();
        let p = d.path().join("a.png");
        std::fs::write(&p, b"\x89PNG\r\n\x1a\n").unwrap();
        let pv = preview::make(&p, "image", "png", 8);
        assert_eq!(pv.kind, "image");
        assert!(pv.text.is_empty());
        assert!(pv.reason.is_empty(), "图片有缩略图，不必再写一句废话");
    }

    // 老版二进制 Office 不能掉进「可能是扫描件/空文档/加密文件」那句里 ——
    // 那是把「我们不会解析」说成「你的文件有问题」，用户会白折腾半天
    #[test]
    fn legacy_office_says_it_is_our_limit_not_your_file() {
        let d = tmp();
        let p = d.path().join("旧格式汇报.ppt");
        std::fs::write(&p, b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1").unwrap();
        let pv = preview::make(&p, "ppt", "ppt", 512);
        assert_eq!(pv.kind, "none");
        assert!(pv.reason.contains("老版"), "{:?}", pv.reason);
        assert!(!pv.reason.contains("扫描件"), "别把锅甩给文件：{:?}", pv.reason);
    }

    // PDF 这一版不解析：老实说，别装
    #[test]
    fn pdf_says_it_is_not_parsed() {
        let d = tmp();
        let p = d.path().join("a.pdf");
        std::fs::write(&p, b"%PDF-1.7\n").unwrap();
        let pv = preview::make(&p, "pdf", "pdf", 9);
        assert_eq!(pv.kind, "none");
        assert!(pv.reason.contains("PDF"), "{:?}", pv.reason);
    }
}

// ================================================================ 归档前改名
//
// 2026-11-20 实测反馈：「有些文件我想在确认迁移后顺便改个名字，让他更明确」。
// 名字只记在库里（`files.target_name`），磁盘上的源文件一动不动；
// 迁移时用目标名落盘，原名留在隔离区的相对路径里，撤销照样还原。

mod rename {
    use super::index::fresh_db;
    use crate::index as db;
    use crate::plan as pl;

    // 老库是 CREATE TABLE IF NOT EXISTS 建的，改建表语句不会自动多出新列 ——
    // 打补丁的那一步必须幂等，且真的能让旧库可用
    #[test]
    fn old_db_gains_target_name_column() {
        let (_d, conn) = fresh_db();
        // 模拟改造前建的库
        conn.execute_batch("ALTER TABLE files DROP COLUMN target_name")
            .unwrap();
        assert!(db::ensure_column(&conn, "files", "target_name", "TEXT").unwrap());
        // 第二次什么都不做，也不能报错
        assert!(!db::ensure_column(&conn, "files", "target_name", "TEXT").unwrap());

        let root = db::add_source_root(&conn, "C:/scanroot").unwrap();
        let f = db::insert_test_file(&conn, root.id, "旧库里的文件.docx", db::Status::PendingConfirm)
            .unwrap();
        db::set_target_name(&conn, f, Some("改过的名字.docx")).unwrap();
        let row = db::file_row(&conn, f).unwrap().unwrap();
        assert_eq!(row.target_name.as_deref(), Some("改过的名字.docx"));
    }

    #[test]
    fn renamed_file_keeps_both_names() {
        let (_d, conn) = fresh_db();
        let root = db::add_source_root(&conn, "C:/scanroot").unwrap();
        let f = db::insert_test_file(&conn, root.id, "IMG_2043.docx", db::Status::PendingConfirm)
            .unwrap();

        let row = db::file_row(&conn, f).unwrap().unwrap();
        assert_eq!(db::effective_name(&row), "IMG_2043.docx", "没改名就还是原名");

        let cleaned = pl::clean_file_name("配电箱巡检记录", &row.ext).cleaned;
        assert_eq!(cleaned, "配电箱巡检记录.docx");
        db::set_target_name(&conn, f, Some(&cleaned)).unwrap();

        let row = db::file_row(&conn, f).unwrap().unwrap();
        assert_eq!(row.name, "IMG_2043.docx", "原名是磁盘事实，不能改");
        assert_eq!(db::effective_name(&row), "配电箱巡检记录.docx");

        // 空串 = 恢复原名（用户清空输入框就是这个意思）
        db::set_target_name(&conn, f, Some("   ")).unwrap();
        let row = db::file_row(&conn, f).unwrap().unwrap();
        assert!(row.target_name.is_none());
        assert_eq!(db::effective_name(&row), "IMG_2043.docx");
    }
}

// ================================================================ 目录特征（P2 T1.1）

mod dir_feature {
    use crate::index as db;
    use crate::model::NewAssignment;

    fn assign(conn: &rusqlite::Connection, file_id: i64, dir_id: i64, decision: &str) {
        db::insert_assignment(
            conn,
            &NewAssignment {
                file_id,
                batch_id: None,
                target_dir_id: Some(dir_id),
                origin: "user_picked".into(),
                confidence: None,
                reason: None,
                decision: decision.into(),
                decided_at: db::now_ms(),
            },
        )
        .unwrap();
    }

    // T1.1 一次查全：类型分布 + topic 标签，且不做 N+1
    #[test]
    fn dir_feature_rows_collect_kind_hist_and_tags() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();

        let a = db::insert_dir(&conn, None, "安全环保", "user").unwrap();
        let b = db::insert_dir(&conn, None, "人事培训", "user").unwrap();

        // 2 目录 / 5 份已确认：word 3、pdf 2，其中 2 份带 topic「安全检查」
        let names = [
            ("安全检查通知.docx", "docx", "word", true),
            ("安全检查记录.docx", "docx", "word", true),
            ("安全检查表.docx", "docx", "word", false),
            ("消防验收报告.pdf", "pdf", "pdf", false),
            ("环评批复.pdf", "pdf", "pdf", false),
        ];
        for (n, ext, kind, tagged) in names {
            let fid = db::insert_test_file_kind(
                &conn,
                root.id,
                n,
                ext,
                kind,
                db::Status::Confirmed,
            )
            .unwrap();
            assign(&conn, fid, a.id, "accepted");
            if tagged {
                db::insert_test_topic_tag(&conn, fid, "安全检查").unwrap();
            }
        }

        // 人事培训只有 1 份，且是 parked —— 不该算进目录特征
        let p = db::insert_test_file_kind(
            &conn,
            root.id,
            "培训计划.docx",
            "docx",
            "word",
            db::Status::Parked,
        )
        .unwrap();
        assign(&conn, p, b.id, "parked");

        let rows = db::dir_feature_rows(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        let sa = rows.iter().find(|r| r.rel_path == "安全环保").unwrap();
        assert_eq!(sa.kind_hist.get("word").copied(), Some(3));
        assert_eq!(sa.kind_hist.get("pdf").copied(), Some(2));
        assert_eq!(sa.confirmed_names.len(), 5);
        assert_eq!(sa.topic_tags, vec!["安全检查".to_string()]);

        let hr = rows.iter().find(|r| r.rel_path == "人事培训").unwrap();
        assert!(hr.confirmed_names.is_empty(), "parked 不算已确认");
        assert!(hr.kind_hist.is_empty());
    }
}

// ================================================================ PDF 抽取（P2 T2.3）

mod pdf {
    use crate::extract;

    fn fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../test/fixtures/sample-cn.pdf")
    }

    // 闸门实测：桌面 22 份 PDF 里 14 份有中文文字层（63.6% ≥ 60%），依赖保留
    #[test]
    fn chinese_pdf_yields_text() {
        let p = fixture();
        let size = std::fs::metadata(&p).unwrap().len();
        let (text, _, err) = extract::extract(&p, "pdf", "pdf", size);
        assert!(!err, "不该报解析错误");
        assert!(
            text.chars().count() >= 100,
            "中文 PDF 只抽出 {} 字",
            text.chars().count()
        );
    }

    /// 造一份最小 docx（zip + word/document.xml），不引入任何人的真实文档
    fn write_docx(dir: &std::path::Path, name: &str, text: &str) -> std::path::PathBuf {
        use std::io::Write;
        let p = dir.join(name);
        let f = std::fs::File::create(&p).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let opts: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("[Content_Types].xml", opts).unwrap();
        z.write_all(b"<?xml version=\"1.0\"?><Types/>").unwrap();
        z.start_file("word/document.xml", opts).unwrap();
        let xml = format!("<w:document><w:body><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:body></w:document>");
        z.write_all(xml.as_bytes()).unwrap();
        z.finish().unwrap();
        p
    }

    // 回归（2026-11-20）：库里 kind 是 word/excel，解析器要的是 doc/xls ——
    // 接深度标签时直接传 "word"，所有 docx 都返回空串且不报错。
    #[test]
    fn library_kind_word_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_docx(dir.path(), "细则.docx", "生产智能化项目组技术栈统一管理细则，明确组件选型与审批流程。");
        let size = std::fs::metadata(&p).unwrap().len();
        let (text, _, _) = extract::extract(&p, "docx", "word", size);
        assert!(text.contains("技术栈统一管理细则"), "抽到的是空串：{text:?}");
        // 旧的词汇表也必须继续好用
        let (text2, _, _) = extract::extract(&p, "docx", "doc", size);
        assert_eq!(text, text2);
    }

    // 扫描件 / 坏文件：返回空串、不 panic、不拖时间
    #[test]
    fn broken_pdf_is_empty_and_fast() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("scan.pdf");
        std::fs::write(&p, b"%PDF-1.4\nthis is not really a pdf\n%%EOF").unwrap();
        let t0 = std::time::Instant::now();
        let (text, _, _) = extract::extract(&p, "pdf", "pdf", 40);
        let ms = t0.elapsed().as_millis();
        assert!(text.is_empty(), "读不出来就该是空的，不许编");
        assert!(ms < 2000, "坏文件不该拖住队列：{ms}ms");
    }
}

// ================================================================ 深度标签落库（P2 T2.5 / T2.7）

mod deep_tag_db {
    use crate::index as db;

    // T2.5：3 个标签 + 1 句 summary → deep，且 file_tags 3 行
    #[test]
    fn apply_deep_tag_writes_tags_summary_and_level() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let fid = db::insert_test_file(&conn, root.id, "安全检查通知.docx", db::Status::Labeled)
            .unwrap();
        // 先挂一个旧 topic 标签，验证重跑会清掉
        db::insert_test_topic_tag(&conn, fid, "旧标签").unwrap();

        let tags = vec!["安全检查".to_string(), "消防验收".to_string(), "年度检查".to_string()];
        db::apply_deep_tag(&mut conn, fid, &tags, Some("2026 年度安全检查通知，涉及生产部。"))
            .unwrap();

        let row = db::file_row(&conn, fid).unwrap().unwrap();
        assert_eq!(row.tag_level, "deep");
        assert_eq!(
            row.summary.as_deref(),
            Some("2026 年度安全检查通知，涉及生产部。")
        );
        let topic = db::topic_tags_for(&conn, &[fid]).unwrap();
        let got = topic.get(&fid).cloned().unwrap_or_default();
        assert_eq!(got.len(), 3, "三个标签都要落库：{got:?}");
        assert!(!got.contains(&"旧标签".to_string()), "重跑要清掉旧 topic");
    }

    // T2.5：空标签 + 空 summary → 仍标 deep，summary 是 NULL（「读过但没答案」）
    #[test]
    fn empty_result_marks_deep_but_keeps_summary_null() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let fid = db::insert_test_file(&conn, root.id, "扫描件001.pdf", db::Status::Labeled)
            .unwrap();
        db::apply_deep_tag(&mut conn, fid, &[], None).unwrap();

        let row = db::file_row(&conn, fid).unwrap().unwrap();
        assert_eq!(row.tag_level, "deep", "「读过但没答案」也是读过");
        assert!(row.summary.is_none(), "空结论必须写 NULL，不能写空串");
        assert!(db::topic_tags_for(&conn, &[fid]).unwrap().is_empty());
    }

    // T2.7：FileRow 的每一列都要读得回来（这条就是防列错位的红灯）
    #[test]
    fn file_row_roundtrips_every_column() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let fid = db::insert_test_file_kind(
            &conn,
            root.id,
            "配电箱巡检记录.docx",
            "docx",
            "word",
            db::Status::PendingConfirm,
        )
        .unwrap();
        db::set_note(&conn, fid, "配电房那边要的").unwrap();
        db::set_target_name(&conn, fid, Some("配电箱巡检记录_2026Q4.docx")).unwrap();
        db::apply_deep_tag(
            &mut conn,
            fid,
            &["配电巡检".to_string()],
            Some("2026 年四季度配电箱巡检记录，配电班组。"),
        )
        .unwrap();

        // 直接从池里读一份，走的是 pool_rows → FILE_COLS → row_to_file 那条真路径
        let pool = db::pool_rows(&conn).unwrap();
        let row = pool.iter().find(|r| r.id == fid).unwrap();
        assert_eq!(row.name, "配电箱巡检记录.docx");
        assert_eq!(row.ext, "docx");
        assert_eq!(row.kind, "word");
        assert_eq!(row.status, "pending_confirm");
        assert_eq!(row.rel_path, "C:/t/配电箱巡检记录.docx");
        assert_eq!(row.source_root_id, Some(root.id));
        assert_eq!(row.note.as_deref(), Some("配电房那边要的"));
        assert_eq!(
            row.target_name.as_deref(),
            Some("配电箱巡检记录_2026Q4.docx"),
            "target_name 与 note 挨着，最容易错位"
        );
        assert_eq!(row.summary.as_deref(), Some("2026 年四季度配电箱巡检记录，配电班组。"));
        assert_eq!(row.tag_level, "deep");
    }
}

// ================================================================ P2 M3 增量与监听

mod p2_m3 {
    use crate::index as db;
    use crate::index::Status;
    use crate::model::ScannedFile;
    use std::time::Duration;

    fn fresh() -> (tempfile::TempDir, rusqlite::Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open(&dir.path().join("t.db")).unwrap();
        (dir, conn)
    }

    fn mk(i: usize) -> ScannedFile {
        ScannedFile {
            name: format!("f{i}.docx"),
            path: format!("C:/scanroot/f{i}.docx"),
            ext: "docx".into(),
            kind: "word".into(),
            size: 100,
            mtime: 1_700_000_000_000 + i as i64,
        }
    }

    // T3.1：第二次扫同样的文件，一份都不许算 updated（否则每次启动都全量重算标签）
    #[test]
    fn second_scan_reports_unchanged_not_updated() {
        let (_d, mut conn) = fresh();
        let root = db::add_source_root(&conn, "C:/scanroot").unwrap();
        let all: Vec<_> = (0..10).map(mk).collect();

        let s1 = db::upsert_files(&mut conn, root.id, &all).unwrap();
        assert_eq!(
            (s1.inserted, s1.updated, s1.unchanged, s1.missing),
            (10, 0, 0, 0)
        );

        let s2 = db::upsert_files(&mut conn, root.id, &all).unwrap();
        assert_eq!(
            (s2.inserted, s2.updated, s2.unchanged, s2.missing),
            (0, 0, 10, 0),
            "未变的文件必须走 unchanged 快路径"
        );

        // 未变 = 连 deep 标签都不许被抹回 basic
        conn.execute("UPDATE files SET tag_level='deep'", []).unwrap();
        let s3 = db::upsert_files(&mut conn, root.id, &all).unwrap();
        assert_eq!(s3.updated, 0);
        let lvl: String = conn
            .query_row("SELECT tag_level FROM files LIMIT 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(lvl, "deep", "重扫不能把深度标签打回 basic");

        // 改一份 mtime → 只有它算 updated
        let mut one = all.clone();
        one[3].mtime += 1;
        let s4 = db::upsert_files(&mut conn, root.id, &one).unwrap();
        assert_eq!((s4.updated, s4.unchanged), (1, 9));
    }

    // T3.5：8 种状态 × 删除事件，只有 3 种允许被打成 missing
    //
    // 这一条扛的是「迁移把源文件搬走后，监听不能把刚迁移的整批判成丢失」。
    #[test]
    fn delete_event_only_touches_three_states() {
        let (_d, conn) = fresh();
        let root = db::add_source_root(&conn, "C:/t").unwrap();
        for st in Status::ALL {
            db::insert_test_file_kind(&conn, root.id, &format!("{}.docx", st.as_str()), "docx", "word", st)
                .unwrap();
        }

        for st in Status::ALL {
            let path = format!("C:/t/{}.docx", st.as_str());
            let marked = db::mark_missing_if_pending(&conn, &path).unwrap();
            let want = matches!(
                st,
                Status::Queued | Status::Labeled | Status::PendingConfirm
            );
            assert_eq!(
                marked,
                want,
                "{} 的删除事件处理错了：{}",
                st.as_str(),
                if want { "应该标 missing" } else { "不该动它" }
            );
        }

        // 已经 missing 的再收到一次删除事件 → 不重复计数
        let again = db::mark_missing_if_pending(&conn, "C:/t/queued.docx").unwrap();
        assert!(!again, "重复的删除事件不该返回 true");
    }

    // T3.6：主线程持锁时，监听侧退避一次就放弃（不 panic、不死等）
    #[test]
    fn watch_gives_up_when_db_busy() {
        let dir = tempfile::tempdir().unwrap();
        let m = std::sync::Mutex::new(db::open(&dir.path().join("t.db")).unwrap());
        let held = m.lock().unwrap();

        let (got, waited) = std::thread::scope(|s| {
            let h = s.spawn(|| {
                let t = std::time::Instant::now();
                let g = crate::lock_db_retry(&m, Duration::from_millis(200));
                (g.is_some(), t.elapsed())
            });
            // 主线程继续持锁，模拟「界面正在渲染」
            std::thread::sleep(Duration::from_millis(300));
            h.join().unwrap()
        });
        drop(held);

        assert!(!got, "抢不到锁时必须放弃这一批，不许死等界面");
        assert!(
            waited <= Duration::from_millis(1200),
            "退避要短：实际等了 {waited:?}"
        );

        // 锁放掉以后能正常拿到（丢的只是时机，不是数据）
        assert!(crate::lock_db_retry(&m, Duration::from_millis(200)).is_some());
    }
}

// ================================================================ P2 M4 待定区闭环

mod p2_m4 {
    use crate::error::BAD_TRANSITION;
    use crate::index as db;
    use crate::index::Status;

    fn fresh() -> (tempfile::TempDir, rusqlite::Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open(&dir.path().join("t.db")).unwrap();
        (dir, conn)
    }

    fn one(conn: &rusqlite::Connection, name: &str, st: Status) -> i64 {
        let root = db::add_source_root(conn, "C:/t").unwrap();
        db::insert_test_file_kind(conn, root.id, name, "docx", "word", st).unwrap()
    }

    fn status_of(conn: &rusqlite::Connection, id: i64) -> Status {
        db::get_file_status(conn, id).unwrap().unwrap()
    }

    // T4.2：三条合法转移
    #[test]
    fn parked_three_legal_moves() {
        // 捞回 → 回池
        let (_d, mut conn) = fresh();
        let id = one(&conn, "安检通知.docx", Status::Parked);
        db::unpark_file(&mut conn, id).unwrap();
        assert_eq!(status_of(&conn, id), Status::PendingConfirm);

        // 移出 → skipped（不是 missing：文件还在，只是不看它了）
        let (_d2, mut c2) = fresh();
        let id2 = one(&c2, "b.docx", Status::Parked);
        db::drop_file(&mut c2, id2).unwrap();
        assert_eq!(status_of(&c2, id2), Status::Skipped);

        // 直接指定 → confirmed + 一条 user_picked 的 assignment
        let (_d3, mut c3) = fresh();
        let id3 = one(&c3, "c.docx", Status::Parked);
        let dir = db::insert_dir(&c3, None, "安全环保", "user").unwrap();
        db::assign_parked_file(&mut c3, id3, dir.id, "user_picked").unwrap();
        assert_eq!(status_of(&c3, id3), Status::Confirmed);
        let n: i64 = c3
            .query_row("SELECT COUNT(*) FROM files WHERE status='confirmed'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "确认后待迁移要能看见它");
        let (target, origin, decision): (i64, String, String) = c3
            .query_row(
                "SELECT target_dir_id, origin, decision FROM assignments ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((target, origin.as_str(), decision.as_str()), (dir.id, "user_picked", "accepted"));
    }

    // T4.2：三条非法转移必须报 BAD_TRANSITION，不许悄悄写库
    #[test]
    fn parked_illegal_moves_are_rejected() {
        let (_d, mut conn) = fresh();
        let id = one(&conn, "安检通知.docx", Status::Parked);

        // parked → migrated：只有 confirmed 才能迁移
        let e = db::set_status(&conn, id, Status::Migrated).unwrap_err();
        assert_eq!(e.code, BAD_TRANSITION);

        // parked → queued：跳过 pending_confirm 回池会把「为什么没定」抹掉
        let e = db::set_status(&conn, id, Status::Queued).unwrap_err();
        assert_eq!(e.code, BAD_TRANSITION);

        // 对一份 queued 的文件调「捞回」也不行（queued → pending_confirm 非法）
        let id2 = one(&conn, "另一份.docx", Status::Queued);
        let e = db::unpark_file(&mut conn, id2).unwrap_err();
        assert_eq!(e.code, BAD_TRANSITION);
    }

    // T4.1：待定区列出的必须只有 parked，且候选是现算的
    #[test]
    fn list_only_parked_rows() {
        let (_d, conn) = fresh();
        let root = db::add_source_root(&conn, "C:/t").unwrap();
        for (name, st) in [
            ("a.docx", Status::Parked),
            ("b.docx", Status::Parked),
            ("c.docx", Status::Labeled),
            ("d.docx", Status::Confirmed),
        ] {
            db::insert_test_file_kind(&conn, root.id, name, "docx", "word", st).unwrap();
        }
        let rows = db::rows_by_status(&conn, "parked").unwrap();
        assert_eq!(rows.len(), 2, "只该列出 parked 的 2 份");
        assert!(rows.iter().all(|r| r.status == "parked"));
    }

    // T4.2：指定一个不存在的目录要报 DIR_INVALID，而不是写出一条指向空气的 assignment
    #[test]
    fn assign_to_missing_dir_is_rejected() {
        let (_d, mut conn) = fresh();
        let id = one(&conn, "安检通知.docx", Status::Parked);
        let e = db::assign_parked_file(&mut conn, id, 999, "user_picked").unwrap_err();
        assert_eq!(e.code, crate::error::DIR_INVALID);
        assert_eq!(status_of(&conn, id), Status::Parked, "失败不能改动状态");
    }
}

// ================================================================ 扫描判丢失的白名单

mod scan_missing_guard {
    use crate::index as db;
    use crate::index::Status;
    use crate::model::ScannedFile;

    /// 回归（2026-11-20 e2e 现场）：扫描判「本次没见到」时，
    /// `confirmed`（已确认未迁移）和 `parked`（待定区）绝不能被扫成 missing。
    /// P1 写的是黑名单 `NOT IN ('migrated','skipped','missing')`，于是这两类
    /// 只要源文件被人挪一下，就在下一次扫描里整批消失 —— 加了启动自动扫描后必现。
    #[test]
    fn scan_never_marks_confirmed_or_parked_as_missing() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scanroot").unwrap();

        for (name, st) in [
            ("a.docx", Status::Queued),
            ("b.docx", Status::Labeled),
            ("c.docx", Status::PendingConfirm),
            ("d.docx", Status::Confirmed),
            ("e.docx", Status::Parked),
            ("f.docx", Status::Skipped),
            ("g.docx", Status::Migrated),
        ] {
            db::insert_test_file_kind(&conn, root.id, name, "docx", "word", st).unwrap();
        }

        // 这一轮扫描一份都没见到（源文件全被挪走）
        let stats = db::upsert_files(&mut conn, root.id, &[]).unwrap();
        assert_eq!(stats.missing, 3, "只有 queued/labeled/pending_confirm 该变 missing");

        let after: Vec<(String, String)> = conn
            .prepare("SELECT name, status FROM files ORDER BY name")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let get = |n: &str| after.iter().find(|(x, _)| x == n).unwrap().1.clone();
        assert_eq!(get("a.docx"), "missing");
        assert_eq!(get("b.docx"), "missing");
        assert_eq!(get("c.docx"), "missing");
        assert_eq!(get("d.docx"), "confirmed", "已确认未迁移不能被扫成丢失");
        assert_eq!(get("e.docx"), "parked", "待定区的文件不能被扫成丢失");
        assert_eq!(get("f.docx"), "skipped");
        assert_eq!(get("g.docx"), "migrated");
    }
}

mod watch_upsert_scope {
    use crate::index as db;
    use crate::index::Status;
    use crate::model::ScannedFile;

    fn mk(name: &str) -> ScannedFile {
        ScannedFile {
            name: name.into(),
            path: format!("C:/t/{name}"), // 必须与 insert_test_file_kind 的路径一致
            ext: "docx".into(),
            kind: "word".into(),
            size: 100,
            mtime: 1_700_000_000_000,
        }
    }

    /// 回归（2026-11-20 e2e 现场）：监听批次一次只送 1~N 份，
    /// 若走普通 upsert 的「本轮未见即丢失」口径，会把库里**其余全部**
    /// labeled / pending_confirm 打成 missing（实测丢 1 份新文件 → 3 份过期）。
    #[test]
    fn watched_upsert_never_touches_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/t").unwrap();
        for (name, st) in [
            ("a.docx", Status::Labeled),
            ("b.docx", Status::PendingConfirm),
            ("c.docx", Status::Queued),
        ] {
            db::insert_test_file_kind(&conn, root.id, name, "docx", "word", st).unwrap();
        }
        // 全量扫描一遍，把 last_seen_at 拉到同一条线上
        let all = vec![mk("a.docx"), mk("b.docx"), mk("c.docx")];
        db::upsert_files(&mut conn, root.id, &all).unwrap();

        let stats = db::upsert_files_watched(&mut conn, root.id, &[mk("new.docx")]).unwrap();
        assert_eq!(stats.inserted, 1);
        assert_eq!(stats.missing, 0, "监听批次不许引发全局 missing 判定");

        let after: Vec<(String, String)> = conn
            .prepare("SELECT name, status FROM files ORDER BY name")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let get = |n: &str| {
            after
                .iter()
                .find(|(x, _)| x == n)
                .map(|(_, st)| st.clone())
                .unwrap_or_else(|| panic!("没有这一行：{n}（实际 {after:?}）"))
        };
        assert_eq!(get("a.docx"), "labeled", "全程只该有这一份 labeled");
        assert_eq!(get("b.docx"), "pending_confirm");
        assert_eq!(get("c.docx"), "queued");
        assert_eq!(get("new.docx"), "queued");
    }
}

// ================================================================ P3 M1 标签：看得见、改得动

mod p3_m1 {
    use crate::index as db;
    use rusqlite::params;

    /// 任意 kind 的测试标签（type/time/keyword 基础标签没有现成 helper）
    fn insert_tag_of_kind(conn: &rusqlite::Connection, file_id: i64, name: &str, kind: &str) -> i64 {
        conn.execute(
            "INSERT OR IGNORE INTO tags(name, kind) VALUES(?1, ?2)",
            params![name, kind],
        )
        .unwrap();
        let tid: i64 = conn
            .query_row("SELECT id FROM tags WHERE name=?1 AND kind=?2", params![name, kind], |r| r.get(0))
            .unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO file_tags(file_id, tag_id, confidence) VALUES(?1,?2,0.9)",
            params![file_id, tid],
        )
        .unwrap();
        tid
    }

    // T1.1 ①：3 份文件 7 个标签跨 4 种 kind，各自数量与顺序正确
    // 顺序 = type → time → topic → keyword → user，同类按 name
    #[test]
    fn attach_tags_returns_count_and_order_across_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let a = db::insert_test_file(&conn, root.id, "甲.docx", db::Status::Labeled).unwrap();
        let b = db::insert_test_file(&conn, root.id, "乙.xlsx", db::Status::Labeled).unwrap();
        let c = db::insert_test_file(&conn, root.id, "丙.pdf", db::Status::Labeled).unwrap();

        insert_tag_of_kind(&conn, a, "word", "type");
        insert_tag_of_kind(&conn, a, "2026", "time");
        insert_tag_of_kind(&conn, a, "安全检查", "topic");
        insert_tag_of_kind(&conn, a, "年度", "topic");
        insert_tag_of_kind(&conn, a, "生产", "keyword");
        insert_tag_of_kind(&conn, a, "重点跟进", "user");
        insert_tag_of_kind(&conn, b, "excel", "type");
        insert_tag_of_kind(&conn, b, "费率", "topic");

        let rows = db::file_rows(&conn, &[a, b, c]).unwrap();
        let ra = rows.iter().find(|r| r.id == a).unwrap();
        let rb = rows.iter().find(|r| r.id == b).unwrap();
        let rc = rows.iter().find(|r| r.id == c).unwrap();

        // 甲：type(1) + time(1) + topic(2) + keyword(1) + user(1) = 6
        let a_kinds: Vec<&str> = ra.tags.iter().map(|t| t.kind.as_str()).collect();
        assert_eq!(ra.tags.len(), 6, "甲应有 6 个标签：{:?}", ra.tags);
        assert_eq!(
            a_kinds,
            vec!["type", "time", "topic", "topic", "keyword", "user"],
            "顺序必须是 type→time→topic→keyword→user：{:?}",
            a_kinds
        );
        // 同类按 name（按码点序）：「安全检查」<「年度」
        assert_eq!(ra.tags[2].name, "安全检查");
        assert_eq!(ra.tags[3].name, "年度");

        assert_eq!(rb.tags.len(), 2);
        assert!(rc.tags.is_empty(), "丙没标签，不该 panic 也不该有占位");
    }

    // T1.1 ②：无标签文件 tags 为空、不 panic（并覆盖 file_row 单份出口）
    #[test]
    fn file_without_tags_is_empty_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let fid = db::insert_test_file(&conn, root.id, "裸文件.docx", db::Status::Queued).unwrap();
        let one = db::file_row(&conn, fid).unwrap().unwrap();
        assert!(one.tags.is_empty());
        let many = db::file_rows(&conn, &[fid, -1]).unwrap();
        assert!(many.iter().all(|r| r.tags.is_empty()));
    }

    // T1.1 ③：tags_for 空入参不发 SQL（trace 只收 fn 指针，用线程局部计数
    // —— 测试是并行跑的，静态量会被别的线程的 SELECT 污染）
    thread_local! {
        static TRACE_SELECTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    fn trace_select(sql: &str) {
        if sql.contains("SELECT") {
            TRACE_SELECTS.with(|c| c.set(c.get() + 1));
        }
    }
    #[test]
    fn tags_for_empty_ids_sends_no_sql() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        TRACE_SELECTS.with(|c| c.set(0));
        conn.trace(Some(trace_select));
        let out = db::tags_for(&conn, &[]).unwrap();
        assert!(out.is_empty());
        assert_eq!(
            TRACE_SELECTS.with(std::cell::Cell::get),
            0,
            "空入参一条 SELECT 都不许发"
        );
    }

    // T1.1 ④：同一份文件的标签顺序稳定（跑两次逐字相等）
    #[test]
    fn tag_order_is_stable_across_calls() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let fid = db::insert_test_file(&conn, root.id, "稳定.docx", db::Status::Labeled).unwrap();
        insert_tag_of_kind(&conn, fid, "用户标注", "user");
        insert_tag_of_kind(&conn, fid, "关键词甲", "keyword");
        insert_tag_of_kind(&conn, fid, "主题乙", "topic");
        insert_tag_of_kind(&conn, fid, "主题甲", "topic");
        insert_tag_of_kind(&conn, fid, "pdf", "type");

        let dump = |conn: &rusqlite::Connection| -> Vec<String> {
            db::file_row(conn, fid)
                .unwrap()
                .unwrap()
                .tags
                .iter()
                .map(|t| format!("{}|{}|{}", t.kind, t.name, t.id))
                .collect()
        };
        let first = dump(&conn);
        let second = dump(&conn);
        assert_eq!(first, second, "两次读取顺序必须逐字相等");
        assert_eq!(first.len(), 5);
    }

    // T1.1 ⑤：rows_by_status('parked') 也带标签（五个出口里最容易被漏的一个）
    #[test]
    fn rows_by_status_parked_carries_tags() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let fid = db::insert_test_file(&conn, root.id, "搁置件.docx", db::Status::Parked).unwrap();
        insert_tag_of_kind(&conn, fid, "待定话题", "topic");

        let rows = db::rows_by_status(&conn, "parked").unwrap();
        let row = rows.iter().find(|r| r.id == fid).expect("parked 行要在列表里");
        assert_eq!(row.tags.len(), 1);
        assert_eq!(row.tags[0].name, "待定话题");
    }

    // T1.3 ①：新增落 kind='user'
    #[test]
    fn add_tag_lands_as_user_kind() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let fid = db::insert_test_file(&conn, root.id, "手动.docx", db::Status::Labeled).unwrap();
        let t = db::add_tag(&mut conn, fid, "领导交办").unwrap();
        assert_eq!(t.kind, "user");
        assert_eq!(t.name, "领导交办");
        let row = db::file_row(&conn, fid).unwrap().unwrap();
        assert!(row.tag_locked, "加标签即表过态，必须锁");
        assert_eq!(row.tags.len(), 1);
        assert_eq!(row.tags[0].kind, "user");
    }

    // T1.3 ②：同名重复不重复插（tags 表只有一行）
    #[test]
    fn add_tag_same_name_no_duplicate_row() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let fid = db::insert_test_file(&conn, root.id, "重复.docx", db::Status::Labeled).unwrap();
        let t1 = db::add_tag(&mut conn, fid, "同名").unwrap();
        let t2 = db::add_tag(&mut conn, fid, "同名").unwrap();
        assert_eq!(t1.id, t2.id);
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM tags WHERE name='同名' AND kind='user'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "tags 表只许一行");
        // 但同一文件重复加，file_tags 也不该翻倍
        let ft: i64 = conn
            .query_row("SELECT COUNT(*) FROM file_tags WHERE file_id=?1", [fid], |r| r.get(0))
            .unwrap();
        assert_eq!(ft, 1);
    }

    // T1.3 ③：超 20 字截断（不是报错）
    #[test]
    fn add_tag_truncates_over_20_chars() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let fid = db::insert_test_file(&conn, root.id, "长名.docx", db::Status::Labeled).unwrap();
        let raw = "这是一个特别特别长的标签名远远超过二十个字符的长度限制内容继续往后写";
        let t = db::add_tag(&mut conn, fid, raw).unwrap();
        assert_eq!(t.name.chars().count(), 20, "截到 20 字，不是报错");
    }

    // T1.3 ④：空名 BAD_INPUT（含只带了 # 的）
    #[test]
    fn add_tag_empty_is_bad_input() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let fid = db::insert_test_file(&conn, root.id, "空名.docx", db::Status::Labeled).unwrap();
        for bad in ["", "   ", "###", "  ##  "] {
            let err = db::add_tag(&mut conn, fid, bad).unwrap_err();
            assert_eq!(err.code, "BAD_INPUT", "输入 {bad:?} 应拒空");
        }
    }

    // T1.3 ⑤：删 type 报 TAG_LOCKED
    #[test]
    fn remove_tag_type_is_locked() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let fid = db::insert_test_file(&conn, root.id, "基础.docx", db::Status::Labeled).unwrap();
        let tid = insert_tag_of_kind(&conn, fid, "word", "type");
        let err = db::remove_tag(&mut conn, fid, tid).unwrap_err();
        assert_eq!(err.code, "TAG_LOCKED");
        // time 同理
        let tid2 = insert_tag_of_kind(&conn, fid, "2026", "time");
        assert_eq!(db::remove_tag(&mut conn, fid, tid2).unwrap_err().code, "TAG_LOCKED");
    }

    // T1.3 ⑥：删 topic 到空后 tag_level 仍是 deep（「读过但没结论」≠「没读过」）
    #[test]
    fn remove_all_tags_keeps_tag_level_deep() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let fid = db::insert_test_file(&conn, root.id, "删空.docx", db::Status::Labeled).unwrap();
        db::apply_deep_tag(&mut conn, fid, &["某话题".into()], Some("读过的一句话。")).unwrap();
        let row = db::file_row(&conn, fid).unwrap().unwrap();
        let tid = row.tags[0].id;
        db::remove_tag(&mut conn, fid, tid).unwrap();
        let row = db::file_row(&conn, fid).unwrap().unwrap();
        assert!(row.tags.is_empty());
        assert_eq!(row.tag_level, "deep", "删到不剩也是读过");
        assert_eq!(row.summary.as_deref(), Some("读过的一句话。"), "summary 不能跟着没");
    }

    // T1.4 ①：手加 user 标签 → 跑 apply_deep_tag → user 还在、模型标签没进来、summary 已更新
    #[test]
    fn locked_row_keeps_user_tag_and_skips_model_tags() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let fid = db::insert_test_file(&conn, root.id, "锁定.docx", db::Status::Labeled).unwrap();
        db::add_tag(&mut conn, fid, "我自己的标签").unwrap();

        let skipped = db::apply_deep_tag(
            &mut conn,
            fid,
            &["模型话题".to_string()],
            Some("模型的新结论。"),
        )
        .unwrap();
        assert!(skipped, "锁定行必须报告跳过了 tags");

        let row = db::file_row(&conn, fid).unwrap().unwrap();
        assert_eq!(row.tags.len(), 1, "模型标签一个都不许写进来");
        assert_eq!(row.tags[0].name, "我自己的标签");
        assert_eq!(row.summary.as_deref(), Some("模型的新结论。"), "summary 照常更新");
        assert_eq!(row.tag_level, "deep");
    }

    // T1.4 ②：未锁定行行为不变（回归）
    #[test]
    fn unlocked_row_behaves_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let fid = db::insert_test_file(&conn, root.id, "没锁.docx", db::Status::Labeled).unwrap();
        let skipped = db::apply_deep_tag(&mut conn, fid, &["新话题".into()], Some("结论。")).unwrap();
        assert!(!skipped);
        let row = db::file_row(&conn, fid).unwrap().unwrap();
        assert_eq!(row.tags.len(), 1);
        assert_eq!(row.tags[0].name, "新话题");
        assert!(!row.tag_locked);
    }

    // T1.4 ③：锁定行重复跑两次结果相同（幂等）
    #[test]
    fn locked_row_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let fid = db::insert_test_file(&conn, root.id, "幂等.docx", db::Status::Labeled).unwrap();
        db::add_tag(&mut conn, fid, "人写的").unwrap();
        for _ in 0..2 {
            db::apply_deep_tag(&mut conn, fid, &["轮次话题".into()], Some("固定结论。")).unwrap();
        }
        let row = db::file_row(&conn, fid).unwrap().unwrap();
        assert_eq!(row.tags.len(), 1, "两轮都只许有人写的那个标签");
        assert_eq!(row.summary.as_deref(), Some("固定结论。"));
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM tags WHERE name='轮次话题'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "连 tags 表都不该被写进模型词");
    }

    // T1.5 ①：delete_tag 级联清 file_tags
    #[test]
    fn delete_tag_cascades_file_tags() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let f1 = db::insert_test_file(&conn, root.id, "文件一.docx", db::Status::Labeled).unwrap();
        let f2 = db::insert_test_file(&conn, root.id, "文件二.docx", db::Status::Labeled).unwrap();
        db::apply_deep_tag(&mut conn, f1, &["共享话题".into()], None).unwrap();
        db::apply_deep_tag(&mut conn, f2, &["共享话题".into(), "另一个".into()], None).unwrap();

        let tags = db::list_tags(&conn).unwrap();
        let target = tags.iter().find(|t| t.name == "共享话题").expect("词表里要有共享话题");
        assert_eq!(target.file_count, 2, "词表要数出挂了几份文件");

        let removed = db::delete_tag(&mut conn, target.id).unwrap();
        assert_eq!(removed, 2, "删词要报告清掉的份数");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM file_tags WHERE tag_id=?1", [target.id], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "file_tags 必须级联清空");
        let n2: i64 = conn
            .query_row("SELECT COUNT(*) FROM tags WHERE id=?1", [target.id], |r| r.get(0))
            .unwrap();
        assert_eq!(n2, 0, "tags 行本身也要删");
    }

    // T1.5 ②：rename_tag 撞名则合并（UNIQUE(name,kind) 不冲突）
    #[test]
    fn rename_tag_to_existing_name_merges() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let f1 = db::insert_test_file(&conn, root.id, "文件一.docx", db::Status::Labeled).unwrap();
        let f2 = db::insert_test_file(&conn, root.id, "文件二.docx", db::Status::Labeled).unwrap();
        db::apply_deep_tag(&mut conn, f1, &["数字化转型".into()], None).unwrap();
        db::apply_deep_tag(&mut conn, f2, &["数智化转型".into()], None).unwrap();

        let tags = db::list_tags(&conn).unwrap();
        let src = tags.iter().find(|t| t.name == "数智化转型").unwrap();
        let merged_id = db::rename_tag(&mut conn, src.id, "数字化转型").unwrap();
        let dst = tags.iter().find(|t| t.name == "数字化转型").unwrap();
        assert_eq!(merged_id, dst.id, "撞名要归并到已有行");

        // 两个文件现在都挂在目标标签上，源标签消失
        let both: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT file_id) FROM file_tags ft JOIN tags t ON t.id=ft.tag_id WHERE t.name='数字化转型'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(both, 2, "合并后两个文件都要在");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM tags WHERE name='数智化转型'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "源标签行要消失");
        let dup: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM (SELECT name FROM tags WHERE kind='topic' GROUP BY name, kind HAVING COUNT(*)>1)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(dup, 0, "UNIQUE(name,kind) 不许出现重复");
    }
}

// ================================================================ P3 M2 浏览页地基

mod p3_m2 {
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

    /// 与 migration_flow::seed 同构：真实目录 + 3 份已确认文件，都归到同一个目录
    fn seed(extra: &[(&str, &[u8])]) -> Seeded {
        let src = tempfile::tempdir().unwrap();
        let arc = tempfile::tempdir().unwrap();
        let (_d, mut conn) = fresh_db();
        let root = db::add_source_root(&conn, &src.path().to_string_lossy()).unwrap();

        let mut names: Vec<(String, Vec<u8>)> = vec![
            ("甲.docx".into(), b"aaa".to_vec()),
            ("乙.xlsx".into(), b"bbbb".to_vec()),
            ("丙.pdf".into(), b"ccccc".to_vec()),
        ];
        for (n, b) in extra {
            names.push((n.to_string(), b.to_vec()));
        }
        let mut scanned = Vec::new();
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

    /// 迁移 3 份并返回（迁移完的 seeded）
    fn migrated(extra: &[(&str, &[u8])]) -> Seeded {
        let mut s = seed(extra);
        mig::apply(&mut s.conn, &s.arc_path, &[], &mut |_, _, _, _| {}).unwrap();
        s
    }

    // T2.1 ①：迁移 1 批 → archived_path == dst、archive_dir_id 正确
    #[test]
    fn apply_writes_archived_path_and_dir_id() {
        let s = migrated(&[]);
        for id in &s.ids {
            let row = db::file_row(&s.conn, *id).unwrap().unwrap();
            assert_eq!(row.status, "migrated");
            let dst = row.archived_path.as_deref().expect("archived_path 必须落库");
            assert!(
                Path::new(dst).starts_with(&s.arc_path),
                "归档路径要在归档根下：{dst}"
            );
            assert!(
                Path::new(dst).is_file(),
                "归档路径必须真实存在（这就是浏览页能打开文件的前提）：{dst}"
            );
            assert_eq!(row.archive_dir_id, Some(s.dir_id));
            // live_path 已归档走归档路径
            assert_eq!(row.live_path(), dst);
        }
    }

    // T2.1 ②：live_path() 对未迁移文件返回源路径
    #[test]
    fn live_path_falls_back_to_source() {
        let s = seed(&[]);
        for id in &s.ids {
            let row = db::file_row(&s.conn, *id).unwrap().unwrap();
            assert!(row.archived_path.is_none());
            assert_eq!(row.live_path(), row.abs_path, "没迁移就用源路径");
        }
    }

    // T2.1 ③：没参与迁移的文件两列为 NULL（迁移失败路径在 migration_flow 已覆盖状态机，
    // 这里锁「未迁移者不带归档路径」这条不变量）
    #[test]
    fn unmigrated_files_have_null_archive_columns() {
        let s = migrated(&[]);
        // 再放一份新扫描的 queued 文件，它没参与迁移
        let root = db::list_source_roots(&s.conn).unwrap().remove(0);
        let queued = db::insert_test_file(&s.conn, root.id, "没确认.docx", db::Status::Queued)
            .unwrap();
        let row = db::file_row(&s.conn, queued).unwrap().unwrap();
        assert_ne!(row.status, "migrated");
        assert!(row.archived_path.is_none(), "未迁移不许有归档路径");
        assert!(row.archive_dir_id.is_none());
    }

    // T2.1 ④：迁移后 live_path 指向的文件就是原内容（预览/打开由此可用）
    #[test]
    fn migrated_live_path_reads_back_original_bytes() {
        let s = migrated(&[]);
        let row = db::file_row(&s.conn, s.ids[0]).unwrap().unwrap();
        let bytes = std::fs::read(row.live_path()).unwrap();
        assert_eq!(bytes, b"aaa", "归档路径读回来的就是原文件内容");
    }

    // T2.1 ⑤：撤销迁移 → 两列清空（不然浏览页指向隔离区里的旧路径）
    #[test]
    fn undo_clears_archive_columns() {
        let mut s = migrated(&[]);
        let mid = db::latest_migration(&s.conn).unwrap().unwrap().id;
        let rep = mig::undo(&mut s.conn, &s.arc_path, mid, &mut |_, _, _, _| {}).unwrap();
        assert_eq!(rep.restored, 3, "{:?}", rep.failed);
        for id in &s.ids {
            let row = db::file_row(&s.conn, *id).unwrap().unwrap();
            assert_eq!(row.status, "confirmed");
            assert!(row.archived_path.is_none(), "撤销后归档路径必须清掉");
            assert!(row.archive_dir_id.is_none());
            assert_eq!(row.live_path(), row.abs_path);
        }
    }

    /// 直接往库里造一条「老数据」：已迁移但 archived_path 为空，带一条 ok 的 migration_items
    fn plant_old_row(conn: &rusqlite::Connection, file_id: i64, dst: &str, dir_deleted: bool) {
        conn.execute("UPDATE files SET status='migrated' WHERE id=?1", rusqlite::params![file_id])
            .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO migrations(id, status, file_count, bytes) VALUES(1, 'done', 1, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO migration_items(migration_id, file_id, src_path, dst_path, result)
             VALUES(1, ?1, 'C:/old/src', ?2, 'ok')",
            rusqlite::params![file_id, dst],
        )
        .unwrap();
        if !dir_deleted {
            // 让归档目录真实存在
            conn.execute(
                "INSERT OR IGNORE INTO archive_dirs(id, name, rel_path, depth, origin, created_at)
                 VALUES(9, '旧目录', '旧目录', 1, 'user', 0)",
                [],
            )
            .unwrap();
        }
    }

    // T2.2 ①：回填 2 份
    #[test]
    fn backfill_fills_two_rows() {
        let (_d, mut conn) = fresh_db();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let f1 = db::insert_test_file(&conn, root.id, "旧一.docx", db::Status::Queued).unwrap();
        let f2 = db::insert_test_file(&conn, root.id, "旧二.docx", db::Status::Queued).unwrap();
        plant_old_row(&conn, f1, "旧目录/旧一.docx", false);
        plant_old_row(&conn, f2, "旧目录/旧二.docx", false);

        let n = db::backfill_archived_paths(&mut conn).unwrap();
        assert_eq!(n, 2);
        let r1 = db::file_row(&conn, f1).unwrap().unwrap();
        assert_eq!(r1.archived_path.as_deref(), Some("旧目录/旧一.docx"));
        assert_eq!(r1.archive_dir_id, Some(9), "按最长前缀反推出目录 id");
        let r2 = db::file_row(&conn, f2).unwrap().unwrap();
        assert_eq!(r2.archive_dir_id, Some(9));
    }

    // T2.2 ②：目录已删的那份留 NULL（浏览页归「未归类」，不隐藏）
    #[test]
    fn backfill_leaves_null_when_dir_gone() {
        let (_d, mut conn) = fresh_db();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let f = db::insert_test_file(&conn, root.id, "孤儿.docx", db::Status::Queued).unwrap();
        plant_old_row(&conn, f, "已删目录/孤儿.docx", true);

        let n = db::backfill_archived_paths(&mut conn).unwrap();
        assert_eq!(n, 1, "回填还是要跑（至少把 archived_path 写上）");
        let row = db::file_row(&conn, f).unwrap().unwrap();
        assert_eq!(row.archived_path.as_deref(), Some("已删目录/孤儿.docx"));
        assert!(row.archive_dir_id.is_none(), "目录没了就归未归类");
    }

    // T2.2 ③：重复执行不改已有值（幂等），且第二次返回 0
    #[test]
    fn backfill_is_idempotent() {
        let (_d, mut conn) = fresh_db();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let f = db::insert_test_file(&conn, root.id, "幂等.docx", db::Status::Queued).unwrap();
        plant_old_row(&conn, f, "旧目录/幂等.docx", false);

        assert_eq!(db::backfill_archived_paths(&mut conn).unwrap(), 1);
        // 改个值再跑：不该被覆盖
        conn.execute(
            "UPDATE files SET archived_path='手工改过的路径' WHERE id=?1",
            [f],
        )
        .unwrap();
        assert_eq!(db::backfill_archived_paths(&mut conn).unwrap(), 0, "第二次必须 0 行");
        let row = db::file_row(&conn, f).unwrap().unwrap();
        assert_eq!(row.archived_path.as_deref(), Some("手工改过的路径"));
    }

    // T2.3 ①：直属 / 含子目录两种口径
    #[test]
    fn browse_dir_direct_and_subtree() {
        let mut s = migrated(&[]);
        // 再建一个子目录 + 把一份文件挪过去
        let sub = db::insert_dir(&s.conn, Some(s.dir_id), "消防", "user").unwrap();
        s.conn
            .execute(
                "UPDATE files SET archive_dir_id=?1 WHERE id=?2",
                rusqlite::params![sub.id, s.ids[0]],
            )
            .unwrap();

        let direct = db::browse_dir(&s.conn, Some(s.dir_id), false).unwrap();
        assert_eq!(direct.len(), 2, "直属不含子目录里的那份");
        let tree = db::browse_dir(&s.conn, Some(s.dir_id), true).unwrap();
        assert_eq!(tree.len(), 3, "含子目录是全量");
        let only_sub = db::browse_dir(&s.conn, Some(sub.id), false).unwrap();
        assert_eq!(only_sub.len(), 1);
        assert_eq!(only_sub[0].id, s.ids[0]);
    }

    // T2.3 ②：空目录返回空数组；不存在的目录报 NOT_FOUND
    #[test]
    fn browse_dir_empty_and_missing() {
        let s = migrated(&[]);
        let empty = db::insert_dir(&s.conn, None, "空目录", "user").unwrap();
        assert!(db::browse_dir(&s.conn, Some(empty.id), false).unwrap().is_empty());
        let err = db::browse_dir(&s.conn, Some(-42), false).unwrap_err();
        assert_eq!(err.code, "NOT_FOUND");
    }

    // T2.3 ③：未归类分组（dir_id=None）只含 archive_dir_id 为空的已迁移行
    #[test]
    fn browse_dir_null_group_only_uncategorized() {
        let s = migrated(&[]);
        // 把一份文件的 archive_dir_id 抹掉（模拟目录被删过的老数据）
        s.conn
            .execute(
                "UPDATE files SET archive_dir_id=NULL WHERE id=?1",
                [s.ids[0]],
            )
            .unwrap();
        let un = db::browse_dir(&s.conn, None, false).unwrap();
        assert_eq!(un.len(), 1);
        assert_eq!(un[0].id, s.ids[0]);
        // 未归类也要带标签（同一套 attach_tags）
        let tagged = db::browse_dir(&s.conn, Some(s.dir_id), false).unwrap();
        assert!(tagged.iter().all(|r| r.tags.is_empty() || !r.tags.is_empty()));
    }

    // T2.3 ④：500 份同目录，browse_dir ≤ 100ms
    #[test]
    fn browse_dir_500_files_under_100ms() {
        let mut s = migrated(&[]);
        let mut scanned = Vec::new();
        for i in 0..500 {
            scanned.push(ScannedFile {
                name: format!("批量{i:03}.docx"),
                path: format!("C:/t/批量{i:03}.docx"),
                ext: "docx".into(),
                kind: "word".into(),
                size: 100,
                mtime: 1_700_000_000_000 + i,
            });
        }
        let root = db::list_source_roots(&s.conn).unwrap().remove(0);
        db::upsert_files(&mut s.conn, root.id, &scanned).unwrap();
        // 直接 SQL 摆成已迁移 + 归到该目录（走迁移流程太慢也没必要）
        s.conn
            .execute(
                "UPDATE files SET status='migrated', archive_dir_id=?1 WHERE status='queued'",
                [s.dir_id],
            )
            .unwrap();

        let t0 = std::time::Instant::now();
        let rows = db::browse_dir(&s.conn, Some(s.dir_id), false).unwrap();
        let ms = t0.elapsed().as_millis();
        assert_eq!(rows.len(), 503);
        assert!(ms <= 100, "500 份同目录要 ≤100ms，实测 {ms}ms");
    }

    // T2.4 后端：browse_tree 的份数是「直属已迁移」
    #[test]
    fn browse_tree_counts_direct_migrated() {
        let mut s = migrated(&[]);
        let sub = db::insert_dir(&s.conn, Some(s.dir_id), "消防", "user").unwrap();
        s.conn
            .execute(
                "UPDATE files SET archive_dir_id=?1 WHERE id=?2",
                rusqlite::params![sub.id, s.ids[0]],
            )
            .unwrap();

        let tree = db::browse_tree(&s.conn).unwrap();
        let parent = tree.iter().find(|n| n.id == s.dir_id).unwrap();
        let child = parent.children.iter().find(|n| n.id == sub.id).unwrap();
        assert_eq!(parent.file_count, 2, "直属份数不含子目录");
        assert_eq!(child.file_count, 1);
    }
}

// ================================================================ P3 M3 检索查询层

mod p3_m3 {
    use crate::index as db;
    use crate::model::ScannedFile;
    use crate::tests::index::fresh_db;

    /// 造一批文件 + 标签，覆盖中英、不同类型、不同时间、多标签
    fn seeded() -> (tempfile::TempDir, rusqlite::Connection) {
        let (dir, mut conn) = fresh_db();
        let root = db::add_source_root(&conn, "C:/scan").unwrap();
        let mut scanned = Vec::new();
        let spec: Vec<(&str, &str, i64)> = vec![
            ("安全检查通知.docx", "word", 1_700_000_000_000),
            ("2026_报表.xlsx", "excel", 1_700_000_500_000),
            ("meeting-notes.pdf", "pdf", 1_700_001_000_000),
            ("照片1.png", "image", 1_700_001_500_000),
            ("整改报告.pdf", "pdf", 1_699_999_000_000),
        ];
        for (name, kind, mtime) in &spec {
            scanned.push(ScannedFile {
                name: name.to_string(),
                path: format!("C:/t/{name}"),
                ext: name.rsplit('.').next().unwrap_or("").to_string(),
                kind: kind.to_string(),
                size: 100,
                mtime: *mtime,
            });
        }
        db::upsert_files(&mut conn, root.id, &scanned).unwrap();
        // upsert 借用，重取 ids
        let ids: Vec<i64> = {
            let mut stmt = conn.prepare("SELECT id FROM files ORDER BY id").unwrap();
            let it = stmt.query_map([], |r| r.get(0)).unwrap();
            it.collect::<Result<Vec<i64>, _>>().unwrap()
        };
        // 标签：文件1 = 安全+检查；文件2 = 安全；文件3 = 会议；其余无
        let ft = |fid: i64, tag: &str| {
            conn.execute(
                "INSERT OR IGNORE INTO tags(name, kind) VALUES(?1, 'topic')",
                rusqlite::params![tag],
            )
            .unwrap();
            let tid: i64 = conn
                .query_row("SELECT id FROM tags WHERE name=?1 AND kind='topic'", rusqlite::params![tag], |r| r.get(0))
                .unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO file_tags(file_id, tag_id, confidence) VALUES(?1,?2,0.8)",
                rusqlite::params![fid, tid],
            )
            .unwrap();
        };
        ft(ids[0], "安全");
        ft(ids[0], "检查");
        ft(ids[1], "安全");
        ft(ids[2], "会议");
        (dir, conn)
    }

    fn req() -> db::SearchReq {
        db::SearchReq {
            q: String::new(),
            tags: vec![],
            tag_and: true,
            kinds: vec![],
            from: None,
            to: None,
            scope: db::Scope::All,
            dir_id: None,
            include_subdirs: false,
            sort: db::Sort::MtimeDesc,
            limit: 50,
            offset: 0,
        }
    }

    // T3.1 ①：纯关键词（中英混）
    #[test]
    fn keyword_matches_name_and_relpath() {
        let (_keep, conn) = seeded();
        let mut r = req();
        r.q = "安全检查".into();
        let (items, total) = db::search_files(&conn, &r).unwrap();
        assert_eq!(total, 1);
        assert_eq!(items[0].name, "安全检查通知.docx");

        r.q = "meeting".into();
        let (_, total) = db::search_files(&conn, &r).unwrap();
        assert_eq!(total, 1, "英文关键词也要命中");
    }

    // T3.1 ②：标签 AND 两个 = 同时具备
    #[test]
    fn tags_and_requires_all() {
        let (_keep, conn) = seeded();
        let tids: Vec<i64> = {
            let mut stmt = conn.prepare("SELECT id FROM tags WHERE name IN ('安全','检查') ORDER BY name").unwrap();
            let it = stmt.query_map([], |r| r.get(0)).unwrap();
            it.collect::<Result<Vec<i64>, _>>().unwrap()
        };
        let mut r = req();
        r.tags = tids;
        r.tag_and = true;
        let (items, total) = db::search_files(&conn, &r).unwrap();
        assert_eq!(total, 1, "同时带「安全+检查」的只有文件1");
        assert_eq!(items[0].name, "安全检查通知.docx");
    }

    // T3.1 ③：标签 OR 两个 = 任一（结果并集）
    #[test]
    fn tags_or_is_union() {
        let (_keep, conn) = seeded();
        let tids: Vec<i64> = {
            let mut stmt = conn.prepare("SELECT id FROM tags WHERE name IN ('安全','会议') ORDER BY name").unwrap();
            let it = stmt.query_map([], |r| r.get(0)).unwrap();
            it.collect::<Result<Vec<i64>, _>>().unwrap()
        };
        let mut r = req();
        r.tags = tids;
        r.tag_and = false;
        let (_, total) = db::search_files(&conn, &r).unwrap();
        assert_eq!(total, 3, "安全(2份) + 会议(1份) = 3");
    }

    // T3.1 ④：类型 + 时间组合
    #[test]
    fn kinds_and_time_window() {
        let (_keep, conn) = seeded();
        let mut r = req();
        r.kinds = vec!["pdf".into()];
        r.from = Some(1_700_000_000_000);
        r.to = Some(1_700_002_000_000);
        let (items, total) = db::search_files(&conn, &r).unwrap();
        assert_eq!(total, 1, "两份 pdf 里只有 meeting-notes 在时间窗内");
        assert_eq!(items[0].name, "meeting-notes.pdf");
    }

    // T3.1 ⑤：三种 scope 各自正确
    #[test]
    fn scopes_partition() {
        let (_keep, conn) = seeded();
        // 全部置为池内
        conn.execute("UPDATE files SET status='labeled'", []).unwrap();
        // 一份归档、一份搁置
        conn.execute("UPDATE files SET status='migrated' WHERE name='安全检查通知.docx'", []).unwrap();
        conn.execute("UPDATE files SET status='parked' WHERE name='照片1.png'", []).unwrap();

        for (scope, want) in [
            ("archive", 1usize),
            ("pool", 3usize),
            ("parked", 1usize),
            ("all", 5usize),
        ] {
            let mut r = req();
            r.scope = db::Scope::parse(scope);
            let (_, total) = db::search_files(&conn, &r).unwrap();
            assert_eq!(total, want as i64, "scope={scope}");
        }
        // skipped / missing 永不出现
        conn.execute("UPDATE files SET status='skipped' WHERE name='整改报告.pdf'", []).unwrap();
        let mut r = req();
        let (_, total) = db::search_files(&conn, &r).unwrap();
        assert_eq!(total, 4, "skipped 不出现在任何 scope");
    }

    // T3.1 ⑥：limit/offset 翻页不重不漏
    #[test]
    fn pagination_no_overlap_no_gap() {
        let (_keep, conn) = seeded();
        let mut r = req();
        r.limit = 2;
        let mut names = Vec::new();
        let mut total_all = 0i64;
        let mut offset = 0i64;
        loop {
            r.offset = offset;
            let (items, total) = db::search_files(&conn, &r).unwrap();
            total_all = total;
            if items.is_empty() {
                break;
            }
            names.extend(items.iter().map(|i| i.id));
            offset += items.len() as i64;
            if offset >= total {
                break;
            }
        }
        assert_eq!(total_all, 5);
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "翻页不许重复");
        assert_eq!(names.len() as i64, total_all, "翻页不许漏");
    }

    // T3.1 ⑦：`_` 与 `%` 被转义（搜 `_` 不该匹配所有文件）
    #[test]
    fn like_wildcards_are_escaped() {
        let (_keep, conn) = seeded();
        let mut r = req();
        r.q = "_".into();
        let (_, total) = db::search_files(&conn, &r).unwrap();
        assert_eq!(total, 1, "只有「2026_报表」字面含下划线");

        r.q = "%".into();
        let (_, total) = db::search_files(&conn, &r).unwrap();
        assert_eq!(total, 0, "字面 % 不命中任何文件");
    }

    // T3.1 ⑧：total 与 items 同源 + 结果带标签（attach_tags 出口）
    #[test]
    fn items_carry_tags_and_total_matches() {
        let (_keep, conn) = seeded();
        let mut r = req();
        r.q = "安全".into();
        let (items, total) = db::search_files(&conn, &r).unwrap();
        assert_eq!(items.len() as i64, total);
        let row = items.iter().find(|i| i.name == "安全检查通知.docx").unwrap();
        assert_eq!(row.tags.len(), 2, "检索结果也要带标签");
    }

    // T3.7 同款性能预检（正式闸门在 test/perf-search.py）：1000 份 + 关键词 ≤ 100ms
    #[test]
    fn search_1000_files_fast() {
        let (_keep, mut conn) = seeded();
        let root = db::list_source_roots(&conn).unwrap().remove(0);
        let mut scanned = Vec::new();
        for i in 0..1000 {
            scanned.push(ScannedFile {
                name: format!("批量材料{i:04}.docx"),
                path: format!("C:/t/批量材料{i:04}.docx"),
                ext: "docx".into(),
                kind: "word".into(),
                size: 100,
                mtime: 1_700_002_000_000 + i,
            });
        }
        db::upsert_files(&mut conn, root.id, &scanned).unwrap();
        let mut r = req();
        r.q = "批量材料00".into();
        let t0 = std::time::Instant::now();
        let (items, _) = db::search_files(&conn, &r).unwrap();
        let ms = t0.elapsed().as_millis();
        assert!(!items.is_empty());
        assert!(ms <= 100, "1000 份关键词检索要 ≤100ms，实测 {ms}ms");
    }
}

// ================================================================ P3 M4：待定区「让 AI 说明」（T4.5）

mod p3_m4 {
    use crate::deep_tag::Queue;
    use crate::index as db;
    use crate::model::ScannedFile;
    use crate::plan::mtime_ms;
    use crate::tests::index::fresh_db;

    /// 两份 basic 文件：乙比甲新。SQL 取件顺序（mtime DESC）会先拿乙
    fn seed_two() -> (rusqlite::Connection, Vec<i64>) {
        let src = tempfile::tempdir().unwrap();
        let (_d, mut conn) = fresh_db();
        let root = db::add_source_root(&conn, &src.path().to_string_lossy()).unwrap();
        let mut scanned = Vec::new();
        for (i, name) in ["甲.docx", "乙.docx"].iter().enumerate() {
            let p = src.path().join(name);
            std::fs::write(&p, b"body text").unwrap();
            let m = std::fs::metadata(&p).unwrap();
            scanned.push(ScannedFile {
                name: name.to_string(),
                path: p.to_string_lossy().to_string(),
                ext: "docx".into(),
                kind: "word".into(),
                size: m.len(),
                mtime: mtime_ms(&m) + i as i64 * 1000,
            });
        }
        db::upsert_files(&mut conn, root.id, &scanned).unwrap();
        let ids: Vec<i64> = db::list_queued(&conn, Some(root.id))
            .unwrap()
            .into_iter()
            .map(|(id, ..)| id)
            .collect();
        assert_eq!(ids.len(), 2);
        (conn, ids)
    }

    /// T4.5：用户点了「让 AI 说明」的文件，下一次取件必须是它，不按 SQL 排序来
    #[test]
    fn explain_front_job_beats_sql_order() {
        let (conn, ids) = seed_two();
        // 前提：不插队时先取到的是更新的乙
        let sql_first = db::next_deep_job(&conn, &[]).unwrap().unwrap();
        assert_eq!(sql_first.file_id, ids[1], "前提不成立：SQL 顺序应先取乙");

        let q = Queue::new(true);
        q.push_front(ids[0]); // 用户点的是更旧的甲
        let front = q.pop_front();
        let picked = match front {
            Some(id) => db::job_by_id(&conn, id).unwrap(),
            None => db::next_deep_job(&conn, &[]).unwrap(),
        };
        assert_eq!(picked.unwrap().file_id, ids[0], "插队的甲必须先于乙被读");
    }

    /// 插队目标已出局（skipped）或不存在 → 取不到，worker 静默丢件，不许报错
    #[test]
    fn explain_target_out_of_game_returns_none() {
        let (mut conn, ids) = seed_two();
        conn.execute(
            "UPDATE files SET status='skipped' WHERE id=?1",
            [ids[0]],
        )
        .unwrap();
        assert!(db::job_by_id(&conn, ids[0]).unwrap().is_none());
        assert!(db::job_by_id(&conn, 999_999).unwrap().is_none());
    }
}

// ================================================================ P4 M1：重新归类（归档区内二次迁移）

mod p4_m1 {
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
        dir1: i64,
        dir2: i64,
        ids: Vec<i64>,
        arc_path: std::path::PathBuf,
        src_path: std::path::PathBuf,
    }

    /// 真实两目录 + 2 份文件，先走完一次普通迁移（都进了 dir1）
    fn seed() -> Seeded {
        let src = tempfile::tempdir().unwrap();
        let arc = tempfile::tempdir().unwrap();
        let (_d, mut conn) = fresh_db();
        let root = db::add_source_root(&conn, &src.path().to_string_lossy()).unwrap();

        let names: Vec<(String, Vec<u8>)> = vec![
            ("安全检查通知.docx".into(), b"aaa".to_vec()),
            ("检查表.xlsx".into(), b"bbbb".to_vec()),
        ];
        let mut scanned = Vec::new();
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

        let d1 = db::insert_dir(&conn, None, "安全检查", "user").unwrap();
        let d2 = db::insert_dir(&conn, None, "整改台账", "user").unwrap();
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
                    target_dir_id: Some(d1.id),
                    origin: "user_picked".into(),
                    confidence: None,
                    reason: None,
                    decision: "accepted".into(),
                    decided_at: db::now_ms(),
                },
            )
            .unwrap();
        }
        let res = mig::apply(&mut conn, arc.path(), &[], &mut |_, _, _, _| {}).unwrap();
        assert_eq!(res.copied, 2, "前提不成立：迁移要先成功");
        let arc_path = arc.path().to_path_buf();
        let src_path = src.path().to_path_buf();
        Seeded {
            _src: src,
            _arc: arc,
            conn,
            dir1: d1.id,
            dir2: d2.id,
            ids,
            arc_path,
            src_path,
        }
    }

    fn files_in(root: &Path) -> usize {
        walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .count()
    }

    /// T1.2 主路径：挪文件、旧副本进隔离区、archived_path/archive_dir_id 更新、状态与标签不动
    #[test]
    fn recat_moves_file_updates_paths_and_keeps_tags() {
        let mut s = seed();
        let id = s.ids[0];
        // 预置标签：重新归类绝不能动标签
        s.conn
            .execute("INSERT INTO tags(name, kind) VALUES('安全检查','topic')", [])
            .unwrap();
        let tag_id = s.conn.last_insert_rowid();
        s.conn
            .execute(
                "INSERT INTO file_tags(file_id, tag_id) VALUES(?1, ?2)",
                [id, tag_id],
            )
            .unwrap();

        let old_path = s.arc_path.join("安全检查").join("安全检查通知.docx");
        assert!(old_path.is_file(), "前提不成立：文件应已在归档区");

        let out = mig::recategorize(&mut s.conn, &s.arc_path, id, s.dir2).unwrap();
        let new_path = s.arc_path.join("整改台账").join("安全检查通知.docx");
        assert!(new_path.is_file(), "新位置必须有文件");
        assert!(!old_path.exists(), "旧位置必须空出来");
        assert_eq!(out.new_path, new_path.to_string_lossy().to_string());
        assert!(!out.renamed);

        // 旧归档副本进了隔离区（永不直删）
        let q = s.arc_path.join(".filebutler/quarantine");
        assert!(files_in(&q) >= 1, "旧副本必须躺在隔离区，不许直删");
        // 内容一致
        assert_eq!(std::fs::read(&new_path).unwrap(), b"aaa");

        // 库：路径与目录更新、状态保持 migrated、台账 kind
        let (ap, adir, status): (String, i64, String) = s
            .conn
            .query_row(
                "SELECT archived_path, archive_dir_id, status FROM files WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(ap, new_path.to_string_lossy().to_string());
        assert_eq!(adir, s.dir2);
        assert_eq!(status, "migrated");
        let mig_row = db::latest_migration(&s.conn).unwrap().unwrap();
        assert_eq!(mig_row.kind, "recategorize");

        // 标签还在
        let n: i64 = s
            .conn
            .query_row(
                "SELECT COUNT(*) FROM file_tags WHERE file_id=?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "重新归类不许动标签");
    }

    /// T1.2 目标目录有同名 → 自动错开 `_2`，绝不覆盖
    #[test]
    fn recat_renames_on_conflict_never_overwrites() {
        let mut s = seed();
        let id = s.ids[0];
        // 目标目录先躺一份同名但内容不同的文件
        let d2path = s.arc_path.join("整改台账");
        std::fs::create_dir_all(&d2path).unwrap();
        std::fs::write(d2path.join("安全检查通知.docx"), "别人的文件".as_bytes()).unwrap();

        let out = mig::recategorize(&mut s.conn, &s.arc_path, id, s.dir2).unwrap();
        assert!(out.renamed, "撞名必须错开并如实回报");
        let moved = std::path::PathBuf::from(&out.new_path);
        assert!(moved.is_file());
        assert_eq!(std::fs::read(&moved).unwrap(), b"aaa", "错开的是新来的那份");
        assert_eq!(
            std::fs::read(d2path.join("安全检查通知.docx")).unwrap(),
            "别人的文件".as_bytes(),
            "原来那份一个字节都不许动"
        );
    }

    /// T1.2 非 migrated 状态拒绝（池内/待定区/已撤销的都不许走这条路）
    #[test]
    fn recat_rejects_non_migrated() {
        let mut s = seed();
        // 撤销第一次迁移 → 状态回 confirmed
        let m = db::latest_migration(&s.conn).unwrap().unwrap();
        mig::undo(&mut s.conn, &s.arc_path, m.id, &mut |_, _, _, _| {}).unwrap();
        let e = mig::recategorize(&mut s.conn, &s.arc_path, s.ids[0], s.dir2).unwrap_err();
        assert_ne!(e.code, "OK");
    }

    /// T1.2 同目录 / 目录不存在拒绝
    #[test]
    fn recat_rejects_same_dir_and_missing_dir() {
        let mut s = seed();
        let id = s.ids[0];
        let e1 = mig::recategorize(&mut s.conn, &s.arc_path, id, s.dir1).unwrap_err();
        assert!(e1.message.contains("当前目录") || e1.code == "BAD_INPUT");
        let e2 = mig::recategorize(&mut s.conn, &s.arc_path, id, 999_999).unwrap_err();
        assert!(e2.message.contains("目录") || e2.code == "BAD_INPUT");
    }

    /// T1.3 撤销重新归类：旧位置回来、新副本清除、库字段复原、状态保持 migrated
    #[test]
    fn undo_recategorize_restores_old_location() {
        let mut s = seed();
        let id = s.ids[0];
        let old_path = s.arc_path.join("安全检查").join("安全检查通知.docx");
        let out = mig::recategorize(&mut s.conn, &s.arc_path, id, s.dir2).unwrap();
        let new_path = std::path::PathBuf::from(&out.new_path);

        let m = db::latest_migration(&s.conn).unwrap().unwrap();
        let report =
            mig::undo(&mut s.conn, &s.arc_path, m.id, &mut |_, _, _, _| {}).unwrap();
        assert_eq!(report.restored, 1, "撤销要把文件还回旧目录");

        assert!(old_path.is_file(), "旧位置必须有文件");
        assert!(!new_path.exists(), "新位置的副本要清掉（内容已在旧位置）");
        assert_eq!(std::fs::read(&old_path).unwrap(), b"aaa");
        let (ap, adir, status): (String, i64, String) = s
            .conn
            .query_row(
                "SELECT archived_path, archive_dir_id, status FROM files WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(ap, old_path.to_string_lossy().to_string());
        assert_eq!(adir, s.dir1, "要还原到旧目录");
        assert_eq!(status, "migrated", "撤销重新归类不回池，文件仍在归档区");
        let mig_row = db::latest_migration(&s.conn).unwrap().unwrap();
        assert_eq!(mig_row.status, "undone");
    }

    /// 撤销前如果有人动了归档文件（新位置被改）→ sha 不符，撤销拒绝，不制造数据丢失
    #[test]
    fn undo_recategorize_refuses_when_new_copy_was_modified() {
        let mut s = seed();
        let id = s.ids[0];
        let out = mig::recategorize(&mut s.conn, &s.arc_path, id, s.dir2).unwrap();
        std::fs::write(&out.new_path, "被用户改过".as_bytes()).unwrap();
        let m = db::latest_migration(&s.conn).unwrap().unwrap();
        let report = mig::undo(&mut s.conn, &s.arc_path, m.id, &mut |_, _, _, _| {}).unwrap();
        assert_eq!(report.restored, 0, "内容对不上就不许撤");
        assert_eq!(report.failed.len(), 1);
    }
}
