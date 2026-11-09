# RFC 0092：taffy 和 parley 的 fork：跟著上游、修改留成 patch、減少 workaround 的成本

- 開始日期：2026-11-10
- 狀態：🚧 已排程：2026-11-10（第 1 段完成；進度見 `progress.md`）
- 細節：`details.md`（workaround 的清單和成本、parley fork 現在的 commit、上游 2026-09-30 之後的變化和進行中的 PR、patch 檔的幾種放法）

## 摘要

taffy 也 fork，和 parley 一樣用 `machopo` 分支、緊跟上游的 main。fork 上的每個 commit 都寫成可以直接送上游的樣子：上游收了就從 fork 拿掉，收不了才在 fork 裡自己維護。每個 commit 另外用 `git format-patch` 匯出成 patch 檔，放在 machopo 的 repo 裡，由腳本檢查它和 fork 的分支一致，升級時照著 patch 重新套上新的上游。parley 的 fork 趁這次重寫：rebase 到上游最新的 main，拿掉上游已經有的，相關的 commit 合在一起。

machopo 這邊只處理一種 workaround：繞在函式庫外面、因此有實際成本的（taffy 的快取看不到 float、`BlockContext` 不能撤回、字型家族的名稱每個 span 重新解析……）。只是換算輸入的 workaround 幾乎沒有成本，和照 Chrome 才有的行為一樣留在 machopo，不搬進 fork。

## 動機

**現況**（2026-11-10）：

- **taffy**：照 RFC 0003 第 14 條不 fork，用 crates.io 的 0.15.0（0.15.1 已經發佈）。它的問題記成限制，或在 `crates/layout` 裡繞過：machopo 在自己的 `BoxTree` 上實作 taffy 的低階 trait，前後的修正都在 `compute_box` / `lay_out_box` 這一層（清單見 `details.md` 的「workaround 的清單」）。
- **taffy 的上游動得很快**：v0.15.0 之後的 main 有 40 個 commit，裡面已經修好 machopo 記成限制的幾件事：裝著 float 的 BFC 根的下方 padding 和 border、BFC 盒子整個高度都避開 float、`ContentSlot` 的高度、replaced 的 block 子項的 min-content。進行中的 PR 還有 last baseline、套在 content box 的 `aspect-ratio`、`fr` 的 track、table（`details.md` 的「taffy 的上游」）。等它發佈才升級，machopo 的 workaround 和限制要多留一陣子。
- **parley**：fork 的 `machopo` 分支從上游 2026-09-30 的 main 分出來，有 16 個 commit，都沒有送上游。`README.machopo.md` 列的 15 條和實際的 commit 對不上：commit 的名稱不同、有只改 README 的 commit、最後一個 commit 把 28 個檔案裡的 `kun:` 改成 `machopo:`。上游之後又多了 66 個 commit，其中有和 fork 重複的（`hanging_advance`，上游 #892）、會衝突的（inline box 改用 U+FFFC，#902），也有讓 machopo 能拿掉 workaround 的（`calculate_content_widths` 算進 `text-indent`，#866、#889）。tab stop（#934）、軟連字號的連字號（#827）、fontconfig 的 metric-compatible 別名（#943）、字型查詢的快取（#796）在上游有進行中的 PR（`details.md` 的「parley 的上游」）。
- **workaround 的成本**：大部分沒有額外成本（取代原本的工作，或只換算輸入）。有成本的集中在幾處：字型家族的清單每個 span 重建、每個名稱重新解析（`docs/performance-gaps.md` 的「每一類最多的函式」裡，文字那一類有 `push_with_metric_compatible` 和 fontique 的 `NameKey::from_str`），float 旁邊的盒子不進 taffy 的快取，`BlockContext` 不能撤回造成的重試，做 fragment 時在最後的寬度再斷一次行（`details.md` 的「有成本的 workaround」）。個別的成本還沒量過。
- **fork 的歷史**：fork 的修改只在 fork 的分支上。rebase 之後，舊的 commit 只剩 machopo 的 `third_party.toml` 釘著的那幾個，沒有分支指著（GitHub 可能清掉）；某個修改以前長什麼樣子、上游的哪個版本改過它，只能從 fork 的 reflog 找。

**目標**：

- taffy 和 parley 都是 fork，緊跟上游的 main；每個 commit 標明它在上游的狀態，能送上游的都寫好草稿（給人審核之後才送出）。
- 每個 fork 的修改也是 machopo 裡的 patch 檔，跟著 machopo 的歷史保存、審核；升級是把 patch 重新套到新的上游上。
- parley 的 fork 重寫成比較少、各自獨立的 commit。
- machopo 裡有成本的 workaround，照成本的順序改成不必繞（搬進 fork 和上游、跟上上游，或在 machopo 裡做得便宜），用計數證明工作量變少。

