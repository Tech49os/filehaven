//! 核心逻辑单元测试：不依赖 GUI，只验证文件操作与解析的正确性

use crate::extract::{parse_drawio, strip_tags, unescape};
use crate::migrate::{migrate, undo};
use crate::model::Plan;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!("fmtest_{tag}_{nanos}"));
    fs::create_dir_all(&p).unwrap();
    p
}

fn write(p: &PathBuf, content: &str) {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let mut f = fs::File::create(p).unwrap();
    f.write_all(content.as_bytes()).unwrap();
}

#[test]
fn unescape_handles_entities() {
    assert_eq!(unescape("a&amp;b"), "a&b");
    assert_eq!(unescape("&lt;tag&gt;"), "<tag>");
    assert_eq!(unescape("&#65;&#x42;"), "AB");
    assert_eq!(unescape("no entity"), "no entity");
    // 未闭合的 & 不应丢字符、也不应重复输出
    assert_eq!(unescape("a & b"), "a & b");
    // 未知实体原样保留，且不能死循环
    assert_eq!(unescape("&foo;bar"), "&foo;bar");
    assert_eq!(unescape("&"), "&");
    assert_eq!(unescape("100% & 50%"), "100% & 50%");
}

#[test]
fn strip_tags_removes_markup() {
    assert_eq!(strip_tags("<b>支付</b>系统"), "支付 系统");
    assert_eq!(strip_tags("a<br>b"), "a b");
    assert_eq!(strip_tags("  many   spaces  "), "many spaces");
}

#[test]
fn parse_drawio_plain_xml() {
    let xml = r#"<mxfile><diagram id="d1">
      <mxGraphModel><root>
        <mxCell id="0"/>
        <mxCell id="1" value="用户中心" vertex="1"/>
        <mxCell id="2" value="订单服务" vertex="1"/>
        <mxCell id="3" value="12345" vertex="1"/>
        <mxCell id="4" value="&lt;b&gt;支付网关&lt;/b&gt;" vertex="1"/>
      </root></mxGraphModel>
    </diagram></mxfile>"#;
    let (nodes, err) = parse_drawio(xml);
    assert!(!err, "不应报解析失败");
    assert!(nodes.contains(&"用户中心".to_string()));
    assert!(nodes.contains(&"订单服务".to_string()));
    assert!(nodes.contains(&"支付网关".to_string()), "应能剥离内嵌 HTML");
    assert!(!nodes.contains(&"12345".to_string()), "纯数字应被过滤");
}

