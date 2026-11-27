# RFC 0118 的進度：字型服務（系統字型由 Browser 給 renderer）

開始日期：2026-11-27。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、量測、不改 fork 的原型、整台機器的比對（`details.md`）、探測之前的四條決定 | 做完 | 2026-11-27 |
| 1a. 第 1 段的三條決定（已決定的事項第 5 到 7 條） | 做完 | 2026-11-27 |
| 1b. `ipc`：開著的檔案是訊息的值裡的欄位（codec 的 `Place`、`Resolve`），收的一側 map 成唯讀的位元組；Windows 的通道把檔案的 handle 交給 child；測試、README、`soundness.md` | 做完 | 2026-11-27 |
| 1c. 第 1 段的檢查和文件 | 做完（在 Windows 上；macOS 和 Linux 的平台層這一段不交檔案，只多了回 `None` 的兩個函式） | 2026-11-27 |
| 2a. 第 2 段的三條決定（已決定的事項第 8 到 10 條） | 做完 | 2026-11-27 |
| 2b. 協定和 Browser 那一側：名字的清單、family、fallback、檔案四種問答；字型 thread；IO thread 上的檢查 | 做完 | 2026-11-27 |
| 2c. renderer 那一側：`TextContext` 照需要向 Browser 要、名字由 machopo 自己對、文字交給 parley 之前掃 script；三種 renderer 都切過去 | 做完 | 2026-11-27 |
| 2d. 守著的測試（同一組文字，問 Browser 的和直接問系統的排出來的字形相同）；sandbox 裡的 renderer 不再載入 `dwrite.dll` | 做完 | 2026-11-27 |
| 2e. 第 2 段的檢查和文件 | 做完 | 2026-11-27 |
| 4a. 第 4 段的探測和決定（已決定的事項第 12 條） | 做完 | 2026-11-27 |
| 4b. `ipc`：macOS 的通道把檔案包成 fileport 交給 child、child 還原；測試、`soundness.md` | 做完 | 2026-11-27 |
| 4c. macOS 的三種 renderer 切到字型服務；profile 拿掉字型的目錄和 `com.apple.fonts` | 做完 | 2026-11-27 |
| 4d. 第 4 段的檢查和文件 | 做完 | 2026-11-27 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 第 1 段：Browser 開的檔案跟著訊息交給 child，child map 成唯讀的位元組；map 著的期間沒有人寫得了它 | `crates/ipc/tests/it/file_in_message.rs`：同一個 process 裡的端點；Windows 上真的 child（`a_child_maps_the_files_its_host_sends_as_its_own`） |
| 第 2 段：Browser 給的系統字型排出來的字形，和 renderer 自己找到的系統字型的相同（多種 script、generic family、粗細和斜體、其他的名字、找不到的名字；block、SVG、canvas） | `crates/layout/tests/it/font_service.rs` 的 `served_system_fonts_lay_text_out_as_the_systems_own`（Windows）；測試字型在每個平台上的 `served_fonts_lay_text_out_as_found_ones` |
| 第 2 段：sandbox 裡外的 renderer process 都不載入 `dwrite.dll`，畫出來和 Browser 的 thread 上的 renderer 相同 | `crates/shell/tests/it/renderer_sandbox_windows.rs` 的 `a_renderer_process_has_the_systems_fonts_from_its_browser`、`a_sandboxed_renderer_shows_a_page_as_one_outside_does`（加上了 thread 的 renderer） |
| 第 2 段：同一個問題不問第二次；文字交給 parley 之前問好它的 script；檔案每次最多 16 個 | `font_service.rs` 的 `each_family_script_and_file_is_asked_for_once`、`a_texts_scripts_are_asked_for_before_it_is_shaped`、`files_are_asked_for_sixteen_at_a_time` |
| 第 2 段：Browser 不回答沒給過的編號、太多的檔案、不是 script 的 tag，當成壞訊息 | `crates/browser/src/render_process_host.rs` 的 `the_io_thread_refuses_font_questions_of_nothing_it_gave` |
| 第 4 段：macOS 上 Browser 開的檔案包成 fileport 跟著訊息交給 child，child 還原成自己的、唯讀的 descriptor（close-on-exec）；memory entry 和 fileport 互相不認 | `crates/ipc/src/platform/macos/fileport.rs` 的單元測試；`file_in_message.rs` 的真的 child（`a_child_maps_the_files_its_host_sends_as_its_own`，名字已經刪掉的檔案也是） |
| 第 4 段：macOS 上 Browser 給的系統字型排出來的字形，和 renderer 自己用 CoreText 找到的相同 | `crates/layout/tests/it/font_service.rs` 的 `served_system_fonts_lay_text_out_as_the_systems_own`（Windows 和 macOS） |
| 第 4 段：macOS 的 sandbox 裡的 renderer 讀不了任何字型的目錄和檔案、查不到 `com.apple.fonts`，畫出來和 sandbox 外的、Browser 的 thread 上的相同 | `crates/shell/tests/it/renderer_sandbox_macos.rs` 的 `a_renderer_reads_no_font_of_the_systems_nor_reaches_its_font_service`、`a_sandboxed_renderer_shows_a_page_as_one_outside_does` |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 1b、1c（2026-11-27，Windows 11 10.0.26300）