**要解決的 `docs/limitations.md` 的限制**（跟上上游、或搬進 fork 之後；各段做完時確認）：

- 「Layout」的 float 那一條裡 taffy 的限制：BFC 的根包住 float 時沒有加上下方的 padding 和 border、BFC 盒子只避開頂端那一段的 float（上游的 main 已經修好）。
- 「Layout」的 `text-indent` 那一條：shrink-to-fit 的段落少了縮排的寬度（上游 #866、#889）。
- 「圖片」的尺寸那一條裡 taffy 的部分：自然比例套在 border box 上、`max-width` 是百分比的 block 圖片的 min-content（上游 #1401 已合併；#1347 進行中）。
- 「Layout」的 flex 和 grid 那一條裡上游已經修好或有 PR 的部分（逐項見 `details.md` 的「taffy 的上游」）。

**不在範圍內**：

- 照 Chrome 才有的行為搬進 fork：字型家族的別名、synthetic bold 的規則、挑 baseline 的方式、表單控制項、`-webkit-box`、照 Blink 做的 table。它們是 embedder 的選擇，上游不會收，搬進 fork 只讓 rebase 變難。
- 只是換算輸入、沒有成本的 workaround（overflow 的對應、`Contain::PAINT`、`calc()` 的對照表、grid 的名稱和飽和）：留在 machopo。
- 換成上游的 table（taffy #1380）：machopo 的 table 照 Blink，換掉要另外比較，放在「之後的可能」。
- rusty_v8、wasmtime 和 elise：它們的 fork 照原本的規則，這份 RFC 只改 taffy 和 parley。patch 檔的做法穩定之後可以推廣（「之後的可能」）。
- 量 Speedometer：照 `docs/benchmarks.md`，只在使用者要求時量。成本用計數驗收。

## 設計概要

網頁看到的：跟上上游時修好的限制消失，其他的排版不變（每段的 WPT 預期結果的變化逐項查過）。

改 machopo 的人看到的：

- taffy 的 fork 和 parley 一樣放在 `../third_party` 底下，由 `python3 scripts/ci.py third-party` 同步，`Cargo.toml` 用 path 依賴。
- 要改 taffy 或 parley 的行為：在 fork 的 `machopo` 分支上加一個 commit（附上上游風格的測試），匯出 patch，更新 `third_party.toml`，一起送審。machopo 的審核看得到 patch 檔的 diff，也就是「修改的修改」。
- fork 的 `README.machopo.md` 的「Local modifications」每一條多一欄上游的狀態：上游的 PR 或 issue（草稿、已送出、已合併），或「只在 fork」和理由。
- 升級：用腳本把 patch 依序套到新的上游 commit（`git am -3`），解掉衝突，重新匯出；上游已經收的 patch 拿掉。

## 詳細設計

### fork 和 patch 的規則

- **兩個 fork 都在 `wusyong/` 底下**，`machopo` 分支從上游的 main 分出來；起點寫在 `README.machopo.md` 和 `third_party.toml`。
- **一個修改一個 commit**，寫成可以直接變成上游 PR 的樣子：只做一件事、附上 fork 自己的測試（照上游的寫法），fork 自己釘的 toolchain 也編得過。`machopo:` 註解照舊標出位置。上游 PR 的 branch 從這個 commit 開出來（送出前先給人審核）。
- **patch 檔**：每個 commit 匯出成一個 patch 檔（`git format-patch`，編號照順序），放在 machopo 的 repo 裡，連同上游的起點 commit。fork 的分支一定等於「上游的起點加上這些 patch」，由 `ci.py third-party` 檢查，對不上時停下。放法見「已決定的事項」第 3 條和 `details.md` 的「patch 檔的放法」。
- **跟上游**：在這份 RFC 的每一段開始前 rebase 一次；之後的頻率見「待決定」。rebase 的同時檢查每個 patch 在上游的狀態，上游收了的拿掉，上游有不同做法的改用上游的、在 machopo 這邊配合。
- **舊的分支頂端打 tag**：重寫歷史前，在舊的頂端打一個 tag，讓 machopo 舊的 commit 釘的 rev 還拿得到（`ci.py third-party` 用 commit 的 hash fetch，GitHub 只保證拿得到 branch 或 tag 指得到的 commit）。有了 patch 檔之後，舊的修改的內容也在 machopo 的歷史裡。
- **fork 的測試**：改 fork 時跑 fork 自己的測試（taffy 的 `cargo test`、parley 的 `cargo test --workspace`）和 machopo 相關的測試。這條規則加進 `CLAUDE.md` 的「第三方的 fork」。