#[test]
fn parse_drawio_compressed() {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use flate2::write::DeflateEncoder;
    use flate2::Compression;

    let inner = r#"<mxGraphModel><root>
        <mxCell id="1" value="数据中台" vertex="1"/>
        <mxCell id="2" value="消息队列" vertex="1"/>
      </root></mxGraphModel>"#;
    let encoded = inner.replace('<', "%3C").replace('>', "%3E").replace('"', "%22");

    let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
    enc.write_all(encoded.as_bytes()).unwrap();
    let compressed = enc.finish().unwrap();
    let b64 = B64.encode(compressed);

    let xml = format!(r#"<mxfile><diagram id="d">{b64}</diagram></mxfile>"#);
    let (nodes, err) = parse_drawio(&xml);
    assert!(!err, "压缩格式应能解析");
    assert!(nodes.contains(&"数据中台".to_string()), "实际节点: {nodes:?}");
    assert!(nodes.contains(&"消息队列".to_string()));
}

#[test]
fn migrate_three_stage_with_conflict_and_missing() {
    let base = tmp_dir("migrate");
    let src = base.join("src");
    let archive = base.join("archive");
    fs::create_dir_all(&src).unwrap();

    write(&src.join("a.txt"), "内容 A");
    write(&src.join("b.txt"), "内容 B");

    let plans = vec![
        Plan {
            file_id: "f1".into(),
            file_name: "a.txt".into(),
            source_path: src.join("a.txt").to_string_lossy().to_string(),
            target_dir: "项目X\\文档".into(),
            target_name: "同名.txt".into(),
        },
        Plan {
            file_id: "f2".into(),
            file_name: "b.txt".into(),
            source_path: src.join("b.txt").to_string_lossy().to_string(),
            target_dir: "项目X\\文档".into(),
            target_name: "同名.txt".into(), // 与上一条同名，必须加序号而非覆盖
        },
        Plan {
            file_id: "f3".into(),
            file_name: "c.txt".into(),
            source_path: src.join("c.txt").to_string_lossy().to_string(), // 不存在
            target_dir: "项目X".into(),
            target_name: "c.txt".into(),
        },
    ];

    let root = archive.to_string_lossy().to_string();
    let rep = migrate(None, &root, "b_test1", plans).unwrap();

    assert_eq!(rep.migrated, 2, "两条正常文件应迁移成功");
    assert_eq!(rep.failed, 1, "缺失源文件应记失败而不是整批中断");

    let d1 = archive.join("项目X").join("文档").join("同名.txt");
    let d2 = archive.join("项目X").join("文档").join("同名_1.txt");
    assert!(d1.is_file(), "第一个文件应就位");
    assert!(d2.is_file(), "同名文件应加序号，不能覆盖");
    assert_eq!(fs::read_to_string(&d1).unwrap(), "内容 A");
    assert_eq!(fs::read_to_string(&d2).unwrap(), "内容 B");

    // 源文件已被隔离，不再留在原处
    assert!(!src.join("a.txt").exists());
    assert!(!src.join("b.txt").exists());

    // 隔离区应保留原件
    let trash = archive.join(".filemanager").join("trash").join("b_test1");
    assert!(trash.join("f1_a.txt").is_file(), "隔离区应有原件");
    assert!(trash.join("f2_b.txt").is_file());

    // 台账已写
    let ledger = archive.join(".filemanager").join("ledger").join("migrations.jsonl");
    assert!(ledger.is_file(), "应写出迁移台账");
    let body = fs::read_to_string(&ledger).unwrap();
    assert!(body.contains("b_test1"));

    // ---- 撤销：应把两个文件还原回原处，并清理归档副本 ----
    let undo_rep = undo(&root, "b_test1", rep.records.clone()).unwrap();
    assert_eq!(undo_rep.restored, 2, "两个文件应全部还原");
    assert_eq!(fs::read_to_string(src.join("a.txt")).unwrap(), "内容 A");
    assert_eq!(fs::read_to_string(src.join("b.txt")).unwrap(), "内容 B");
    assert!(!d1.exists(), "撤销后归档副本应清理");
    assert!(!d2.exists());

    fs::remove_dir_all(&base).ok();
}

#[test]
fn migrate_never_overwrites_existing_file() {
    let base = tmp_dir("nooverwrite");
    let src = base.join("src");
    let archive = base.join("archive");
    fs::create_dir_all(&src).unwrap();

    // 目标位置已存在同名文件，内容不同
    write(&archive.join("文档").join("报告.txt"), "旧的内容");
    write(&src.join("报告.txt"), "新的内容");

    let plans = vec![Plan {
        file_id: "f1".into(),
        file_name: "报告.txt".into(),
        source_path: src.join("报告.txt").to_string_lossy().to_string(),
        target_dir: "文档".into(),
        target_name: "报告.txt".into(),
    }];

    let root = archive.to_string_lossy().to_string();
    let rep = migrate(None, &root, "b_test2", plans).unwrap();
    assert_eq!(rep.migrated, 1);

    // 旧文件必须原样保留
    assert_eq!(
        fs::read_to_string(archive.join("文档").join("报告.txt")).unwrap(),
        "旧的内容",
        "既有文件绝不能被覆盖"
    );
    assert_eq!(
        fs::read_to_string(archive.join("文档").join("报告_1.txt")).unwrap(),
        "新的内容"
    );

    fs::remove_dir_all(&base).ok();
}
