# 文件怎麼維護

這份說明每份文件放什麼、什麼時候改，以及一份 RFC 做完時要怎麼更新。原則只有一條：**每件事只寫在一個地方，其他地方指過去**，這樣改的時候只要改一處，不會有兩份互相矛盾的說明。

## 每份文件放什麼

| 文件 | 放什麼 | 不放什麼 |
| --- | --- | --- |
| `docs/architecture.md` | 目前的架構：process 和 thread、rendering pipeline、各個 crate 和對應的 Chromium 元件、第三方 crate 的選型和理由、刻意和 Chromium 不同的地方 | 設計的來龍去脈（寫在 RFC）、API（寫在 crate 的 README） |
| `docs/limitations.md` | **目前仍然成立**的已知限制，依領域分組 | 已經解決的、當時發生了什麼 |
| `docs/testing.md` | 測試的種類和跑法、CI、快取和增量的交叉檢查、mutation test 的做法、工具和跑法、測試和建置用的環境變數 | 某次工作的測試結果和數字、守不住的地方（`docs/test-gaps.md`）、效能的量測（`docs/benchmarks.md`） |
| `docs/test-gaps.md` | **測試守不住的地方**：只靠審查、只做過手動確認、缺少的工具、還沒補的測試空缺、沒有計數守著的優化（各附之後怎麼處理），等價的 mutation。只放目前仍然成立的 | 解決了的、某次工作當時發生了什麼 |
| `docs/benchmarks.md` | 效能的量測：benchmark 工具（`bench.py`）的用法、雜訊、怎麼讀數字、比較兩個版本 | 某次量到的數字（寫在那份 RFC 的 `progress.md`） |
| `docs/performance-gaps.md` | 和 Chrome 的效能差在哪：最近一次分析的快照（日期、量法、各類工作多花多少、最熱的函式、對應的 blink_perf 頁）和可以開 RFC 的方向。重量時整份照新的結果改寫；某個方向做完時重量、改寫那一節 | 每次量測的紀錄（寫在 Speedometer 量測工具那份 RFC 的 `progress.md`）、怎麼量（`docs/benchmarks.md`） |
| `rfcs/` 底下的 RFC（每份一個資料夾，範本是 `rfcs/0000-template/`） | 一個功能或一個要先比較選項的設計：目標、比較過的做法和理由、架構、已決定的事項。`README.md` 是 RFC 本身，`details.md` 放細節，開始實作之後加上 `progress.md`。流程、狀態和索引見 `rfcs/README.md` | `README.md` 和 `details.md` 不放進度和測試結果 |
| RFC 的 `progress.md` | 實作的進度：每一步的狀態、驗收（測了什麼、怎麼驗證）、做過的檢查（mutation test、手動確認、找到的 bug、不穩定的測試）、測試數字 | 設計理由、目前仍然成立的限制（寫在 `docs/limitations.md`） |
| `rfcs/pre-rfcs/` | MVP 時期的規劃、各階段的設計和進度。2026-10-04 起凍結，不再更新內容（見它的 README）；它的路徑和測試指令是當時的樣子，`sanity.py docs` 不檢查，只剩壞掉的章節引用要修 | 任何新的東西 |
| `crates/*/README.md` | 那個 crate 的 API、使用規則、怎麼修改、測試怎麼跑；各子系統怎麼運作也寫在這裡。`crates/shell/README.md` 另外放視窗裡的鍵盤和滑鼠操作 | 進度 |
| `crates/bindings/soundness.md` | `bindings` 的安全論證（DOM 的 GC 的在 elise 的 `docs/`）：假設、不變式、找到的漏洞、mutation test、人工審查清單 | API 教學 |
| `../third_party/*/README.machopo.md` | 放在 machopo 旁邊、各自是 git repo 的第三方專案裡，上游的 fork（`machopo` 分支；`../third_party/wpt` 是上游本身、不改，沒有這個檔，見 `tests/wpt/README.md`）：上游和分支的起點、為什麼 fork、本地的修改（`machopo` 分支上的 commit；parley 和 taffy 的另外標明上游的狀態，每個也是 machopo 的 `patches/<名稱>/` 底下的 patch）、怎麼更新。用英文 | 設計理由（寫在對應的 RFC） |
| repo 根目錄的 `README.md` | 入口，英文：這是什麼、從哪幾份文件讀起、怎麼放第三方的專案、怎麼 build 和跑、常用的測試指令、哪些部分用哪一種授權（條文在 `LICENSE-MIT`、`LICENSE-APACHE`） | 設計、細節（寫在各自的文件） |
| repo 根目錄的 `CLAUDE.md` | 給 AI 助手的工作約定：先讀哪些文件、怎麼做決定、不要做的事、完成前要跑的檢查、fork 的規則。內容盡量指向其他文件；在 repo 裡，`sanity.py docs` 也檢查它（它寫到的 `../third_party/rusty_v8_artifacts` 是 `ci.py v8` 在本機產生的，不算錯） | 設計、進度、規則的完整內容（寫在它指向的文件裡） |
| `tests/*/README.md`、`resources/*/README.md` | 測試和資源的格式、來源、固定的 commit、license、怎麼更新 | 結果統計以外的進度 |
| `machopo --help`、`scripts/*.py -h` | 命令列選項和會影響程式的環境變數。**這是唯一的來源**，文件只寫「見 `--help`」 | |