### 每個 workaround 放在哪裡

`details.md` 的「workaround 的清單」每一條都標出：成本的種類、建議（跟上上游、搬進 fork 並送上游、在 machopo 裡做得便宜、留著），以及 Chromium 怎麼做。原則：

- **成本來自繞在函式庫外面的**：搬進 fork（同時寫上游的草稿），或在上游已經有的時候跟上。例如 taffy 的快取 key 加上 float 的狀態（Blink 的 layout 快取以 exclusion space 為 key）、`BlockContext` 可以撤回、column flexbox 的 intrinsic 寬度不排子項（Blink 的 `FlexLayoutAlgorithm::ComputeMinMaxSizes`）。
- **成本來自 machopo 自己做法的**：在 machopo 裡改。例如字型家族的清單照 `font-family` 的值快取解析的結果（Blink 照 `FontDescription` 快取 `FontFallbackList`）。
- **沒有成本的換算和照 Chrome 的行為**：留在 machopo。

### 計數

照 `layout` 現有的 `LayoutUpdate::lines_broken` 的做法，給有成本的 workaround 加計數，測試守著次數：

- float 旁邊沒有用 taffy 的快取的盒子（`compute_box`）。
- 做 fragment 時在最後的寬度又斷了一次行（`inline_fragments`）。
- 行在 float 旁邊重試的次數（`break_beside_floats`）。
- 字型家族名稱的解析次數。

第 2 段先用它們找出實際會發生的量，決定後面幾段的順序；之後每一段用它們證明工作量變少。

### 測試

- 第 1 段（taffy fork，起點 v0.15.0）：行為完全不變；`cargo test --workspace`、reftest 和 WPT 的結果和改之前一樣。
- 跟上游的每一段：WPT 的預期結果的變化逐項查過，變好的更新預期結果、刪掉對應的限制；變壞的查清楚是上游的問題還是 machopo 接錯，上游的問題寫成 patch 或上游的草稿。
- 搬進 fork 的修改：fork 自己的測試，加上 machopo 現有的測試（`layout` 的 `float::`、`flex::`、`parley_*::`……）。拿掉 machopo 的 workaround 時，原本守著它的測試照樣要過。
- patch 和 fork 一致：`ci.py third-party` 的檢查；它本身的測試照 `scripts/` 現有的做法。
- mutation test：照 `CLAUDE.md`，每段新的行為（計數、快取、fork 的修改）都跑。

### 分階段

1. **taffy fork 的基礎設施**：`wusyong/taffy` 的 `machopo` 分支，起點是 v0.15.0（和現在用的一樣），`third_party.toml`、`Cargo.toml` 的 path 依賴、`README.machopo.md`；patch 檔的放法和 `ci.py third-party` 的一致性檢查（第 3 條）；`CLAUDE.md`、根目錄的 `README.md`、`docs/architecture.md` 的第三方的選型跟著改。行為不變。
2. **計數**：加上「計數」那一節的計數，量出各個 workaround 實際的量，決定第 5、6 段的順序和範圍。
3. **重寫 parley 的 fork**：舊的頂端打 tag；rebase 到上游最新的 main；拿掉上游已經有的（`hanging_advance`）；把相關的 commit 合在一起（`details.md` 的「parley fork 的重寫」）；commit 帶上 fork 自己的測試；匯出 patch；machopo 拿掉上游已經不需要的 workaround（`text-indent` 的 `indented()`）。可能分成 3a（rebase 和整理）、3b（fork 自己的測試和上游的草稿）。
4. **taffy 跟上上游的 main**：rebase 到 main，處理 API 的變化（例如 `calc()` 的 handle、`ScrollableOverflow`），查 WPT 的變化，拿掉不再需要的 workaround（`float_bands` 自己找一段空間的底），刪掉修好的限制。
5. **machopo 裡有成本的 workaround**：字型家族的清單和名稱解析（看第 3 段之後上游的 #943、#796 的狀態決定做法），做 fragment 時多斷的那一次行。
6. **搬進 taffy 的 fork**：快取的 key 加上 float、`BlockContext` 可以撤回、column flexbox 的 intrinsic 寬度。每一項同時寫上游的草稿。
7. **上游的草稿**：parley 和 taffy 剩下的 patch，各寫一份 PR 或 issue 的草稿（本地，給人審核）。可以和第 3 到 6 段穿插著做。

