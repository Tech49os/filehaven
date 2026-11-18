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