其他規則：
- **測試數字只寫在 RFC 的 `progress.md`**（例如「3267 個通過」），其他文件寫「結果見那份 RFC 的 `progress.md`」。例外是 `tests/wpt/README.md` 記錄挑選測試時的數字（匯入時的紀錄，不是目前的結果），以及凍結的 `rfcs/pre-rfcs/` 裡的進度。
- **日期寫絕對日期**（2026-09-30），不寫「上週」「最近」。
- **RFC 的引用只放在 `rfcs/`**：`rfcs/` 以外的檔案（`docs/`、crate 的 README、程式碼和註解、`tests/`、`resources/`、`scripts/`）只寫現在的行為和它的理由，不寫是哪一份 RFC 做的、哪一份會做：不寫 RFC 的編號、`rfcs/` 底下的路徑、某份 RFC 的第幾條或第幾段，也不寫 MVP 時期的文件和階段代號。
  - 帶著理由的，把理由用一兩句寫在原處；理由太長的寫結論，細節留在 RFC。
  - 之後才做的事寫「之後」，不寫哪一份 RFC 會做；只是來歷（哪次工作加的）就不寫。
  - 反方向由 RFC 指過去：RFC 的「動機」列出它要解決的 `docs/limitations.md` 裡的限制，寫出它改到的檔案和模組；做完時刪掉那一條限制，不必在其他文件裡找它的編號。要找某個檔案的來歷，在 `rfcs/` 裡 grep 它的路徑，或看 `git log`。
  - 例外是講流程和導覽的這份、`CLAUDE.md` 和根目錄的 `README.md`：可以寫 `rfcs/`、`rfcs/README.md`、`rfcs/0000-template/`、`rfcs/pre-rfcs/` 這幾個不會過時的路徑。
  - 為什麼：RFC 會延後、拆開、改範圍，做完時也會改寫，指向它的引用和預告就跟著過時，要一處處找出來改；MVP 時期沒排的預告也是到改了範圍、到里程碑才一起發現過時。現況文件寫的是現在，來歷對讀的人沒有用。
- **程式碼的註解也不引用 `docs/` 的文件**，要解釋的直接寫在註解裡；文件可以引用程式碼。
- **沿用每份文件原本的語言**：`docs/`、`rfcs/` 和 crate 的 README 用中文，根目錄的 `README.md`（repo 的入口）和 `tests/` 底下的 README 用英文。
- **機械式的規則有腳本檢查**：`python scripts/sanity.py docs` 檢查日期的寫法、測試數字的位置、`rfcs/` 以外有沒有引用 RFC 和 MVP 的文件與階段、程式碼註解有沒有引用 `docs/`，以及文件裡寫的路徑、`cargo test` 指令（`--test it <模組>::` 的模組也檢查）和「見『某一節』」是否存在；已完成的 RFC 和 `rfcs/pre-rfcs/` 的路徑和測試指令記的是當時的樣子，不檢查。它只能確定這些名字存在；寫的行為對不對，還是要讀。

## 做到一半、有設計改變時

實作常常和規劃不同。發現時就更新 RFC，不要等到做完：
- **改原本的段落，不要在後面追加一段新的**。追加會讓舊的規劃文字和新的做法同時存在、互相矛盾。
- **留下理由**：寫出原本打算怎麼做、為什麼改掉。例如「原本打算用 parley 的 `WhiteSpaceCollapse`，但它會在每個 span 的頭尾各自 trim，所以改成自己處理」。以後有人想改回去時，才知道那條路試過了。
- **找到的 bug 或反例**，連同是哪個測試抓到的，寫進對應的設計段落。

## 完成一步時

RFC 的「分階段」裡的一步（或再細分的一段）做完時。那份 RFC 的 `progress.md`：
1. 步驟的狀態。
2. 驗收加上新的項目：「測了什麼」和「怎麼驗證」（可以直接執行的指令）。
3. 「做過的檢查」加上 mutation test 的結果：弄壞了什麼、哪個測試失敗了。如果有沒抓到的，也要寫，並說明原因。沒抓到、又補不了測試的（不是等價的），加進 `docs/test-gaps.md` 的「測試守不住的地方」，寫明為什麼測不到、現在靠什麼守住、之後怎麼處理；等價的加進那份文件最後的「等價、不用處理的 mutation」。新增只靠手動確認或只靠審查的東西時也一樣。