## 缺點

- **多一個 fork 要跟著 rebase**：taffy 的上游一個月有幾十個 commit，跟 main 代表它的回歸也直接變成 machopo 的，WPT 的預期結果更常變。
- **parley 的重寫是一次性的大工作**：上游多了 66 個 commit，inline box 改用 U+FFFC 和 reshaping 的改動會和 fork 的 inline box、字詞快取衝突。
- **patch 檔和 fork 兩份**：有腳本檢查，但改 fork 時要多一步匯出；大的 patch（字詞快取有兩千多行）讓 machopo 的 diff 變大。
- **送上游要時間**：寫成上游的樣子、補上游風格的測試、等審核，比只在 fork 裡改慢。
- **fork 變大**：有成本的 workaround 搬進 fork，fork 和上游的差異變多，直到上游收下。

## 理由和其他做法

- **不 fork taffy（現在的做法，RFC 0003 第 14 條）**：維護成本最低，但上游修好的要等發佈，繞不過的只能當成限制，繞得過的留在 machopo、帶著成本。上游正在修 machopo 碰到的問題（float、baseline、`aspect-ratio`），跟 main 拿得到它們。
- **fork 但跟 release**：rebase 次數少，但等發佈的時間差就是現在的問題；taffy 0.15.0 到 0.15.1 只隔八天，main 上的 float 修正都還沒發佈。
- **不重寫 parley，照現在的 16 個 commit rebase**：少一次大工作，但重複的 commit、只改 README 的 commit、改名的 commit 每次 rebase 都要再解一次衝突，也不能直接送上游。
- **patch 檔的放法**：只用 fork（現在）、patch 是 machopo 裡的副本、只有 patch 沒有 fork 的分支，比較見 `details.md` 的「patch 檔的放法」。
- **把有成本的 workaround 都搬進 fork**：連沒有成本的換算也搬，machopo 比較乾淨，但 fork 變大、上游不會收的 Chrome 專屬行為永遠留在 fork。

## 先例

- **Chromium**：`third_party/` 的每個專案有 `README.chromium`，其中的 Local Modifications 列出本地的修改；修改多的另外放 `patches/` 底下的 patch 檔（例如 ICU），升級時重新套上。HarfBuzz、Skia 這類照上游優先，由 autoroller 跟著上游，不留本地的修改。照做：上游優先、patch 檔、README 列出修改。
- **Firefox**：`moz.yaml` 寫上游的版本和要套的 patch，`mach vendor` 拿上游、套 patch。照做：上游的起點和 patch 一起記錄。
- **Blitz**（DioxusLabs）：用 taffy 和 parley 的瀏覽器引擎，維護者也在改這兩個上游（taffy 的 float、baseline 修正，parley 的 `hanging_advance`、`text-indent`），所以瀏覽器需要的修改上游收得進去。

## 已決定的事項

2026-11-10 確認：

1. **fork taffy，緊跟上游的 main**（2026-11-10）：推翻 RFC 0003 第 14 條，以及 RFC 0044 照它不 fork 的做法。不選繼續用 crates.io 的版本：上游修好的要等發佈，繞不過的只能當成限制。不選 fork 但跟 release：等發佈的時間差就是現在的問題（「理由和其他做法」）。
2. **上游優先**（2026-11-10）：每個 commit 寫成可以送上游的樣子，標明它在上游的狀態；只在 fork 的要寫理由。送上游前先在本地寫草稿，給人審核。不選只在 fork 裡改：每次 rebase 都要再解一次衝突，差異只會越來越大。
3. **patch 檔是 machopo 裡的副本，fork 的分支照舊**（2026-11-10）：在 fork 上用 git 改、rebase，每個 commit 匯出成 patch 放在 machopo 的 repo，`ci.py third-party` 檢查兩者一致。不選只有 fork：修改的歷史只在 fork 的 reflog，machopo 的審核看不到修改本身。不選只有 patch：送上游的 PR 和 rebase 還是要一個 fork 的分支（比較見 `details.md` 的「patch 檔的放法」）。
4. **taffy 的 fork 先從 v0.15.0 分出來**（2026-11-10）：第 1 段的行為完全不變，跟上 main 是第 4 段。不選一開始就從 main：基礎設施和上游的行為改變會混在同一段，WPT 的變化分不清是哪一邊造成的。
5. **parley 的 fork 這次重寫**（2026-11-10）：rebase 到最新的 main，合併、拿掉 commit，舊的頂端打 tag。不選照現在的 16 個 commit rebase：重複的、只改 README 的和改名的 commit 每次都要再解衝突，也不能直接送上游。
6. **留在 machopo 的**（2026-11-10）：沒有成本的換算和照 Chrome 的行為；只搬繞在外面才有成本的。不選全部搬進 fork：fork 變大，上游不會收的 Chrome 專屬行為會永遠留在 fork。
7. **成本用計數驗收**（2026-11-10）：照 `docs/benchmarks.md`，不排 Speedometer 的量測。
8. **一份 RFC**（2026-11-10）：fork 的規則和清單是共用的。第 5 段的字型家族如果變大，再拆出去。

