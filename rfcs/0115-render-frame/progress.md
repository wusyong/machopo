# RFC 0115 的進度：每個 frame 的狀態照 content 和 Blink 的界線各有一個擁有者

開始日期：2026-11-25

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. renderer 那一側收成 `RenderFrame`（已決定的事項第 2 到 8 條） | 進行中 | |
| 1a. `render_frame.rs`：`RenderFrames`、`RenderFrame`、`NavigationRequest`，`FrameDocument` 搬進來；最上層的 `document` 和 `ChildFrames::documents` 改成表上的 `document`（第 2、3、4 條） | 做完，待審查 | 2026-11-25 |
| 1b. 導覽的帳（`ChildFrames::navigations`、`main_origin_source`）、`owner_loads`、`devtools_main_frame_token` 搬進 `RenderFrame`，每種轉換一個方法（第 3、5 條）；frame 不見時一起拿掉（第 8 條） | 做完，待審查 | 2026-11-25 |
| 1c. debug build 在讀表時檢查沒分派的 `FrameRequest`（第 6 條） | 沒開始 | |
| 2 到 4. 方向（已決定的事項第 1 條）：第 1 段做完之後重新評估 | — | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |

## 做過的檢查

### 1b（2026-11-25，Linux：Fedora 45，debug build，`5b6297dd` 之上）

**做了什麼**：`RenderFrame` 多了 `navigation`（`NavigationRequest`：`begin_id`、`no_referrer`、`origin_source`，最上層和子 frame 同一種）和 `owner_load_at`；表上多了 `top_token`（Browser 給最上層的，換文件、換 engine 都留著）和跨 frame 的 `commits` 佇列。轉換是表的方法：`navigation_sent`（取代之前的導覽和排著的 commit）、`navigation`、`take_navigation`（照編號）、`take_any_navigation`（Browser 發起的 commit 拿走 renderer 在等的那個，照原本的行為）、`queue_commit`、`next_commit`、`is_navigating`、`owner_load_at`、`owner_loads_due`、`take_due_owner_loads`；`commit_top` 清掉子 frame 和它們的 commit、留著最上層在等的導覽，`remove` 連 commit 一起拿掉，`clear` 留著 token。`ChildFrames`、`ChildNavigationRequest`、`RenderThread` 的 `main_origin_source`、`devtools_main_frame_token` 拿掉。最上層每一次送出的導覽都記（原本只記要繼承 origin 的那種）：commit 時只拿 `begin_id` 對得上的，沒有 origin 的結果一樣。

和已決定的事項不同的兩處（改進了 README 的第 3、4 條）：DevTools 的 token 在表上、不在最上層的 `RenderFrame`；`FrameDocument` 的定義留在 `documents.rs`（1a 記過）。

**找到的 bug**（第 8 條）：frame 不見時它排著的 owner `load` 沒有拿掉（`ChildFrames::remove` 漏了 `owner_loads`）。看不到效果：到期時找不到 iframe 就跳過、順便清掉，只是最多多留 100 ms，`known()` 的期間把它當成還在。現在 frame 不見時一起拿掉，由單元測試守。

**測試**：`render_frame::tests` 多兩個：`a_frame_navigates_one_navigation_at_a_time`（取代之前的導覽和 commit、照編號拿、Browser 發起的拿任何一個、最上層的撐過別的 commit、子 frame 的 commit 跟著舊文件走）、`a_frame_gone_takes_its_owner_load_and_commits`（最早到期的先、拿走到期的、frame 不見時帶走它的、token 撐過換 engine）。`renderer`、`shell` 的測試通過。WPT（`html/browsers/`、`html/semantics/embedded-content/the-iframe-element/`，404 個）：0 個退步；2 個非預期的通過（`history_pushstate_url_rewriting.html`、`overlapping-navigations-and-traversals/cross-document-traversal-cross-document-nav.html` 的一個子測試）在沒有 1b 的程式上（`git stash`）三次都通過，是原本就過時的預期結果，沒改。

**mutation test**（`mutants-1b.toml`，14 個，都抓到）：每一種轉換（送出、拿走、排 commit、在不在導覽、最上層的 commit、owner `load` 的三個、frame 不見、換 engine）和「最上層的導覽沒記下來」（`shell` 的測試抓到，其他的是單元測試）。

### 1a（2026-11-25，Linux：Fedora 45，debug build，`65f6530a` 之上）

