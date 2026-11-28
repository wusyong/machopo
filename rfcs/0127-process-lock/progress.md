# RFC 0127 的進度：process lock 和 Browser 的檢查

開始日期：2026-11-28。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點（`details.md` 的「盤點」「Chromium」）、已決定的事項第 1 到 7 條 | 做完 | 2026-11-28 |
| 1. renderer 說了算的地方：blob URL 照 loader 的 origin 檢查、不能蓋別人的（第 3 條）；`OpenUrl` 帶發起者（新分頁的 `SameSite`）；被攻破的 renderer 的角色；`docs/architecture.md` 的「Process 的邊界」 的規則 | 進行中 | |
| 2. 鎖：`SiteInfo::of_document` 和歷史紀錄的 `SiteInfo`、「鎖收不收」取代 `swaps`（第 1 條）；交出 loader、storage area、commit 時的檢查（第 2 條）；`set_site_info` 只寫一次；`ProcessLockMismatch`（第 6 條）；收尾 | | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| blob URL 經過 Browser 給過的 loader 登記（worker 的經過 worker 的），網址的 origin 是 loader 的或 opaque 的；別的 origin、不是 `blob:` 的、沒給過的 loader、不是 worker 的 loader 是壞訊息，什麼都不登記 | `cargo test -p browser --lib render_process_host::tests::a_blob_url_is_of_its_loaders_origin_and_no_one_elses` |
| 別的 renderer 登記過的網址（撤銷了、導覽還拿得到的也算）不能再登記，自己的可以 | 同上；`cargo test -p net --lib blob::tests::another_renderers_url_is_kept` |
| 被攻破的 renderer（自己的 process）拿著它的文件的 loader 登記別的網站的 blob URL、或別人的網址：被結束，網址沒有登記給導覽、別人的還是別人的 | `cargo test -p browser --test it renderer_process::a_renderer_registers_no_blob_url_but_its_documents_own` |
| renderer 登記 blob URL 帶著文件的 loader；Browser 沒給 loader 的文件不登記 | `cargo test -p dom_js --test it files::blob_urls_go_to_the_host`；真的頁面：`cargo test -p shell --test it documents::blob_urls_load_until_revoked` |
| 頁面開到別的網站的新分頁帶 `Lax` 的 cookie、不帶 `Strict` 的；使用者自己的導覽兩個都帶 | `cargo test -p browser --test it cookies::a_new_tabs_navigation_is_the_pages_that_opened_it` |
| 新分頁的 `about:blank` 是開它的頁面的 origin | `cookies::a_new_tabs_about_blank_is_of_the_page_that_opened_it` |
| 新分頁由不在樹上的 frame 發起是壞訊息；本機檔案只給本機檔案的頁面開 | `navigation::links::a_bad_message_ends_the_renderer`、`navigation::a_link_to_a_new_tab_is_handed_to_the_embedder` |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 第 1 段：renderer 說了算的地方（2026-11-28，Linux：Fedora 45，x86-64，AMD Radeon 890M）

- **新的測試**：
  - `browser` 的 `render_process_host::tests::a_blob_url_is_of_its_loaders_origin_and_no_one_elses`：
    - loader 的 origin、opaque 的、opaque 的 loader 的照收；
    - 別的 origin、別的 scheme、不是 `blob:` 的、解析不了的、沒給過的 loader、不是 worker 的 loader 都是壞訊息，也不登記；
    - 別的 renderer 的網址（撤銷了的也算）是 `BlobUrlTaken`。
  - `net` 的 `blob::tests::another_renderers_url_is_kept`。
  - 被攻破的 renderer 的兩個角色（`crates/browser/tests/it/renderer_process.rs` 的 `rogue_on_commit`：讀 Browser 送的 commit，拿它的 loader 和網址），`renderer_process::a_renderer_registers_no_blob_url_but_its_documents_own`。
  - `cookies::a_new_tabs_navigation_is_the_pages_that_opened_it`、`cookies::a_new_tabs_about_blank_is_of_the_page_that_opened_it`。
  - `navigation::links::a_bad_message_ends_the_renderer` 多了不在樹上的 frame 開的新分頁。
  - `dom_js` 的 `files::blob_urls_go_to_the_host` 多了沒有 loader 的文件不登記。
- **改的設計**：
  - `ChangeStorage` 的網址照 Chromium 不檢查（已決定的事項第 8 條）。
  - 第 2 條寫的「`browser` 的 README」不存在，規則寫在 `docs/architecture.md` 的「Process 的邊界」的第 9 條。
- **mutation test**（`mutants-1.toml`，15 個）：15 個都抓到。
  - 第一次跑時，「an opaque blob URL is refused」寫成拿掉那一行，`match` 不完整、編譯不過。改成回答壞訊息之後重跑，抓到了。
  - 跑完確認每個 mutant 的 `from` 都還在檔案裡（檔案都還原了）；`cargo fmt` 之後也確認了一次。
- **不穩定的測試**：`shell` 的 `renderer_sandbox::a_renderer_process_is_told_the_zone_a_copied_localtime_is` 在第一次的 `cargo test --workspace` 裡失敗了一次。
  - 失敗的是第 798 行：renderer process 的環境變數的 `TZ` 不是測試算的時區。完整的訊息被輸出的篩選截掉了。
  - 前面一行比 offset 的 assert 過了，所以 renderer 看到的時區是對的。
  - 之後都沒再失敗：
    - 單獨跑 8 次；
    - 同時跑另外 4 個測試 binary 當負載，再跑 15 次；
    - 第二次的整個 `cargo test --workspace`。
  - 同時跑好幾份這個測試會互相搶同一個暫存資料夾（`CARGO_TARGET_TMPDIR/renderer-copied-localtime`），那樣的失敗不算數。
  - 第 1 段沒有碰到它走的路（`about:blank`、時區、sandbox）。這個測試是 RFC 0120 加的。
  - 原因沒找到。為了下次看得到，這個 assert 改成失敗時印出 renderer 的 pid、所有 renderer 的 pid 和它的整個環境。
- **跑過的**：
  - 改到的 7 個 crate 的測試；
  - sanity 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；
  - `cargo fmt`、`cargo clippy --workspace --all-targets -- -D warnings`；
  - `cargo test --workspace`：第二次 5,065 個通過、0 個失敗、1 個 ignored。比 RFC 0124 做完時少，是因為 RFC 0126 把 sunaba 的測試搬出了 workspace；
  - `test.py wpt FileAPI/url FileAPI/BlobURL`：0 個意外的結果。
  - `test.py scenarios --window`（階段的驗收）：75 次，0 個失敗。
- 量測工具的測試沒跑：renderer 的啟動、trace、census 和 automation pipe 的 CDP 方法都沒改。
