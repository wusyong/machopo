# RFC 0024：RFC 的引用只放在 `rfcs/`

- 開始日期：2026-10-09
- 狀態：✅ 已完成：2026-10-09（三段都在 `0024` 上做完；驗收和做過的檢查見 `progress.md`）
- 細節：`details.md`
- 進度：`progress.md`

## 摘要

`rfcs/` 以外的檔案（`docs/`、crate 的 README、程式碼、`tests/`、`resources/`、根目錄的 README）不再引用 RFC：不寫 RFC 的編號、`rfcs/` 的路徑、某份 RFC 的「第 N 條」「第 N 段」，也不寫 MVP 時期的文件（`pre-rfcs/`、`P3.md`、「（P3.1）」）。那些地方只描述現在的行為和它的理由；來歷、比較過的做法、之後的計畫只在 RFC 裡，由 RFC 指向它改到的檔案。例外只有講流程和導覽的 `CLAUDE.md`、`docs/README.md` 和根目錄的 `README.md`，它們可以指向 `rfcs/README.md` 這幾個不會過時的路徑。`python3 scripts/sanity.py docs` 機械式地檢查，違反時失敗。開始時的 852 處（152 個檔案）在這份 RFC 裡一次遷完。

## 動機

**現況（2026-10-09）**：`rfcs/` 以外有 738 處「RFC NNNN」和 53 處 MVP 的文件的引用（另外還有「（P3.1）」這類階段代號）（分布見 `details.md` 的「現有的引用」）。`docs/README.md` 的規則是「預告只指向 RFC」，程式碼的註解不引用文件，其他地方可以引用。

代價：

- **收尾要回頭找**：RFC 做完時要把它改寫成完成時的樣子（`docs/README.md` 的「一份 RFC 做完時」），其他文件裡指向它中間狀態的引用（「RFC 0021 的第 1b 段」「第 3 段才做」）也要一處處找出來改。RFC 0021 收尾時，`docs/architecture.md`、`docs/testing.md`、`crates/driver/README.md` 都有這種引用。
- **預告會過時**：寫「（RFC 0013）」表示那份 RFC 會解決這個限制；RFC 延後、拆開或改了範圍，所有預告都要跟著改。MVP 時期的「（P5）」「（P6）」就是這樣到里程碑才一起發現（`docs/README.md` 的「每份文件放什麼」最後的其他規則記了這件事）。
- **同一件事寫在兩個地方**：引用旁邊常常順便寫了一段 RFC 的結論，RFC 改了，那一段就不一致。
- **現況文件混進了歷史**：`docs/`、crate 的 README 應該描述現在的狀態，「哪一份 RFC 加的」對讀的人沒有用，要找來歷可以從 RFC 那一側找。

**目標**：

- 規則寫進 `docs/README.md`、`CLAUDE.md`、`rfcs/README.md`，`sanity.py docs` 檢查，違反時失敗。
- 現有的引用全部遷完：帶著理由的把理由寫在原處，預告改成「之後」，只是來歷的刪掉。
- 「一份 RFC 做完時」不再需要找其他文件裡的引用。

**不在範圍內**：

- `rfcs/` 裡的內容（RFC 之間互相引用照舊，`pre-rfcs/` 照舊凍結）。
- commit message、branch 的名稱（以 RFC 編號命名的規則照舊）。
- 檢查其他 repo 的檔案：`../third_party/*/README.machopo.md` 照同樣的規則改，但不在檢查裡（「已決定的事項」第 6 條）。
- 文件寫的行為和程式不一致的問題：RFC 0021 收尾時花最多時間的是這個，這條規則省不掉。

## 設計概要

**給寫文件和程式的人**：`rfcs/` 以外，寫現在是什麼、為什麼；不寫是哪一份 RFC 做的、哪一份會做。

| 以前 | 現在 |
| --- | --- |
| `docs/limitations.md`：「沒有 `position`（RFC 0013）。」 | 「沒有 `position`。」RFC 0013 的「動機」列出它要解決這一條 |
| `docs/architecture.md`：「情境的執行器…（RFC 0021 的第 12 條）」 | 直接寫出那條決定的理由，或不寫（架構本身就是現況） |
| 程式碼註解：「// RFC 0021 第 19 條」 | 註解直接寫出理由（「id 本身帶著 document 和節點，renderer 不存表」） |
| WPT 清單的註解：「# RFC 0016」 | 「# CSSOM」：寫功能，不寫 RFC |
| `resources/demo.html` 頁面上的「(dataset, RFC 0017)」 | 「(dataset)」 |