`docs/` 底下的現況文件：

4. `docs/limitations.md`：新的限制加進去；**被這一步解決的舊限制刪掉**。`docs/test-gaps.md` 的「測試守不住的地方」裡被這一步解決的項目（補了測試、加了工具）也刪掉。新增或拿掉 crate、thread 時更新 `docs/architecture.md`。
5. **在視窗裡看得到新的行為**：在 `about:demo`（`resources/demo.html`，視窗預設開的頁面）加上示範這一步的東西，例如「Busy for 3 seconds」按鈕（點了之後 script 忙等 3 秒，期間照樣能捲）。頁面維持英文，新增的地方標明它示範什麼。然後把「在視窗裡做什麼、應該看到什麼」寫成情境，或補進現有的一份（`tests/scenarios/`，格式見它的 README）：`python3 scripts/test.py scenarios` 在沒有平台視窗的瀏覽器視窗裡跑它（`cargo test --workspace` 跳過情境：寫了或改了情境時用 `python3 scripts/test.py scenarios <名稱>` 跑那幾份，CI 每天跑全部）。純重構、畫面上看不出差別時，現有的情境就是回歸檢查。
   - **動到視窗那一層時**（`CLAUDE.md` 列出的檔案）再跑 `python3 scripts/test.py automation`：同樣的情境在真的視窗（winit、wgpu）裡跑；看 `target/automation-results/` 的截圖，結果和日期記進 `progress.md` 的「做過的檢查」。
   - **人開視窗確認**只在情境做不到的地方：輸入法、剪貼簿、游標的樣子、外觀和「順不順」（`docs/test-gaps.md` 的「只做過手動確認」），或使用者要求時。

其他文件裡寫到這次改動的地方，當下就改，不要留到最後：

6. **找出寫到它的地方**：這次改名、搬家或改了行為的型別、函式、訊息、檔案、測試和指令，用 grep 在所有文件裡找一遍（例如 `grep -rn "browser::loader" README.md docs rfcs crates tests resources ../third_party/*/README.machopo.md`），包括其他 RFC、crate 的 README、`machopo --help` 和程式碼註解。只改描述已經不對的句子；`rfcs/pre-rfcs/` 是凍結的，已完成的 RFC 基本上也不再更新（`rfcs/README.md` 的「流程」第 6 步），都不用改，除非 `sanity.py docs` 抓到壞掉的引用（它不檢查這些文件裡的路徑和測試指令），或有特殊要求。
7. **跑 `python scripts/sanity.py docs`**。

## 一份 RFC 做完時

依序做：

1. **驗證**：在 `rust-toolchain.toml` 釘住的 toolchain 上跑 build、fmt、clippy、`cargo test --workspace`；整套的 `wpt`、`wdspec`、`reftest`、`scenarios` 由每天的 CI 跑，`matrix`、`fuzz` 由每週的 CI 跑（`docs/testing.md` 的「CI」），動到 GC 的 RFC 在 merge 之後手動開始一次 `matrix`、`fuzz`、看它們的結果，不在開發機上跑；動到 GC 或 `ComputedStyle` 時也跑 `python scripts/sanity.py unsafe`，動到 crate 的依賴或 V8 的程式時跑 `deps`。記下日期和平台。
2. **`progress.md`**：開頭寫上完成的日期和平台；把各步驟的段落**併成一段**：一份驗收表（依領域分組）、一份濃縮的「做過的檢查」。
3. **RFC 改寫成完成時的樣子**（`rfcs/README.md` 的「流程」第 6 步）：狀態改成「已完成」；每一節直接描述最後的設計，把進行中追加的內容併進正文，拿掉已經不成立的中間狀態；保留設計決定的理由和踩過的坑；驗收結果指向 `progress.md`，限制寫在 `docs/limitations.md`。
4. **`docs/` 的現況文件**：
   - `docs/limitations.md`、`docs/test-gaps.md` 的「測試守不住的地方」：逐項確認還成立，已經解決的刪掉（這份 RFC 要解決哪些，看它的「動機」；其他文件不寫 RFC 的編號，不用 grep 編號），「之後怎麼處理」寫到的時機已經到了的，排進下一份 RFC 或改寫；
   - `docs/architecture.md`：新的 crate、thread，過時的說明。