- **新的測試**（`crates/ipc/tests/it/file_in_message.rs`，11 個，其中 3 個只在 Windows；另有 Windows 的 child 的入口）：值裡各種位置的檔案照順序附上、解回來各是自己的檔案、送的一方的還在；別的序列化器送不出、讀不出，被拒絕之後不留著 handle；不附 handle 的一端、附不了那麼多的，值送不出去；號碼不是這則訊息的、提兩次的是 `UnknownHandle`；長度 0 的檔案是空的 mapping；Windows 上開著和 map 著的檔案沒有人能開來寫、但能改名；同一個 process 裡的端點：host 送、child 收，child 送不出，超過 16 個整則不送、通道照常，假的 host 把 region 當成檔案是壞訊息；Windows 上真的 child 收到檔案、map，host 放掉自己的之後 child 的還擋著寫入，child 結束之後才寫得了。
- **改了的測試**：`endpoint.rs` 原本的 `only_the_cores_own_messages_come_with_handles`（給上層的訊息帶 handle 是壞訊息）改成 `a_hosts_handles_that_a_value_doesnt_take_go_with_the_message`：這一段刻意改了這條規則，值沒拿走的 handle 跟著訊息關掉。`local` 的 handle 從 region 改成 region 或檔案（`local::Handle`），測試和 fuzz target 跟著改。
- **Mutation test**（`mutants-1.toml`，11 個，全部抓到）：開檔時讓別人寫、不讓改名、別的序列化器被拒絕之後留著 handle、codec 每個檔案都寫第一個 handle、每個號碼都拿第一個 handle、送的時候丟掉 handle、交給上層時丟掉 handle、給上層的訊息帶 handle 還是拒絕、不是檔案的 handle 當成檔案、Windows 的 host 不交、Windows 的 child 不收。第一輪漏了兩個：「長度 0 的檔案也 map」是等價的（memmap2 自己把長度 0 當成空的 mapping），所以把那段特例拿掉了；「被拒絕之後留著 handle」原本的測試看不到（下一次序列化會蓋掉它），補了 `a_file_another_serializer_was_refused_is_left_open_nowhere`。
- **跑過的檢查**：`cargo test -p ipc`（單元 103、整合 114，1 個 ignored）；`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs；第 1 段做完的 `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（63 個 binary，4981 個通過，2 個 ignored，沒有失敗）。
- **沒列成 mutant 的（等價）**：`Placing::file` 的「不能附 handle 的一端不附」（拿掉之後核心照樣拒絕整則訊息，`HandlesNotAllowed`，送的一方一樣是 `Refused`）；`map` 的「不是一般的檔案不 map」（目錄 map 不了，裝置的長度是 0，結果都一樣；留著當成明說的檢查）；codec 解碼之後清掉 thread-local（只有 `ReadOnlyFile` 用這個名字，它的 visitor 一定拿走）。

### 2b 到 2e（2026-11-27，Windows 11 10.0.26300）

