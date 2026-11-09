# RFC 0075 的進度：Chrome 的 DevTools 連上 kun：CDP 的骨架和 V8 inspector

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-23。完成：2026-10-23，macOS（Apple Silicon；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0075` 上做，四段各一個 commit，加上 fork 的 release（`kun-v152.2.0-18`）和換過去的 commit。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 傳輸和 session：`--remote-debugging-port`、`DevToolsHandlerThread`、HTTP 的 endpoint、websocket、好幾個 client；安全檢查和隨機的 id；訊息的大小上限 | 完成 | 2026-10-23 |
| 2 V8 inspector 和 `Runtime`：轉送、renderer 的 session、context 和 frame 的 id、console、例外；`bench.py` 改用 `Runtime.evaluate`；`test.py cdp` | 完成 | 2026-10-23 |
| 3 `Debugger`、`HeapProfiler`、`Profiler`：DevTools 佇列、巢狀迴圈、`RequestInterrupt`、停住時斷線和關掉分頁；`Profiler` 換成 inspector 的 | 完成 | 2026-10-23 |
| 4 DevTools 前端的相容：錄下前端送的命令，renderer 的 `Page` agent（frame 樹、文件的原始碼、frame 的事件） | 完成 | 2026-10-23 |
| fork 的 release `kun-v152.2.0-18`，kun 換過去（第 18、24、28 條） | 完成 | 2026-10-23 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 傳輸 | `/json/version`、`/json/list`、`/json/new`（只收 `PUT`）、`/json/activate`、`/json/close` 照 Chrome；stderr 的 `DevTools listening on …`；`--name=value` 和兩個新的開關 | `cargo test -p shell --test it remote_debugging::`；`cargo test -p shell --bin kun` |
| 傳輸 | 擋住網頁：`Host`、`Origin`（`--remote-allow-origins`、`devtools://devtools`）、`PUT`、猜不到的 id；大小的上限：websocket 的 frame 16 MiB、訊息 64 MiB，pipe 的訊息 64 MiB | `cargo test -p shell --test it remote_debugging::`；`cargo test -p shell --lib devtools_http_handler` |
| session | browser 的 websocket 和 pipe 一樣（flat session、事件）；分頁的 websocket 是那個分頁的 session，分頁關掉時 `Inspector.detached`；好幾個 client 各自的 session 和事件、斷線帶走自己的 session；`Kun` domain 只在自動化模式 | `cargo test -p shell --test it remote_debugging:: windowless::without_a_driver` |
| 轉送 | browser 不處理的命令照原樣交給 renderer，回答原樣送回；命令的 `id` 是 `i32`；沒有 session、renderer 當掉、wasm 的文件的錯誤；renderer 的訊息的檢查（別的分頁的、不是物件、超過 256 MiB）；交給 renderer 還沒回答的命令在 renderer 不在時回答錯誤 | `cargo test -p shell --test it devtools:: windowless::a_renderers_devtools automation_pipe::` |
| `Runtime` | 每個 frame 的 realm 是 context（`frameId`、`origin`），沒指定 context 的命令在 main frame 的；文件丟棄時 `executionContextDestroyed`，main frame 的整組 `executionContextsCleared`；renderer 重開、換 engine、client 斷線之後 session 照舊；UTF-8 | `cargo test -p shell --test it devtools::` |
| console | V8 的 `console`：DevTools 的 `consoleAPICalled`（補送、導覽時清掉）、終端機的 `[console] … (網址:行)`；V8 的輸出的格式 | `cargo test -p shell --test it devtools::`；`cargo test -p dom_js --test it console::` |
| 例外 | `exceptionThrown`（例外本身、位置、晚打開 `Runtime` 時補送）、`exceptionRevoked`（最近 1000 個）；stack 只在 `Runtime` 打開時記 | `cargo test -p shell --test it devtools::` |
| `Debugger` | 停在 `debugger` 和斷點（其他 task 等著、在 call frame 上求值）、單步、`getScriptSource`；`Debugger.pause` 打斷跑著的 script、`terminateExecution`；`Runtime.evaluate` 等 script 跑完；停住時斷線、停住時才連上、停住時關分頁 | `cargo test -p shell --test it devtools::` |
| `Profiler`、`HeapProfiler` | `Profiler` 是 inspector 的（要 `enable`、取樣的間隔、`(idle)` 和 `(program)`）；heap snapshot 裡有 DOM 的物件 | `cargo test -p shell --test it devtools:: automation_pipe::the_profile`；`cargo test -p bindings --test it cpu_profiles` |
| `Page` 的 frame 樹 | frame 的 id、父 frame、`loaderId`、網址、origin、MIME type、`name`、secure context；文件的原始碼（要 `Page.enable`）；`frameAttached`、`frameNavigated`（初始的 `about:blank` 不送；main frame 的在 realm 之前）、`frameDetached`；`Page.disable` 連 browser 的事件一起停；停住時、連上的第一個命令也回答 | `cargo test -p shell --test it devtools::` |
| 和 Chrome 比 | `Runtime.evaluate` 的 RemoteObject、`getProperties`、`callFunctionOn`、console、context、例外、導覽、斷點和單步、frame 樹和 frame 的事件：一樣，或只有附上原因的已知差異 | `python3 scripts/test.py cdp`（本機，不在 CI） |
| bench 的工具 | `bench.py` 對 kun 用 `Runtime.evaluate`，`--profile` 讀 inspector 的 profile | `python3 scripts/bench/blink_perf/blink_perf_test.py`、`speedometer/speedometer_test.py`（不在 CI） |
| 手動 | Chrome 的 `chrome://inspect` 和 `devtools://` 連上 kun 的視窗：Console、Sources、Memory | 每段做完時，使用者（`docs/test-gaps.md` 的「只做過手動確認」） |