**做了什麼**：新的 `render_thread/render_frame.rs`：`RenderFrames`（`BTreeMap<FrameId, RenderFrame>` 加上最上層是哪一個）、`RenderFrame`（這一段只有 `document: Option<FrameDocument>`）。`RenderThread` 的 `document` 和 `ChildFrames::documents` 拿掉，`doc`、`doc_mut`、`is_top`、`frames`、走訪的地方都經過表；最上層 commit 用 `commit_top`（清掉舊文件的 frame），子 frame 的文件用 `set_child_document`，frame 不見時 `follow_child_frames` 也從表上拿掉，換 engine 時 `clear`。`ChildFrames` 只剩導覽的帳（1b 搬）。

和已決定的事項不同的一處：第 4 條說 `FrameDocument` 的定義搬進 `render_frame.rs`；它的三十個欄位是 `documents.rs` 私有的，搬過去要全部改成 `pub(super)`（`render_thread` 底下的每個模組都看得到），所以定義留在 `documents.rs`，表在 `render_frame.rs`。

走訪的順序：原本子 frame 的文件照 `follow_child_frames` 推進去的順序（parent 在 children 之前），commit 了新文件的子 frame 移到最後；現在照 frame 的編號（建立的順序），commit 不會移位。看得到順序的只有發子資源的請求、網路的 idle 的檢查這類，測試沒有差別。

**測試**：`renderer` 的 `render_frame::tests` 兩個（最上層在前、子 frame 照建立的順序、最上層 commit 時清掉舊的 frame；換掉子 frame 的文件、frame 不見、換 engine）。`renderer`、`shell` 的測試通過（`shell` 的 webdriver 的那一個見下一節）。

**mutation test**（`mutants-1a.toml`，9 個，都抓到）：第一輪 10 個，4 個抓到、2 個卡住（最上層永遠是「沒有」、走訪漏掉最上層：頁面等不到結果，測試等到 900 秒的時限），4 個沒抓到。沒抓到的：
- 子 frame commit 新文件之前先拿掉舊的（`drop_document`）是等價的：取消舊文件的 `fetch()` 看的是 DOM 裡文件被丟掉了沒，不讀這張表；拿掉了這一步和這個 mutant。
- 最上層 commit 時不清掉舊的 frame、frame 不見時不拿掉、子 frame 的走訪混進最上層：畫面和行為看不出來（過期的項目在下一次 `follow_child_frames` 清掉，或讀它的地方在最上層擋著畫面時不跑），守的是表自己的規則；補上面兩個單元測試之後都抓到。

### 原本就有的不穩定的測試：`webdriver::a_session_with_a_hung_page_ends_in_time`（2026-11-25，Linux：Fedora 45，debug build）

不是這份造成的，之後另外查。

- **怎麼碰到的**：1a 的 `cargo test -p renderer -p shell` 有一次失敗。那次整套 `shell` 跑了 69 秒，平常是 12 秒，機器很忙。單獨跑、沒有負載時都會過。
- **怎麼重現**：每個 CPU 跑一個空迴圈，同時跑 `cargo test -p shell --test it webdriver::`。1a 的程式 15 次失敗 11 次；沒有 1a 的程式（`65f6530a`，`git stash`）8 次失敗 7 次。
- **失敗在哪**：測試送給 machopodriver 的一個 HTTP 請求 60 秒內沒有回應，讀回應時逾時（`webdriver.rs` 的 `raw`，`read_to_string` 是 `WouldBlock`）。不是「刪 session 要在 4 秒內」那個斷言。還不知道是哪一個請求（建 session、navigate 還是刪 session）。
- **猜錯的**：以為頁面的 timer（100 ms 之後無窮迴圈）比 `load` 先觸發，navigate 等不到 `load`。改成 `load` 之後才開始迴圈，照樣 8 次失敗 7 次，所以改回原樣。
- **現在的猜測，還沒驗證**：navigate 在 `load` 之後還要 renderer 回答一次（網址、readiness 或一個畫面）。機器忙的時候 renderer 先進了迴圈，那個問題沒人回答，navigate 等到 `pageLoad` 的 300 秒時限，測試的 HTTP 60 秒就放棄了。如果是這樣，就是 machopodriver 這條路的問題：Chrome 的 navigate 收到 renderer 送的「載入完成」就回。
- **之後怎麼查**：在負載下加 log 或 backtrace，看是哪一個請求沒回、machopodriver 和 Browser 在等什麼。
- **做錯的**：查的時候兩次用 `pkill -f`、`pgrep -f` 砍負載的迴圈，樣式也比對到那條指令自己，指令被砍掉（一次是 `git stash` 沒做，一次是測試頁沒改回）；兩次都確認過狀態再重做，沒有丟掉改動。