第 1 段（2026-11-10 確認）：

9. **patch 放在 repo 根目錄的 `patches/<名稱>/`**（2026-11-10）：和 `third_party.toml` 放在一起。不選 `third_party/patches/`：machopo 現在沒有 `third_party/`，加一個容易和旁邊的 `../third_party` 搞混。Chromium 的 patch 放在每個依賴自己的資料夾底下（例如 `third_party/icu/patches/`）；machopo 的依賴不在 repo 裡，所以集中放一個資料夾。
10. **上游的起點記在 `third_party.toml`**（2026-11-10）：有 patch 的 pin 多兩個欄位，`upstream`（上游的 repo）和 `upstream_rev`（起點的 commit）。不選在 patch 的資料夾放一個記起點的檔案：pin 分在兩個地方。
11. **`ci.py patches <名稱>` 匯出，`ci.py third-party` 每次檢查**（2026-11-10）：匯出用 `git format-patch --zero-commit --no-signature`，沒改的 patch 重新匯出時不變。檢查在暫時的 index 上把 patch 套到 `upstream_rev` 的樹上，和 fork 的 rev 的樹比；`test.py` 的每個指令都先跑它，CI 也一樣。不選單獨的 `ci.py patches --check`：會漏跑。升級的輔助指令先不做：在 fork 上 rebase 再匯出就夠了。
12. **parley 在第 1 段就匯出現在的 16 個 commit**（2026-11-10）：第 3 段重寫之後，machopo 的審核看得到重寫前後的 patch 的 diff，重寫前的樣子也留在 machopo 的歷史裡。不選等第 3 段：少匯出一次，但看不到重寫前後的差別。
13. **`README.machopo.md` 留在 fork 裡，是最後一個、只改它的 commit**（2026-11-10）：所以也是最後一個 patch；其他 commit 不碰它，才能直接送上游。和現有的規則、wasmtime 和 rusty_v8 的做法一樣。不選搬到 machopo 的 `patches/<名稱>/`：要改文件的規則，在 GitHub 上看 fork 也看不到說明。parley 現在的 commit 都改了它，第 3 段重寫時照這條分開。
14. **taffy 的 fork 用 machopo 釘的 toolchain 編，送上游的 patch 守住 taffy 的 `rust-version`（1.71）**（2026-11-10）：taffy 沒有 `rust-toolchain.toml`。只在送上游前用 1.71 確認一次（上游的 CI 也檢查），平常不裝舊的 toolchain。

## 待決定

- **跟上游的頻率**：這份 RFC 的每段開始前一次；之後是每月一次、上游合了 machopo 需要的修改時，還是只在有需要時。做完第 4 段、知道一次 rebase 要多少工夫之後決定。
- **parley 的 commit 怎麼合**：`details.md` 的「parley fork 的重寫」是建議的分組，rebase 到最新的 main 之後才知道衝突和上游的 API 改了多少。
- **上游的 tab stop（#934）、軟連字號（#827）合併時**：改用上游的、在 machopo 這邊配合，還是保留 fork 的。看它們合併時的 API 和行為。
- **字型家族的成本怎麼解**：machopo 自己快取、fork 加上接受解析好的 `FamilyId` 的 API，或等上游的 #943、#796。第 2 段的計數和第 3 段之後上游的狀態決定。
- **fork 要不要有自己的 CI**：現在 parley 的 fork 靠 machopo 的測試。帶上自己的測試之後，是在 fork 上開 GitHub Actions，還是由 machopo 的 CI 一起跑。

## 之後的可能

- 換成上游的 table（taffy #1380）：和 machopo 照 Blink 做的 table 比 WPT 的結果、效能，決定要不要換。
- rusty_v8、wasmtime 也改用 patch 檔的做法。
- parley 斷行時複製 `LineState` 的成本（`overflow-wrap: break-word` 每個字元一次）：要改 parley 斷行的核心，看上游的 reshaping 改動（#930）之後再決定。
- `min-*` / `max-*` 的 intrinsic 關鍵字：要改 taffy 的 style 型別，先寫上游的 issue。