- **新的測試**：`crates/layout/tests/it/font_service.rs`（8 個，其中 1 個只在 Windows）：測試字型由假的 Browser 給、排出來和 `TextContext::new(true)` 的相同；每個 family、script、檔案只問一次（名字不分大小寫）；一段文字的 script 在交給 parley 之前問好（block、SVG、canvas；ASCII 算 Latin，沒有 script 的文字算 Latin，每次加上漢字）；檔案一次最多 16 個，開不了的檔案的字型不算、沒有字型的 family 不再問；名字先照 Browser 的清單、再照集合自己的；generic family 用到時才問；一個 family 的字型照 Browser 的 family 註冊（檔案自己說的是別的 family 也一樣），一個檔案有幾個字型也只問一次；Windows 上 Browser 給的系統字型和 renderer 自己找到的排出來相同（多種 script、generic family、粗細和斜體、Chrome 的其他名字、找不到的名字，block、SVG、canvas）。`browser` 的單元測試 2 個（`render_process_host.rs`）：IO thread 轉過去的問題由字型 thread 直接回答 renderer；沒給過的 family、檔案的編號，太多的檔案、空的清單、不是 tag 的 script 回答沒有、當成壞訊息。`shell` 的 `renderer_sandbox_windows.rs`：sandbox 裡外的 renderer process 都不載入 `dwrite.dll`、拿到 Browser 給的字型（`a_renderer_process_has_the_systems_fonts_from_its_browser`）；sandbox 裡外的比較多比 Browser 的 thread 上的 renderer。
- **Mutation test**（`mutants-2.toml`，38 個，`--keep-going`）：第一輪抓到 36 個。沒抓到的兩個：「單一字型的檔案照它自己說的 family 註冊」（測試字型的檔案說的 family 都和 Browser 的一樣）、「一個檔案有幾個字型就問幾次」（測試字型沒有一個檔案是同一個 family 的幾個字型），補了 `a_familys_faces_are_its_own_whatever_their_files_name`（一個 family 的字型是 Ahem 和 Lato 兩個檔案，Lato 的檔案是兩個字型），重跑這兩個都抓到。檔案都還原了（`git status` 和 `target/mutate/originals/`）。
- **沒列成 mutant 的**：字型 thread 送不出檔案時改送沒有檔案的回答、字型 thread 不在時 IO thread 回答沒有（測試造不出來，記在 `docs/test-gaps.md`）；`--single-process` 的 renderer 也問 Browser（改成自己找，排出來一樣，Browser 的 process 本來就載入 `dwrite.dll`；同上）。
- **跑過的測試**：`cargo test -p layout -p protocol -p renderer -p browser`（browser 39、189；layout 32、490；protocol 7、13；renderer 122，都通過）；`cargo test -p shell --test it renderer_sandbox_windows::`（10 個通過）。
- **第 2 段做完的檢查**：`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs；`cargo fmt --check`；`cargo clippy --workspace --all-targets -- -D warnings`（修了兩處：`SystemFontSource::Found` 放進 `Box`，字型 thread 的 `send` 回傳問題的編號、不回傳整則訊息）；`cargo test --workspace`（63 個 binary，4993 個通過，2 個 ignored，沒有失敗）。fmt 改了換行之後，`mutants-2.toml` 有一個 mutant 對不上，改好、重跑抓到。

### 4a 到 4d（2026-11-27，macOS 26.6.2，Apple Silicon）

- **探測**：`served_system_fonts_lay_text_out_as_the_systems_own` 暫時拿掉 `#[cfg(windows)]` 在 Mac 上跑：Browser 的字型 thread（fontique 的 CoreText 後端）回答的和 renderer 自己找的排出來逐一相同。renderer 的 `Collection` 是 `system_fonts: false`，fontique 不建 CoreText 的後端（讀程式）。之後照第 12 條做，這個測試在 macOS 上也跑。
- **新的測試**：`ipc` 的 `platform::macos::fileport` 單元測試 2 個（還原的 descriptor 是同一個開著的檔案、只能讀、close-on-exec，port 放掉之後還在；region 的 handle 做不出檔案、fileport map 不成 region）；`file_in_message.rs` 的真的 child 在 macOS 上也跑，多送一個名字已經刪掉的檔案。`shell` 的 `renderer_sandbox_macos.rs`：`a_renderer_reads_no_font_of_the_systems_nor_reaches_its_font_service`（兩種字型的 renderer 都讀不了 `/System/Library/Fonts`、`/Library/Fonts`、`FontServices.framework` 的 `Reserved`、系統下載的字型、家目錄的 `Library/Fonts`，查不到 `com.apple.fonts`、`com.apple.FontObjectsServer`；取代原本「讀得到系統字型」的那個），`a_sandboxed_renderer_shows_a_page_as_one_outside_does` 多比 Browser 的 thread 上的 renderer。`renderer_sandbox.rs` 的 profile 的單元測試改成只帶 Browser 的 pid。
- **改了的測試**：`a_child_maps_the_files_its_host_sends_and_nobody_writes_them` 改名成 `a_child_maps_the_files_its_host_sends_as_its_own`（「沒有人寫得了」只在 Windows 上成立，那幾行留在 `#[cfg(windows)]`）。`mutants-2.toml` 的一個 mutant 跟著 `SERVES_SYSTEM_FONTS` 改。
- **Mutation test**（`mutants-4.toml`，7 個，`--keep-going`）：全部抓到：通道不送檔案、不還原檔案、`fileport_makefd` 失敗也當成檔案、拿 -1 做 port、macOS 的 renderer 自己找系統字型（sandbox 裡讀不了字型，排出來和外面的不同）、profile 放行 `com.apple.fonts`、放行 `/System/Library/Fonts`。檔案都還原了（每個 `from` 都在原處）。
- **沒列成 mutant 的**：`make_port` 的「回 0 但名字是 null」的檢查（kernel 成功時一定給名字，造不出來）。
- **手動確認，沒做成**：想從系統的 log 看 sandbox 裡的 renderer 有沒有被拒絕的嘗試（例如 CoreText 連字型服務），但這台機器的 `log show` 連 probe 故意被拒絕的也看不到。守著同一件事的是 sandbox 裡外的比較：renderer 自己叫 CoreText 的話，在 sandbox 裡失敗、外面成功，寬度就不同（上面第五個 mutant 就是這樣抓到的）。
- **中途的一件**：第一次跑 mutation test 時 `ci.py third-party` 停下（`../third_party/rusty_v8` 不在釘的 `92a2314`，有 11/22 留下、沒 commit 的格式變更），使用者處理之後同步，相關的測試重跑過。
- **跑過的檢查**：`cargo test -p ipc -p layout -p browser -p renderer -p protocol -p shell`（ipc 96、121；layout 32、490；browser 39、193；protocol 7、13；renderer 122；shell 759，都通過）；`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs；第 4 段做完的 `cargo fmt --check`（改了兩處換行）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（63 個 binary，4992 個通過，1 個 ignored，沒有失敗）。
