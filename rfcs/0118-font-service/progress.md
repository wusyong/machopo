# RFC 0118 的進度：字型服務（系統字型由 Browser 給 renderer）

開始日期：2026-11-27。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、量測、不改 fork 的原型、整台機器的比對（`details.md`）、探測之前的四條決定 | 做完 | 2026-11-27 |
| 1a. 第 1 段的三條決定（已決定的事項第 5 到 7 條） | 做完 | 2026-11-27 |
| 1b. `ipc`：開著的檔案是訊息的值裡的欄位（codec 的 `Place`、`Resolve`），收的一側 map 成唯讀的位元組；Windows 的通道把檔案的 handle 交給 child；測試、README、`soundness.md` | 做完 | 2026-11-27 |
| 1c. 第 1 段的檢查和文件 | 做完（在 Windows 上；macOS 和 Linux 的平台層這一段不交檔案，只多了回 `None` 的兩個函式） | 2026-11-27 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 第 1 段：Browser 開的檔案跟著訊息交給 child，child map 成唯讀的位元組；map 著的期間沒有人寫得了它 | `crates/ipc/tests/it/file_in_message.rs`：同一個 process 裡的端點；Windows 上真的 child（`a_child_maps_the_files_its_host_sends_and_nobody_writes_them`） |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 1b、1c（2026-11-27，Windows 11 10.0.26300）

- **新的測試**（`crates/ipc/tests/it/file_in_message.rs`，11 個，其中 3 個只在 Windows；另有 Windows 的 child 的入口）：值裡各種位置的檔案照順序附上、解回來各是自己的檔案、送的一方的還在；別的序列化器送不出、讀不出，被拒絕之後不留著 handle；不附 handle 的一端、附不了那麼多的，值送不出去；號碼不是這則訊息的、提兩次的是 `UnknownHandle`；長度 0 的檔案是空的 mapping；Windows 上開著和 map 著的檔案沒有人能開來寫、但能改名；同一個 process 裡的端點：host 送、child 收，child 送不出，超過 16 個整則不送、通道照常，假的 host 把 region 當成檔案是壞訊息；Windows 上真的 child 收到檔案、map，host 放掉自己的之後 child 的還擋著寫入，child 結束之後才寫得了。
- **改了的測試**：`endpoint.rs` 原本的 `only_the_cores_own_messages_come_with_handles`（給上層的訊息帶 handle 是壞訊息）改成 `a_hosts_handles_that_a_value_doesnt_take_go_with_the_message`：這一段刻意改了這條規則，值沒拿走的 handle 跟著訊息關掉。`local` 的 handle 從 region 改成 region 或檔案（`local::Handle`），測試和 fuzz target 跟著改。
- **Mutation test**（`mutants-1.toml`，11 個，全部抓到）：開檔時讓別人寫、不讓改名、別的序列化器被拒絕之後留著 handle、codec 每個檔案都寫第一個 handle、每個號碼都拿第一個 handle、送的時候丟掉 handle、交給上層時丟掉 handle、給上層的訊息帶 handle 還是拒絕、不是檔案的 handle 當成檔案、Windows 的 host 不交、Windows 的 child 不收。第一輪漏了兩個：「長度 0 的檔案也 map」是等價的（memmap2 自己把長度 0 當成空的 mapping），所以把那段特例拿掉了；「被拒絕之後留著 handle」原本的測試看不到（下一次序列化會蓋掉它），補了 `a_file_another_serializer_was_refused_is_left_open_nowhere`。
- **跑過的檢查**：`cargo test -p ipc`（單元 103、整合 114，1 個 ignored）；`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs；第 1 段做完的 `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（63 個 binary，4981 個通過，2 個 ignored，沒有失敗）。
- **沒列成 mutant 的（等價）**：`Placing::file` 的「不能附 handle 的一端不附」（拿掉之後核心照樣拒絕整則訊息，`HandlesNotAllowed`，送的一方一樣是 `Refused`）；`map` 的「不是一般的檔案不 map」（目錄 map 不了，裝置的長度是 0，結果都一樣；留著當成明說的檢查）；codec 解碼之後清掉 thread-local（只有 `ReadOnlyFile` 用這個名字，它的 visitor 一定拿走）。
