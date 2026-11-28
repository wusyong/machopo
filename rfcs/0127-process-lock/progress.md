# RFC 0127 的進度：process lock 和 Browser 的檢查

開始日期：2026-11-28。完成日期：2026-11-28（Linux：Fedora 45，x86-64，AMD Radeon 890M）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點（`details.md` 的「盤點」「Chromium」）、已決定的事項第 1 到 7 條 | 做完 | 2026-11-28 |
| 1. renderer 說了算的地方：blob URL 照 loader 的 origin 檢查、不能蓋別人的（第 3 條）；`OpenUrl` 帶發起者（新分頁的 `SameSite`、`about:blank` 的 origin）；`ChangeStorage` 的網址照 Chromium 不檢查（第 8 條）；被攻破的 renderer 的角色；`docs/architecture.md` 的「Process 的邊界」的第 9 條規則 | 做完 | 2026-11-28 |
| 2. 鎖：`SiteInfo::of_document` 和歷史紀錄的 `SiteInfo`、`site_info::admits` 取代 `swaps`（第 1 條）；交出 commit、loader、storage area 時的檢查（第 2 條，`site_info::allows`）；`set_site_info` 只寫一次；`ProcessLockMismatch`（第 6 條）；收尾 | 做完 | 2026-11-28 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| **blob URL** | |
| 經過 Browser 給過的 loader 登記（worker 的經過 worker 的），網址的 origin 是 loader 的或 opaque 的；別的 origin、不是 `blob:` 的、沒給過的 loader、不是 worker 的 loader 是壞訊息，什麼都不登記 | `cargo test -p browser --lib render_process_host::tests::a_blob_url_is_of_its_loaders_origin_and_no_one_elses` |
| 別的 renderer 登記過的網址（撤銷了、導覽還拿得到的也算）不能再登記，自己的可以 | 同上；`cargo test -p net --lib blob::tests::another_renderers_url_is_kept` |
| 被攻破的 renderer（自己的 process）拿著它的文件的 loader 登記別的網站的 blob URL、或別人的網址：被結束，網址沒有登記給導覽，別人的還是別人的 | `cargo test -p browser --test it renderer_process::a_renderer_registers_no_blob_url_but_its_documents_own` |
| renderer 登記 blob URL 帶著文件的 loader；Browser 沒給 loader 的文件不登記 | `cargo test -p dom_js --test it files::blob_urls_go_to_the_host`；真的頁面：`cargo test -p shell --test it documents::blob_urls_load_until_revoked` |
| **新分頁** | |
| 頁面開到別的網站的新分頁帶 `Lax` 的 cookie、不帶 `Strict` 的；使用者自己的導覽兩個都帶 | `cargo test -p browser --test it cookies::a_new_tabs_navigation_is_the_pages_that_opened_it` |
| 新分頁的 `about:blank` 是開它的頁面的 origin | `cookies::a_new_tabs_about_blank_is_of_the_page_that_opened_it` |
| 新分頁由不在樹上的 frame 發起是壞訊息；本機檔案只給本機檔案的頁面開 | `navigation::links::a_bad_message_ends_the_renderer`、`navigation::a_link_to_a_new_tab_is_handed_to_the_embedder` |
| **一份文件是誰的、換不換** | |
| 每一種文件的 `SiteInfo`（網頁、繼承了 origin 的和 opaque 的 `about:blank`、`blob:`、`data:`、錯誤頁、`file:`、內建頁），鎖收不收它 | `cargo test -p browser --lib site_info::` |
| 重新整理之後落在別的網站（伺服器改成 redirect）：新的 renderer | `cargo test -p browser --test it process_swap::a_reload_that_lands_on_another_site_commits_in_a_new_renderer` |
| 跨網站的 iframe 把最上層導覽到 `about:blank`：那個網站的新 renderer，origin 是 iframe 的 | `process_swap::about_blank_of_another_sites_origin_commits_in_that_sites_renderer` |
| `data:` 的重新整理、從 `about:blank` 上一頁回到它：同一個 renderer（entry 沿用它 commit 時的網站）；回到別的 `data:` entry：新的 | `process_swap::every_data_url_is_a_site_of_its_own` |
| **鎖** | |
| 第一份有網站的文件鎖住 renderer，之後不變，別的網站的是錯；不合鎖的 loader 不登記，還沒鎖的只拿得到 opaque 的最上層網站的 | `cargo test -p browser --lib render_process_host::tests::a_renderer_is_locked_to_its_first_site`、`site_info::tests::a_renderer_is_given_only_its_top_level_sites` |
| 正常的瀏覽一次都不踩鎖 | `cargo test --workspace`、`python3 scripts/test.py scenarios`（`--window` 也是）、`test.py wpt` 的幾個目錄（下面「做過的檢查」） |
| 鎖拒絕時不交出、結束 renderer（`WebContents` 的那幾支） | 從外面走不到，只靠審查（`docs/test-gaps.md` 的「只靠審查」） |

## 做過的檢查

2026-11-28，Linux：Fedora 45，x86-64，AMD Radeon 890M。