**要找來歷的人**：從 `rfcs/README.md` 的索引找到 RFC，RFC 裡寫了它改了哪些檔案、模組和限制；或在 `rfcs/` 裡 grep 檔案的路徑、`git log` 那個檔案。

**做 RFC 的人**：RFC 要解決的現有限制，列在 RFC 的「動機」或「目標」（反方向的引用）；做完時刪掉 `docs/limitations.md` 裡那一條，不必在其他文件找自己的編號。

## 詳細設計

### 規則

- **範圍**：repo 裡 git 追蹤的文字檔，`rfcs/` 以外。照原樣保存的上游輸出（`4ff8d6e:tests/wpt/baselines/`、`tests/sites/`）和檢查本身（`scripts/sanity.py`，它的程式要寫出 `rfcs/` 底下的路徑）整個檔案排除，不做逐行的白名單（第 4、5 條）。
- **不能出現**（第 4 條）：
  - `RFC` 加以 `0` 開頭的四位數字（`RFC 0013`）：kun 的編號都有前導零，IETF 的 RFC（程式碼裡有 `RFC 4648`、`RFC 6761`）沒有，所以不會誤判；
  - `rfcs/` 開頭的路徑、`pre-rfcs`；
  - MVP 的文件名（`MVP.md`、`P0.md` 到 `P4.md`、`PROGRESS.md`）；
  - MVP 的階段代號：`P0` 到 `P4`，後面可以接 `.數字`（`P3.1`），前後不是英數字或 `_`。程式碼註解裡的「in P1」「since P3」也算。
- **例外**（第 5 條）：`CLAUDE.md`、`docs/README.md` 和根目錄的 `README.md` 可以寫 `rfcs/`、`rfcs/README.md`、`rfcs/0000-template/`、`rfcs/pre-rfcs/`（目錄本身，不含裡面的 `P3.md` 等），說明流程和到哪裡找；不寫某一份 RFC 的編號、MVP 的文件和階段（branch 的名稱「例如 `0013`」不帶 `RFC`，不算引用）。`docs/README.md` 裡原本用 `rfcs/0001-wasm-scripts/`、RFC 0021、P5 當例子的地方，改成了不帶編號的說法。
- **規則寫在哪裡**：`docs/README.md` 的「每份文件放什麼」最後的其他規則（取代「預告只指向 RFC」那一條；「程式碼的註解不引用」留成一條，只管 `docs/`）；`CLAUDE.md` 的「怎麼做決定」指過去；`rfcs/README.md` 的「和 Rust 的 RFC 不同的地方」裡，已決定的事項只讓其他 RFC 引用；範本的「動機」要列出要解決的 `docs/limitations.md` 裡的限制。「一份 RFC 做完時」第 4 步從 RFC 的「動機」找要刪的限制，不用 grep 編號。

### 檢查

`sanity.py docs` 的 `rfc_citations` 讀所有追蹤的文字檔（`git ls-files`，跳過含 NUL 或不是 UTF-8 的檔案、`rfcs/` 和排除的檔案），比對上面「不能出現」的樣式，例外的三個檔案只允許那四個路徑；每一處都是一個問題，錯誤訊息指向 `docs/README.md` 的「每份文件放什麼」。原本程式碼註解的檢查（`DOC_CITATION`）也比對 `rfcs/` 和 MVP 的文件名，那部分交給新的檢查，它只剩 `docs/`。`sanity.py` 整個檔案排除，但它的註解和錯誤訊息照規則寫（不寫 RFC 編號、MVP 的階段）。`sanity.py` 沒有測試的 harness，這個檢查的 fixture 和 mutation test 是這個資料夾的 `citation_check_test.py`（改檢查時手動跑，`docs/test-gaps.md` 的「只做過手動確認」）。

### 遷移

每一處照這三種處理：

1. **帶著理由**（「為什麼這樣做，見 RFC 00xx 第 N 條」）：把理由用一兩句寫在原處；理由太長的，寫結論，細節留在 RFC（RFC 那一側本來就指向這個檔案或模組）。
2. **預告**（「（RFC 0013）」「之後由 RFC 00xx 做」）：改成「之後」；確認那份 RFC 的「動機」或「目標」列了這一條，沒有的補上（`rfcs/` 裡的修改，已完成的 RFC 照 `rfcs/README.md` 的規則不改，只改還沒完成的）。
3. **只是來歷**（「RFC 0017 加的」「（RFC 0021）」）：刪掉。