## 做過的檢查

- **最後的驗證**（2026-10-23，macOS）：不設 `RUSTY_V8_ARCHIVE`，連結 `kun-v152.2.0-18` 的 prebuilt V8（build script 的紀錄確認 `.a` 和 binding 都從 release 下載）：`python3 scripts/ci.py third-party` 對得上，`cargo test --workspace` 3644 個通過、0 個失敗、3 個 ignored。每段做完時另外跑：`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；WPT 經過 machopodriver 的小目錄（`html/webappapis/timers/`、`console/`、`html/webappapis/scripting`，都照預期）；動到視窗那一層的第 1、2 段跑 `test.py automation`：54/54、`pipe --window` 通過（第 1 段的第一輪 15/54 失敗是螢幕睡了，`caffeinate -d -i` 重跑通過）。
- **測試**：`shell` 的 `remote_debugging`（12 個）、`devtools`（26 個）、`windowless` 的兩個、`automation_pipe` 的 `i32` id 和沒有 id 的錯誤、`Profiler` 的兩個；`devtools_http_handler` 的單元測試 3 個、`main.rs` 的 2 個；`protocol` 的 `Inspector.detached`、`Method::Other`、`Message::Raw`、`i32` 的 id；`bindings` 的 `cpu_profiles`（經過 inspector 的 session）；`dom_js` 的 `console`（V8 的輸出）。
- **Mutation test**：四段各一份，共 100 條，最後都抓到或說明了等價。
  - `mutants-1.toml`（29 條）：「每個 client 都收到 target 的事件」一開始沒抓到（測試的 `pipe_call` 等回答時把事件丟掉），改成留著之後抓到。不列的兩項：server 停下時不等 websocket（排不出時序，`docs/test-gaps.md`）、pipe 一次多讀一個 byte 的 `take(limit)`（行為等價）。
  - `mutants-2.toml`（34 條，33 條抓到）：script 的 id 和例外的位置的三條一開始沒抓到（V8 有 stack 時自己用 stack 頂端的 script id），補了晚打開 `Runtime` 時補送的例外之後抓到；只留最後一個 rejection、opaque origin、換 engine 不接上 session、client 斷線不拿掉 renderer 的 session、rejection 不設上限，補了測試之後抓到。沒抓到的「新的 main frame 的 realm 也清掉」：renderer 一律先丟舊文件、再建新的 realm，做不出這個順序（`docs/test-gaps.md`）。
  - `mutants-3.toml`（16 條）：「renderer 結束時不終止停住的 script」一開始沒抓到（測試的 script 停住之後馬上結束），改成停住之後是無窮迴圈之後抓到。卡住的 mutant 都在幾十秒內由測試的讀取逾時抓到。
  - `mutants-4.toml`（21 條）：「agent 用之前的 frame 回答」一開始沒抓到（client 剛連上、attach 和第一個命令在同一輪時 agent 還沒有 frame），補了 `/json/new` 的分頁的第一個命令之後抓到；一條 mutant 寫錯（等價），改正之後抓到。
- **和 Chrome 比**（`python3 scripts/test.py cdp`，Chrome for Testing 152.0.7977.82）：比出來改的：例外的 `text` 和例外本身（script id、shared cross-origin）、例外的位置（stack 頂端）、promise 的位置、context 的 `origin`、`Runtime.evaluate` 丟出例外時的 stack（只在 `Runtime` 打開時）、導覽時的 `executionContextsCleared`、opaque origin 的 `://`、新的 iframe 的初始 `about:blank` 不送 `frameNavigated`、`getResourceContent` 要先 `Page.enable`、子 frame 的 `name`、`adFrameStatus`、`securityOriginDetails`、`data:` 的 frame 的 secure context。最後 8 個 case 都一樣，或只有已知的差異（`scripts/cdp_compare.py` 的 `KNOWN`）：Chrome 導覽時清兩次 context；Chrome 對換到 `data:` 的 frame 還留著初始的 `about:blank` 的 context；Chrome 的 breakpoint id 開頭多 `br:`；Chrome 的 `frameAttached` 有建立它的 script 的 stack；Chrome 沒有快取的文件給不出原始碼。工具本身改的：記號照排序後的 key 編號、等 `load` 時留著其他事件、frame 物件的 `id` 和 `parentId` 也換成記號、Chrome 的 headless 載入之後列出的子 frame 沒有網址（看起來在別的 process），只比 main frame 和子 frame 的數量。
- **inspector 的成本**（第 13 條；`log` 20000 次的迴圈，7 次的中位數，ms；stderr 導到 `/dev/null`）：

  | 迴圈 | 第 1 段（kun 自己的 `console`） | V8 的，沒有 `Runtime.enable` | V8 的，`Runtime.enable` | Chrome，沒有 `Runtime.enable` | Chrome，`Runtime.enable` |
  | --- | --- | --- | --- | --- | --- |
  | `log('x')` | 26.9 | 35.0 | 100.8 | 27.4 | 103.3 |
  | `log('x', i, {a: i})` | 32.8 | 46.5 | 274.9 | 39.6 | 243.8 |
  | `log('%d items', i)` | 29.9 | 41.7 | 145.6 | 35.5 | 123.3 |

  沒有 DevTools 時比原本慢 30% 到 40%，和 Chrome 差不多（kun 多印到 stderr）；`Runtime.enable` 之後是事件經過 pipe 的成本，和 Chrome 同一個量級。閒著的頁面的 RSS：104.6 MB、105.0 MB。`console` 的引數會被留著：V8 留最近 1000 則訊息，它對引數大小的估計不算陣列的元素——2000 則各帶 50000 個 double 的陣列，GC 之後 heap 從 0.5 MB 變成 382.0 MB，`Runtime.discardConsoleEntries` 之後回到 0.5 MB；Chrome 一模一樣（381.9 MB）。寫進 `docs/limitations.md`。