- **新的測試**：
  - 單元測試：`site_info` 的表（`of_document`、`admits`、`allows`）；`render_process_host` 的 `a_renderer_is_locked_to_its_first_site`（`RendererLauncher::Detached` 的 host）和 `a_blob_url_is_of_its_loaders_origin_and_no_one_elses`；`net` 的 `blob::tests::another_renderers_url_is_kept`。
  - 被攻破的 renderer 的兩個角色（`crates/browser/tests/it/renderer_process.rs` 的 `rogue_on_commit`：讀 Browser 送的 commit，拿它的 loader 和網址）。
  - 整合測試：新分頁的 `SameSite` 和 `about:blank`；重新整理之後落在別的網站；繼承了跨網站 iframe 的 origin 的 `about:blank`；`data:` 的上一頁、下一頁；不在樹上的 frame 開的新分頁加進壞訊息的清單；`dom_js` 的沒有 loader 的文件不登記 blob URL。
  - 測試伺服器的路由加了 `Route::then`（第一次回這個，之後回下一個），給「重新整理時頁面已經搬走」用。
- **改的設計**（`README.md` 都改寫了）：
  - `ChangeStorage` 的網址照 Chromium 不檢查（已決定的事項第 8 條）。
  - 第 2 條寫的「`browser` 的 README」不存在，規則寫在 `docs/architecture.md` 的「Process 的邊界」的第 9 條。
  - 草稿寫「`file:` 的最上層網站要鎖成 `File`」：`file:` 的文件的 origin 是 opaque 的，最上層網站分不出它和其他 opaque 的，照 opaque 的給任何 renderer；讀檔照 commit 給的權限。
  - 草稿想用 `Detached` 造出不合鎖的狀態測鎖的安全網：做不到（換 renderer、renderer 不在了時 frame 樹清成一個 opaque 的 main frame），記進 `docs/test-gaps.md`。
- **mutation test**：
  - 第 1 段（`mutants-1.toml`，15 個）：都抓到。「an opaque blob URL is refused」第一次寫成拿掉那一行，`match` 不完整、編譯不過；改成回答壞訊息之後重跑，抓到了。
  - 第 2 段（`mutants-2.toml`，18 個）：第一次跑 14 個抓到、4 個沒抓到。
    - 「a traversal keeps no site」：原本的 `data:` 測試在上一頁時本來就會換 renderer，看不出差別。補了「從 `about:blank` 上一頁回到 `data:` 留在同一個 renderer」，重跑抓到了。
    - 「a document the lock refuses commits anyway」「a storage area the lock refuses is opened」「a lock's mismatch isn't a bad message」：`WebContents` 裡鎖拒絕時的那幾支，從外面走不到（上面「改的設計」的最後一條），整個 `browser` 的整合測試都沒有失敗。記在 `docs/test-gaps.md` 的「只靠審查」。
  - 每次跑完都確認每個 mutant 的 `from` 還在檔案裡（檔案都還原了）；`cargo fmt` 之後也確認了一次，改了一個被 fmt 重排的 `from`。
- **不穩定的測試**：`shell` 的 `renderer_sandbox::a_renderer_process_is_told_the_zone_a_copied_localtime_is` 在第 1 段的第一次 `cargo test --workspace` 裡失敗了一次。
  - 失敗的是比 `TZ` 的 assert：renderer process 的環境變數的 `TZ` 不是測試算的時區。完整的訊息被輸出的篩選截掉了。
  - 前面一行比 offset 的 assert 過了，所以 renderer 看到的時區是對的。
  - 之後都沒再失敗：單獨跑 8 次；同時跑另外 4 個測試 binary 當負載，再跑 15 次；之後兩次的整個 `cargo test --workspace`。
  - 同時跑好幾份這個測試會互相搶同一個暫存資料夾（`CARGO_TARGET_TMPDIR/renderer-copied-localtime`），那樣的失敗不算數。
  - 這份 RFC 沒有碰到它走的路（`about:blank` 的 opaque 的 origin、時區、sandbox）。這個測試是 RFC 0120 加的。
  - 原因沒找到。為了下次看得到，這個 assert 改成失敗時印出 renderer 的 pid、所有 renderer 的 pid 和它的整個環境。
- **WPT 的預期結果**：`html/browsers/history/the-history-interface/history_pushstate_url_rewriting.html` 預期 `ERROR`，現在是 `OK`。在這份 RFC 之前的 `main`（`3ba43af0`）上也是 `OK`：它早就會過，只是這個目錄之前沒跑。用 `MACHOPO_UPDATE_EXPECTATIONS` 更新了。
- **跑過的**：
  - 改到的 crate 的測試：第 1 段 7 個（`protocol`、`net`、`browser`、`renderer`、`dom_api`、`dom_js`、`shell`），第 2 段 3 個（`browser`、`net`、`shell`）；
  - sanity 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；
  - `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`；
  - `cargo test --workspace`：第 1 段 5,065 個通過，第 2 段 5,068 個通過，都是 0 個失敗、1 個 ignored。比 RFC 0124 做完時少，是因為 RFC 0126 把 sunaba 的測試搬出了 workspace；
  - `test.py wpt`：第 1 段 `FileAPI/url`、`FileAPI/BlobURL`，0 個意外的結果；第 2 段 `FileAPI/url`、`html/browsers/browsing-the-web/unloading-documents`、`html/browsers/history/the-history-interface`，99 個測試，意外的只有上面那一個；
  - `test.py scenarios`：99 次，0 個失敗；
  - `test.py scenarios --window`：第 1 段、第 2 段各 75 次，0 個失敗。
- 量測工具的測試沒跑：renderer 的啟動、trace、census 和 automation pipe 的 CDP 方法都沒改。
- 在視窗裡看不出新的行為（鎖只在出錯時看得到），`about:demo` 和情境沒有加東西：現有的情境（`sites.txt` 走過跨網站的導覽、上一頁、下一頁）是回歸檢查。
