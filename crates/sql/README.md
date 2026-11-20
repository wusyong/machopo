# sql

對應 Chromium 的 `//sql`：profile 的資料庫。每種資料一個資料庫，是 profile 的資料夾裡的一個檔案（只在記憶體的 profile 開在記憶體裡，走同一套程式），用的一方在自己的 thread 上拿著自己的連線：Chromium 的每個功能各自的資料庫和 page cache。SQLite 連同原始碼一起編（`rusqlite` 的 `bundled`）；這個 crate re-export `rusqlite`，用資料庫的 crate 只依賴 `sql`。

```rust
let mut db = sql::Database::open(profile.dir(), "Cookies")?; // dir 是 None：在記憶體裡
db.migrate(&[
    "CREATE TABLE cookies (...);",          // 版本 1
    "ALTER TABLE cookies ADD COLUMN ...;",  // 版本 2：之後只能往後加
])?;
db.connection().execute("INSERT INTO cookies ...", (...))?;
```

## 設定

照 Chromium 的 `sql::Database` 的預設：

- exclusive locking：打開它的連線之外，沒有人能讀寫它（profile 的資料夾本來就只給一個 machopo）。被鎖住時不等，馬上是錯誤（`rusqlite` 預設等 5 秒，SQLite 和 Chromium 不等）：那表示有另一條連線開了它。
- `journal_mode=TRUNCATE`（rollback journal，每次 commit 之後截斷），`synchronous` 用 SQLite 預設的 FULL：當掉、斷電時檔案不壞。Chromium 的註解說 WAL 還在開發中，不用。
- 打開時就讀 schema：拿到鎖，也找出壞掉的檔案。

## schema 的版本

`migrate` 照 `PRAGMA user_version` 跑還沒跑過的那幾個，每個一個 transaction、連同新的版本一起 commit（失敗的整個退回）。清單只能往後加，不能改已經有的。比清單新的版本（較新的 machopo 寫的）是 `Error::Newer`，不動它（Chromium 的 `sql::MetaTable` 還有「相容的版本」，讓舊版讀新版的資料庫；machopo 還不需要）。

## 壞掉的檔案

不是資料庫、或 schema 讀不出來的檔案，移到旁邊（`名稱.corrupt`，蓋掉上一個）、建一個新的，log 裡有一筆警告（Chromium 的 `RazeAndPoison` 之後重新開始）。用的一方看到的是一個新的資料庫：存過的資料沒了。

## 測試

`cargo test -p sql`：記憶體和資料夾、跨開關保留、設定（journal、locking、被鎖住時不等）、schema 的版本（只跑新的、較新的拒絕、失敗的退回）、不是資料庫的檔案和 schema 壞掉的檔案。