- **實作時改的設計**：都寫回 `README.md` 的正文；需要確認的是「已決定的事項」第 23 到 27 條（renderer 的訊息上限 256 MiB、多補的三個 binding、stack 交給 inspector 開關、shared cross-origin、console 的 sink 在 engine 上），使用者 2026-10-23 確認。其他：停住時的斷線和結束用 CDP 本身的命令（不改 fork）；停住時才 attach 的 session 等繼續之後；`set_idle` 留著。
- **找到、修好的**：`speedometer_test.py` 的 `test_the_probe_posts_what_kun_lacks` 早就壞了（`<span>` 10-21 起是 `HTMLSpanElement`），探測的元素換成 `<details>`。
- **不穩定的測試**：
  - `speedometer_test.py` 的 `test_profiles_of_both`（不在 CI 裡）：kun 那一側把計時的區間放到 profile 上的誤差上限是 0.07 到 1.51 ms（整套跑 8 次，上限 1.5，1 次超過；第 2 段時 kun 自己的 profiler 在機器忙時也超過過，2.1），Chrome 那一側 0.1 到 0.7 ms；應該是頁面的時鐘函式前後的樣本間隔在機器忙時變大。另一次失敗在 Chrome 那一側（profile 裡沒有 suite 的程式碼）。還沒修：放寬上限或改用 trace 對齊，要另外決定。
  - `devtools::frames_coming_and_going_are_told` 一開始在 `Page.disable` 之後看到 `frameStartedNavigating`：是之前那次導覽的事件留在 client 的緩衝裡，不是 browser 沒有停；改成先讀掉它，連跑 3 次通過。