照檔案分成八批（`docs/` 的三份各一批、`dom_api` 等 crate 和 `sdk/` 的 README、`dom` 的 README 和 `soundness.md`、`tests/wpt/` 和 `scripts/`、`resources/` 和情境、`crates/` 的程式碼和 `Cargo.toml`），各交給一個 agent 照上面三種改寫；根目錄的 `README.md`、`CLAUDE.md` 和 fork 的 `README.machopo.md` 直接改。每批的 diff 逐處審過，寫在原處的理由和 RFC 原文對過，再交給使用者審核，最後合成一個 commit（原本打算每批一個，使用者審核時改成一個）。

遷移時碰到的：

- **沒帶編號、只有對著 RFC 才看得懂的**也一起改：「第 1b 段」「stage 2」「Q15」、路線圖的「S4」「H2」。檢查抓不到這些，只能靠遷移時 grep 和之後的審查。
- **引用拿掉之後才看出過時的說法**，對過程式之後改成現在的樣子（例如 `Cargo.toml` 的「Only block layout in P1」、iframe 情境的「The iframes show nothing yet.」）；沒能確認的列在 `progress.md`。
- **頁面的文字和情境一起改**：`about:demo` 的 script 寫出的文字有 7 處被 `demo-script.txt` 的 `wait-text` 比對。
- **預告**指向還沒完成的 RFC 的，0004、0013、0023 的「動機」加上「要解決的限制」；其他的指向已完成的 RFC（照規則不改）或還沒有自己的 RFC 的路線圖項目，只改成「之後」。

### 測試

- `sanity.py docs` 的檢查：`citation_check_test.py` 的 fixture（每種樣式、IETF 的 RFC、例外的檔案和路徑、排除的目錄、二進位檔、沒有追蹤的檔案）和 20 個 mutant；啟用之後，在追蹤的檔案裡暫時放引用，確認 `sanity.py docs` 失敗。
- 遷移之後：`sanity.py docs` 0 個問題；fmt、clippy、`cargo test --workspace`（程式只改了註解，確認沒有誤改）；`test.py scenarios`、`automation`（頁面的文字和情境）、`reftest`、`wpt`（清單的註解）、`wdspec`、`smoke`（`wptrunner_machopo` 和 wdspec 的清單）。結果見 `progress.md`。

### 分階段

1. **規則和檢查**：改 `docs/README.md`、`CLAUDE.md`、`rfcs/README.md`、範本；`sanity.py docs` 加新的檢查，這一步只回報數量、不失敗。
2. **遷移**：照上面分批改寫，和第 1 段一起一個 commit。
3. **啟用**：新的檢查改成失敗，`DOC_CITATION` 只剩 `docs/`；`sanity.py docs` 0 個問題。

## 缺點

- **從文件跳到理由多一步**：讀 `docs/limitations.md` 的人看不到「哪份 RFC 會解決」，要到 `rfcs/README.md` 的索引找。
- **一次改很多檔案，和其他線衝突**：852 處、169 個檔案。其他線如果在別台裝置上進行，rebase 時會有大量的衝突；這次開始前確認過沒有（「已決定的事項」第 3 條）。
- **遷移要逐處判斷**：不是機械式的取代，帶著理由的要改寫，審核的量不小。
- **檢查可能誤判**：「P3.1」這種樣式可能出現在別的地方（例如 WPT 的輸出、CSS 的值）。2026-10-09 實測只有 `4ff8d6e:tests/wpt/baselines/` 裡的一處，用檔案層級的排除處理；之後再有誤判，也是排除那個檔案或目錄，或改寫那一處。

## 理由和其他做法

2026-10-09 比較過（使用者確認）：

- **全部禁止**（選的）：最乾淨；改 RFC 時完全不用回頭找；可以機械式檢查。
- **只禁止「第 N 段」「第 N 條」**，「（RFC 0021）」這種標出來源的照舊：遷移量小，收尾時省下的主要就是這一塊；但預告照樣會過時，標來源的引用對讀現況的人也沒有用。
- **只禁止文件，程式碼和測試清單照舊**：程式碼的引用通常是「這段是哪次工作加的」，但它們一樣會過時，規則也比較難說清楚。
- **遷移的方式**：一次遷完（選的）；或先立規則、舊的碰到再改（成本分散，但兩種寫法會並存很久，檢查要有白名單）。

## 先例