5. **跟著程式改變的文件**：
   - crate 的 README（API、規則、測試清單）；
   - `crates/shell/README.md` 的「鍵盤和滑鼠」（改了快捷鍵或 input 的分配時）；
   - `soundness.md`（新的 `unsafe` 的論證、新的 mutation test、新的假設）；
   - `tests/*/README.md`（新匯入的測試、固定的 commit）；
   - `machopo --help` 和 `docs/testing.md` 的「測試和建置用的環境變數」（`machopo` 的單元測試會檢查 help 裡有沒有每個選項和變數）。
6. **檢查一致性，範圍是這份 RFC 改過的東西**，不必讀全部的文件：
   - 跑 `python scripts/sanity.py docs`；
   - 用 `git diff --stat <開始實作前的 commit>..HEAD` 和 `git log` 列出改過的檔案和名稱，只核對寫到它們的段落（可以交給一個唯讀的 agent 做），重點是行為的描述、數字和範圍；
   - 每一步都照「完成一步時」的第 6 步改過的話，這一步應該只剩少數幾處。
7. **merge 回 `main`**：上面都做完、經過審核之後，才把那份 RFC 的 branch merge 回 `main`，一律 fast-forward（branch 和 merge 的規則見 `rfcs/README.md` 的「流程」；之前做完的階段可以先 merge，見那裡的「階段的 merge」）。
8. **`cargo clean`**：清掉這份 RFC 的重編和 mutation test 在 `target/` 留下的舊輸出（`docs/testing.md` 的「工具和跑法」）。

## 其他會牽動文件的變更

- **調整 Rust 版本**：`rust-toolchain.toml` 和 workspace 的 `rust-version` 一起調；elise 釘著同樣的版本，一起調（它的 compile-fail 預期輸出用 `TRYBUILD=overwrite` 重新產生，並檢查 diff）。釘住 toolchain，測的就是 `rust-version` 宣稱支援的版本（只寫 `rust-version`、實際用 rustup 預設的 nightly 編，最低版本就從來沒驗證過）；compile-fail 的預期輸出要確認每個檔案的錯誤種類和位置沒變，只有措辭和排版不同。
- **升級 `v8` crate**：先把 `../third_party/rusty_v8` 的 `machopo` 分支 rebase 到新的版本（見它的 `README.machopo.md`），跑 `python scripts/ci.py v8` 在本機重編 V8、設 `RUSTY_V8_ARCHIVE` 測過，再照 `crates/bindings/soundness.md` 的「需要人工審查的地方」重新確認假設；最後在 fork 推新的 release tag，把 `.cargo/config.toml` 的 `RUSTY_V8_MIRROR_TAG` 換過去、拿掉 `third_party.toml` 的 `[rusty_v8]` 的 `rev`，跑 `python scripts/ci.py third-party` 確認 `../third_party/rusty_v8` 在同一個 tag。
- **更新 parley 或 taffy 的 fork**：在 `../third_party/parley` 或 `../third_party/taffy` 的 `machopo` 分支 commit 並 push，在同一次改動裡把 `third_party.toml` 的 `rev` 換成那個 commit（只改 `README.machopo.md` 的也一樣；rebase 到上游的新 commit 時連同 `upstream_rev`），用 `python3 scripts/ci.py patches <名稱>` 重新匯出 `patches/<名稱>/`，先 push fork 再 push machopo。別台機器的 `ci.py third-party` 只拿得到 push 過的 commit，也檢查 patch 和 `rev` 一致。fork 的 `README.machopo.md` 的「Local modifications」跟著改，連同上游的狀態。
- **rusty_v8 的 fork 只改了 Rust**（`.rs`，和 `.md`、`.github/` 的 CI 設定）：不用發新的 release，在 `machopo` 分支 commit 並 push，把 `third_party.toml` 的 `[rusty_v8]` 的 `rev` 換成那個 commit，跑 `python scripts/ci.py third-party` 確認 tag 之後沒有改到 prebuilt 裡的東西。動到 C++、`Cargo.toml` 或 `v8` submodule 時照上一條發新的 release；release 之前可以先在本機 commit、`rev` 指過去，設 `RUSTY_V8_ARCHIVE` 連結本機編的 V8（`ci.py third-party` 這時不檢查 prebuilt），發 release 時換 tag、拿掉 `rev`。
- **換 WPT 的 commit、加跑 WPT 的測試**：照 `tests/wpt/README.md` 的「Updating」做（commit 在 `third_party.toml`），並更新那裡的 commit 和數字。
- **新增或拿掉 crate、thread，改了 crate 之間的分工**：更新 `docs/architecture.md`。
- **新增環境變數或命令列選項**：寫進 `machopo --help`（會影響 `machopo` 的），或 `docs/testing.md` 的「測試和建置用的環境變數」（只在測試或編譯時用的）。
