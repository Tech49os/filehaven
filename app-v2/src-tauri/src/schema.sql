-- 文件管家 P1 · 一次建全 10 张表（P2/P3 只加读写逻辑，不改表）
-- 依据：设计文档_P1_详细设计_v1.0 §5.2

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS source_roots (          -- P1
  id       INTEGER PRIMARY KEY,
  path     TEXT NOT NULL UNIQUE,
  enabled  INTEGER NOT NULL DEFAULT 1,
  added_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS archive_dirs (          -- P1
  id           INTEGER PRIMARY KEY,
  parent_id    INTEGER REFERENCES archive_dirs(id),
  name         TEXT NOT NULL,
  rel_path     TEXT NOT NULL UNIQUE,               -- 统一 / 分隔、不带首尾 /
  depth        INTEGER NOT NULL,                   -- 1 起
  origin       TEXT NOT NULL,                      -- imported|model_approved|user|inline
  created_at   INTEGER NOT NULL,
  last_used_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_dirs_parent ON archive_dirs(parent_id);

CREATE TABLE IF NOT EXISTS files (                 -- P1
  id             INTEGER PRIMARY KEY,
  source_root_id INTEGER REFERENCES source_roots(id),
  abs_path       TEXT NOT NULL UNIQUE,
  rel_path       TEXT NOT NULL,
  name           TEXT NOT NULL,
  ext            TEXT NOT NULL,
  kind           TEXT NOT NULL,                    -- word|excel|ppt|pdf|image|diagram|other
  size           INTEGER NOT NULL,
  mtime          INTEGER NOT NULL,                 -- 毫秒
  content_hash   TEXT,                             -- P1 不填（P4 增量用）
  tag_level      TEXT NOT NULL DEFAULT 'basic',    -- basic|deep
  status         TEXT NOT NULL DEFAULT 'queued',
  defer_count    INTEGER NOT NULL DEFAULT 0,
  summary        TEXT,                             -- 废弃占位：P3 才写
  note           TEXT,
  target_name    TEXT,                             -- 归档时改成的名字（NULL = 保持原名）
  tag_locked     INTEGER NOT NULL DEFAULT 0,        -- P3：人改过标签，队列重跑不许覆盖
  archived_path  TEXT,                             -- P3：迁移后在归档区的真实路径
  archive_dir_id INTEGER,                          -- P3：迁移落到了哪个归档目录
  discovered_at  INTEGER NOT NULL,
  last_seen_at   INTEGER NOT NULL,
  missing        INTEGER NOT NULL DEFAULT 0        -- 废弃：以 status='missing' 为准 (C7)
);
CREATE INDEX IF NOT EXISTS idx_files_status   ON files(status);
CREATE INDEX IF NOT EXISTS idx_files_mtime    ON files(mtime);
CREATE INDEX IF NOT EXISTS idx_files_root     ON files(source_root_id);
CREATE INDEX IF NOT EXISTS idx_files_name     ON files(name);

CREATE TABLE IF NOT EXISTS tags (                  -- P1（只写 type/time/keyword）
  id   INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,                              -- type|time|keyword|topic|user
  UNIQUE(name, kind)
);

CREATE TABLE IF NOT EXISTS file_tags (             -- P1
  file_id    INTEGER REFERENCES files(id) ON DELETE CASCADE,
  tag_id     INTEGER REFERENCES tags(id) ON DELETE CASCADE,
  confidence REAL,
  PRIMARY KEY(file_id, tag_id)
);
CREATE INDEX IF NOT EXISTS idx_file_tags_tag ON file_tags(tag_id);

CREATE TABLE IF NOT EXISTS batches (               -- P1
  id         INTEGER PRIMARY KEY,
  seq        INTEGER NOT NULL,
  size       INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  closed_at  INTEGER
);

CREATE TABLE IF NOT EXISTS assignments (           -- P1
  id            INTEGER PRIMARY KEY,
  file_id       INTEGER REFERENCES files(id),
  batch_id      INTEGER REFERENCES batches(id),
  target_dir_id INTEGER REFERENCES archive_dirs(id),
  origin        TEXT NOT NULL,                     -- recommended|user_changed|user_picked|new_dir
  confidence    REAL,
  reason        TEXT,
  decision      TEXT NOT NULL,                     -- accepted|changed|deferred|parked
  decided_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_assign_batch ON assignments(batch_id);
CREATE INDEX IF NOT EXISTS idx_assign_file  ON assignments(file_id, decided_at);

CREATE TABLE IF NOT EXISTS migrations (            -- P1
  id          INTEGER PRIMARY KEY,
  batch_id    INTEGER,
  started_at  INTEGER,
  finished_at INTEGER,
  status      TEXT,                                -- running|done|partial|failed
  file_count  INTEGER,
  bytes       INTEGER
);

CREATE TABLE IF NOT EXISTS migration_items (       -- P1
  id           INTEGER PRIMARY KEY,
  migration_id INTEGER REFERENCES migrations(id),
  file_id      INTEGER,
  src_path     TEXT,
  dst_path     TEXT,
  sha256       TEXT,
  result       TEXT,                               -- ok|failed|skipped
  error        TEXT
);
CREATE INDEX IF NOT EXISTS idx_mitems_mig ON migration_items(migration_id);