- **手動確認**（2026-10-23，使用者，macOS，Chrome for Testing 152，看過截圖）：
  - 第 1 段：`chrome://inspect` 找得到 kun 的分頁，DevTools 開得起來。
  - 第 2 段：Console 的訊息補送、運算式和自動完成、從 Console 改頁面、例外和 rejection 劃掉、切 context、重新載入。找到 Console 一開始停在 iframe 的 context（frame 樹，第 22 條）。
  - 第 3 段：`debugger` 停住、Step into、在打開的 script 上設斷點、heap snapshot。找到 Sources 的檔案樹和 `Cmd + P` 是空的、screencast 是黑的。
  - 第 4 段：Console 一打開就在 `top`，選單是 `top` 和 `child.html`；Sources 的 Page 樹列出頁面和子 frame，打開是整份 HTML；在 HTML 裡設的斷點停得住，Scope 和 Call Stack 都對。前面找到的 frame 樹的問題都解決了。
- **錄下 DevTools 送的命令**（第 4 段）：在 DevTools 和 kun 之間放一個記錄 websocket 訊息的轉送（暫時的，沒有進 repo），點過 Elements、Console、Sources、Network、Memory、Application：87 個命令、62 種，15 種成功、47 種 -32601、沒有卡住或其他錯誤（`README.md` 的第 8 節）。
- **fork 的 release**（第 28 條）：fork 的 `kun` 分支 commit `9bc01c9`（第 2 段補的五個 binding、第 3 段拿掉的 `CpuProfiler`；前面的 `b9667c5` 是別台機器推的 rustfmt 格式調整，一起進了 release），用 fork 自己釘的 toolchain（1.91.0）`cargo check` 過，push，打 `kun-v152.2.0-18`；release 的 workflow（run `39933270022`）6 個 job 都成功。kun 的 `RUSTY_V8_MIRROR_TAG` 換過去，`third_party.toml` 拿掉 `rev`。已經通知其他機器（使用者）。在那之前（第 2 到 4 段）只重編本機的 release V8（`ci.py v8 --profile release`，每次增量約 10 秒），用 `RUSTY_V8_ARCHIVE` 連結。