- **Chromium**：程式碼的註解常引用 bug（`crbug.com/123`、`TODO(crbug.com/…)`），設計文件放在 repo 外面（Google Docs），程式碼裡偶爾有連結。和這份相反：Chromium 靠 bug tracker 串起來，kun 沒有 tracker，RFC 就是設計的唯一來源，所以由 RFC 單向指向程式。
- **Rust**：RFC 在獨立的 repo，合併之後不改；編譯器的程式碼引用 issue 和 tracking issue。kun 的 RFC 會跟著實作改寫（`rfcs/README.md` 的「和 Rust 的 RFC 不同的地方」），被引用的內容會變，引用更容易過時。

## 已決定的事項

2026-10-09 確認：

1. **`rfcs/` 以外全部禁止引用 RFC**：RFC 的編號、`rfcs/` 的路徑、「第 N 條」「第 N 段」、`pre-rfcs/` 和 MVP 的文件與階段；例外只有講流程的 `CLAUDE.md`、`docs/README.md`，可以指向 `rfcs/README.md`（第 5 條加上根目錄的 `README.md`）。`sanity.py docs` 檢查。不選只禁止階段和條號、不選只管文件（「理由和其他做法」）。
2. **開這份 RFC 一次遷完**：先改規則和檢查，再分批（`docs/`、crate 的 README、`tests/`、程式碼、`resources/`）改寫、逐批審核，最後讓檢查擋住新增的。不選先立規則、舊的碰到再改。
3. **現在開始**（原本的待決定 A）：`origin` 上沒有 RFC 0016、0023 的 branch，使用者確認其他裝置上沒有進行中的 branch，開 `0024`、第 2 段一次遷完。不選先做第 1 段、等其他線 merge 完再遷移（兩種寫法並存的時間變長，好處只在有其他線時才有）。
4. **比對的樣式寬鬆、誤判用檔案層級排除**（原本的待決定 B）：RFC 的編號比對 `RFC 0\d{3}`，前導零避開 IETF 的 RFC；階段代號比對前後不是英數字的 `P[0-4]`（可以接 `.數字`），程式碼註解裡的「in P1」也算；照原樣保存的 `4ff8d6e:tests/wpt/baselines/`、`tests/sites/` 整個排除，不做逐行的白名單。和 Chromium 的 `PRESUBMIT.py` 一樣：禁止的寫法用 regex、誤判用 `excluded_paths`。不選只比對「（P3）」「P3.1」這種明顯的形式（程式碼註解裡的大多數抓不到，要靠遷移時人工找，之後新增的也擋不住）。
5. **例外的檔案和路徑**（原本的待決定 C）：`CLAUDE.md`、`docs/README.md` 和根目錄的 `README.md` 可以寫 `rfcs/`、`rfcs/README.md`、`rfcs/0000-template/`、`rfcs/pre-rfcs/`（目錄本身）；`scripts/sanity.py` 是檢查本身，程式要寫出 `rfcs/` 底下的路徑，整個檔案排除。根目錄的 README 用這幾個路徑導覽，它們不會過時。不選照草稿只例外兩個檔案（根目錄的 README 要繞一層才找得到 RFC）、不選任何檔案都可以寫這幾個路徑（crate 的 README、程式碼沒有需要導覽到 RFC 的理由，規則也比較難說清楚）。
6. **fork 的 `README.machopo.md` 照同樣的規則改，但不加進檢查**（原本的待決定 D）：`../third_party/*/README.machopo.md` 在其他 repo、用英文，目前只有 `rusty_v8` 的有引用；改了要在 fork 另外 commit、push，再換 `third_party.toml` 的 `rev`，所以檢查（`git ls-files`）只看這個 repo。不選加進檢查（檢查要知道 fork 的位置，fork 沒有 checkout 時又要跳過）。
7. **引用的是 RFC 定下的規則時，規則寫在現況文件**（原本的待決定 E）：例如 `CLAUDE.md` 的「V8 專屬的程式只在 `bindings` 和 `dom_js`」後面接 MVP 文件的理由：規則本身寫在 `docs/architecture.md`（或那個 crate 的 README），理由寫一句，`CLAUDE.md` 指向那裡，不再指向 RFC。

## 待決定

無。

## 之後的可能

- RFC 的「動機」加一個固定的小節「改到的檔案和限制」，讓反方向的引用有固定的位置（這次只在範本的「動機」說明裡要求列出限制）。
- 給 `sanity.py` 一套在 CI 跑的測試，`citation_check_test.py` 搬過去。
- 檢查抓不到的「第 N 段」「stage N」這類不帶編號的引用：要找出不誤判的樣式才能加進檢查。
