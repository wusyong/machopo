# RFC 0092：taffy 和 parley 的 fork：跟著上游、修改留成 patch、減少 workaround 的成本

- 開始日期：2026-11-10
- 狀態：✅ 已完成：2026-11-10（接受、排程：2026-11-10；「已決定的事項」第 1 到 14 條：整份的範圍、第 1 段；第 15 到 20 條：第 2 段；第 21 到 32 條：第 3 段；第 33 到 37 條：第 4 段；第 38 到 43 條：第 5 段；第 44 到 51 條：第 6 段；第 52 條：第 7 段）
- 進度和驗收：`progress.md`
- 細節：`details.md`（workaround 的清單和成本、parley fork 重寫前的 commit、上游 2026-09-30 之後的變化和進行中的 PR、patch 檔的幾種放法）

## 摘要

taffy 也 fork，和 parley 一樣用 `machopo` 分支、緊跟上游的 main。fork 上的每個 commit 都寫成可以直接送上游的樣子：上游收了就從 fork 拿掉，收不了才在 fork 裡自己維護。每個 commit 另外用 `git format-patch` 匯出成 patch 檔，放在 machopo 的 repo 裡（`patches/<名稱>/`），由 `ci.py third-party` 檢查它和 fork 的分支一致。parley 的 fork 重寫過：rebase 到上游的 main，拿掉上游已經有的，相關的 commit 合在一起，每個 commit 帶 fork 自己的測試。

machopo 這邊只處理一種 workaround：繞在函式庫外面、因此有實際成本的。先加計數量出每一種實際的量，再照量做：字型家族的名稱改成一個 `font-family` 的值只查一次，column flexbox 的 intrinsic 寬度搬進 taffy 的 fork，做 fragment 時多斷的那一次行拿掉；float 旁邊的盒子進 taffy 的快取、`BlockContext` 可以撤回量過之後不做。只是換算輸入的 workaround 幾乎沒有成本，和照 Chrome 才有的行為一樣留在 machopo，不搬進 fork。

## 動機

**開始前**（2026-11-10）：

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

**解決的 `docs/limitations.md` 的限制**：

- 「Layout」的 float 那一條裡 taffy 的限制：BFC 的根包住 float 時沒有加上下方的 padding 和 border、BFC 盒子只避開頂端那一段的 float（上游的 main 修好的）；`display: flow-root` 的盒子裡的 float 放在 max-content 的寬度裡（第 2 段找到，第 6 段修好）。
- 「Layout」的 `text-indent` 那一條：shrink-to-fit 的段落少了縮排的寬度（上游 #866、#889）。
- 「Layout」的 flex 和 grid 那一條裡上游修好的部分：`wrap-reverse` 溢出的 line、grid container 的 baseline。

沒有解決、還在 `docs/limitations.md` 的：「圖片」的尺寸那一條裡 taffy 的部分（自然比例套在 border box 上，上游 #1347 還沒合併；只有 `max-width` 是百分比的 block 圖片的 min-content，跟上上游的 #1401 之後仍然一樣）、寬是 0 的 float 的邊界情況、移到比較窄的地方的 BFC 盒子沒有重排、旁邊有 float 的盒子每次重排。

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
- 升級：在 fork 上 rebase 到新的上游 commit（rebase 前在舊的頂端打 tag），解掉衝突，更新 `third_party.toml` 的 `rev` 和 `upstream_rev`，重新匯出；上游已經收的 commit 拿掉。

## 詳細設計

### fork 和 patch 的規則

- **兩個 fork 都在 `wusyong/` 底下**，`machopo` 分支從上游的 main 分出來；起點寫在 `README.machopo.md` 和 `third_party.toml`。
- **一個修改一個 commit**，寫成可以直接變成上游 PR 的樣子：只做一件事、附上 fork 自己的測試（照上游的寫法），fork 自己釘的 toolchain 也編得過。`machopo:` 註解照舊標出位置。上游 PR 的 branch 從這個 commit 開出來（送出前先給人審核）。
- **patch 檔**：每個 commit 匯出成一個 patch 檔（`python3 scripts/ci.py patches <名稱>`：`git format-patch --zero-commit --no-signature`，檔名不編號，沒改的 commit 重新匯出時不變），放在 machopo 的 `patches/<名稱>/`；順序是 fork 的歷史的順序。上游的 repo 和起點的 commit 是 `third_party.toml` 那個 pin 的 `upstream` 和 `upstream_rev`。fork 的分支一定等於「上游的起點加上這些 patch」：`ci.py third-party`（`test.py` 的每個指令開始前、CI 也跑）在暫時的 index 上把 patch 套到 `upstream_rev` 的樹上，和 `rev` 的樹比，對不上時指出是哪一個 patch、停下。`patches/` 在 `.gitattributes` 標成 `-text`：上游有 CRLF 的檔案，patch 要照原樣保存。理由見「已決定的事項」第 3、9 到 11 條和 `details.md` 的「patch 檔的放法」。
- **跟上游**：rebase 的同時檢查每個 patch 在上游的狀態，上游收了的拿掉，上游有不同做法的改用上游的、在 machopo 這邊配合（parley 的 inline box 改用上游的 U+FFFC 就是這樣）。這份 RFC 裡 parley 跟到 2026-11-08 的 main、taffy 跟到 2026-11-06 的 main；之後多久跟一次沒有定（「之後的可能」）。
- **舊的分支頂端打 tag**：每次重寫歷史前，在舊的頂端打一個 tag（`machopo-<日期>`），讓 machopo 舊的 commit 釘的 rev 還拿得到（`ci.py third-party` 用 commit 的 hash fetch，GitHub 只保證拿得到 branch 或 tag 指得到的 commit）。有了 patch 檔之後，舊的修改的內容也在 machopo 的歷史裡。
- **fork 的測試**：改 fork 時跑 fork 自己的測試（它的 `README.machopo.md` 的「Tests」）和 machopo 相關的測試；規則在 `CLAUDE.md` 的「第三方的 fork」。taffy 沒有釘 toolchain，用 machopo 的編；送上游前用它的 `rust-version` 確認。
- **`README.machopo.md`**：fork 的最後一個、只改它的 commit（也是最後一個 patch）；「Local modifications」一個 commit 一列，標明上游的狀態。

### 每個 workaround 放在哪裡

`details.md` 的「workaround 的清單」每一條都標出：成本的種類、建議（跟上上游、搬進 fork 並送上游、在 machopo 裡做得便宜、留著），以及 Chromium 怎麼做。原則：

- **成本來自繞在函式庫外面的**：搬進 fork（同時寫上游的草稿），或在上游已經有的時候跟上。做了的：column flexbox 的 intrinsic 寬度不排子項（Blink 的 `FlexLayoutAlgorithm::ComputeMinMaxSizes`）、`text-indent` 的 intrinsic 寬度改用上游的。量過不做的：float 的狀態進 taffy 的快取、`BlockContext` 可以撤回（第 20、44 條）。
- **成本來自 machopo 自己做法的**：在 machopo 裡改。做了的：字型家族的清單照 `font-family` 的值快取查到的 family（Blink 照 `FontDescription` 快取 `FontFallbackList`；fork 讓 parley 接受 family 的 id）、fragment 用最後排版時斷的行。
- **沒有成本的換算和照 Chrome 的行為**：留在 machopo。

### 計數

照 `layout` 的 `LayoutUpdate::lines_broken` 的做法，有成本的 workaround 各有一個計數，一般的 build 也數，測試守著次數：

- `LayoutUpdate::laid_out_beside_floats`：因為旁邊有 float（或把自己的 float 放進共用的 block formatting context）沒有用 taffy 的快取的盒子（`compute_box`）。
- `LayoutUpdate::lines_broken_for_fragments`：做 fragment 時在最後的寬度又真的斷了一次行（`inline_fragments`），`lines_broken` 的一部分。
- `LayoutUpdate::lines_broken_again_beside_floats`：行在 float 旁邊從頭重斷的次數（`break_beside_floats`），四種原因合成一個數。
- `BoxTreeUpdate::font_family_names`：照名稱在字型集合裡查 family 的次數；看過的 `font-family` 的值是 0。

第 2 段用它們量出實際的量（`progress.md` 的「量測」），決定了後面幾段的順序和哪些不做；做了的每一段用它們驗收。

### 字型家族

`layout` 的 `FontFallbackMap`（Blink 的同名類別）記每個 `font-family` 的值在字型集合裡查到的 `FamilyId` 清單（generic 的也展開）：值第一次用到時每個名稱查一次，之後直接把 id 給 parley（fork 的 `StyleProperty::FontFamilyIds`）。key 是清單的值，值一樣、來自不同 style 的清單共用一筆；註冊字型、改 generic 的對應時整個清掉。canvas 的文字走同一個快取。parley 的 root style 也是沒有文字的 layout 和空的行的 style，它的家族用 fork 的 `TreeBuilder::push_default` 給（第 38 到 43 條）。

### column flexbox 的寬度

taffy 的 fork 的 `compute_preliminary` 在只被問寬度、column、不換行、沒有子項帶 `aspect-ratio` 時，不量子項的高度，直接取子項的 hypothetical cross size 的最大值。量子項時給它自己的 style 的高度，不是 flex 之後的高度（和完整演算法的差異見第 46 條）。寬度有限時的 shrink-to-fit 由 machopo 的 `fit_content_width` 做，和 row、grid 一樣（第 45 到 47 條）。

### 做 fragment 時的斷行

`compute_inline` 在 `PerformLayout` 結束時記下斷行的寬度和旁邊的 float（`InlineContent::keep_laid_out`），`inline_fragments` 用記下的寬度問 `break_lines`，命中存著的結果；記下的寬度和盒子最後的寬度差超過 1/64 px 時才在盒子的寬度再斷一次。寬度由內容決定的盒子（taffy 沒有傳已知的寬度、style 的寬度是 `auto` 或 `fit-content`、沒有 `aspect-ratio`），行在 fit-content 的寬度斷；被給了寬度的盒子，行和裡面的 float 直接在那個寬度排（第 48 到 51 條）。

### 測試

- 第 1 段（taffy fork，起點 v0.15.0）：行為不變；fork 的 `src/` 和 crates.io 的 0.15.0 逐檔一樣。
- 跟上游的每一段：reftest 和 WPT 的預期結果的變化逐項查過，變好的刪掉 `.ini` 和對應的限制。
- 搬進 fork 的修改：fork 自己的測試，加上 machopo 原有的測試（`layout` 的 `float::`、`flex::`、`inline::`、`font_selection::`……）。拿掉 machopo 的 workaround 時，原本守著它的測試照樣過。經過 machopo 的 layout 才看得到的、要用 fork 沒有的字型的測試留在 machopo。
- patch 和 fork 一致：`ci.py third-party` 的檢查；它自己的測試是 `scripts/ci_patches_test.py`（CI 的 `lint` 跑）。
- mutation test：每段新的行為（patch 的匯出和檢查、計數、快取、fork 的修改）都跑過。

每一項怎麼驗證、跑了哪些目錄、mutation 的結果在 `progress.md`；測試守不住的在 `docs/test-gaps.md`。

### 分階段

1. **taffy fork 的基礎設施**：`wusyong/taffy` 的 `machopo` 分支，起點是 v0.15.0（和當時用的一樣），`third_party.toml`、`Cargo.toml` 的 path 依賴、`README.machopo.md`；`patches/`、`ci.py patches` 和 `ci.py third-party` 的一致性檢查；parley 當時的 16 個 commit 也匯出。行為不變。
2. **計數**：加上「計數」那一節的計數，量出各個 workaround 實際的量，決定第 5、6 段的順序和範圍（第 20 條）。
3. **重寫 parley 的 fork**：3a 把 16 個 commit 合成分組、搬到上游的 main（`1b67b75`）、拿掉上游已經有的 `hanging_advance`、atomic inline 改用上游的 U+FFFC，machopo 配合；3b 每個 commit 帶上 fork 自己的測試和完整的訊息，optical sizing 改由 machopo 做，拿掉 machopo 的 `text-indent` workaround，寫三份上游的草稿。
4. **taffy 跟上上游的 main**（`4aca4f49`）：machopo 配合 API，刪掉修好的 18 個 WPT 的預期失敗和對應的限制。原本還有 4b（用 slot 的高度改寫 `float_bands`），取消（第 37 條）。
5. **字型家族**：5a 是 parley 的 fork 的兩個 commit（`FontFamilyIds`、`push_default`），5b 是 machopo 的 `FontFallbackMap`。
6. **column flexbox 的 intrinsic 寬度、做 fragment 時的斷行**：6a 把 column 的寬度搬進 taffy 的 fork；6b 在 machopo 裡拿掉做 fragment 時多斷的那一次行。原本 6a 還要把 float 的狀態加進 taffy 的快取的 key，量過之後不做（第 44 條）。
7. **上游的草稿**：不寫剩下的草稿，`upstream/parley/README.md` 列出每個 commit 的狀態和送之前的下一步（第 52 條）。沒有送出任何東西。

## 缺點

- **多一個 fork 要跟著 rebase**：taffy 的上游一個月有幾十個 commit，跟 main 代表它的回歸也直接變成 machopo 的，WPT 的預期結果更常變。
- **parley 的重寫是一次性的大工作**：上游多了 66 個 commit，inline box 改用 U+FFFC 和 reshaping 的改動和 fork 的 inline box、字詞快取衝突；之後上游合併 tab stop（#934）、軟連字號（#827）時還有一次。
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

第 2 段（2026-11-10 確認）：

15. **四個計數，放在 `LayoutUpdate` 和建 box tree 的統計裡**（2026-11-10）：`laid_out_beside_floats`（`compute_box` 因為 float 沒用 taffy 的快取的盒子）、`lines_broken_for_fragments`（`inline_fragments` 在最後的寬度又真的斷了一次行，`lines_broken` 的一部分）、`lines_broken_again_beside_floats`（`break_beside_floats` 用 `revert_to` 重來的次數，四種原因合成一個數）、`font_family_names`（`text_style` 傳給 parley 的字型家族名稱，每個 span 每個名稱一次）。不選把字型名稱的計數放在 parley 的 fork 裡：那是只為計數、送不了上游的 commit。
16. **每個計數都有測試守著**（2026-11-10）：`layout` 現有的整合測試的模組裡，構造會和不會觸發的例子；跑 mutation test。
17. **量法：`RUST_LOG=layout=debug` 加上一次性的加總**（2026-11-10）：script 放在 scratchpad，不 commit，結果記進 `progress.md`。不選寫成 `bench.py` 的子指令：這一段只量一次，之後的驗收靠測試裡的計數。
18. **量的頁面**（2026-11-10）：`tests/sites/` 的三個網站、blink_perf 的 `layout/` 裡相關的頁（`floats_*`、`chapter-reflow*`、`line-layout-line-height`、`flexbox-*`）和 Speedometer 3.1 的 suite（使用者同意：只數計數，不量時間）。
19. **計數一直開著**（2026-11-10）：和 `lines_broken` 一樣，一般的 build 也數，每次加一個整數。不選只在 debug build 或測試裡數：release 的頁面量不到真正的工作量。

第 2 段的量測之後（2026-11-10 確認）：

20. **第 5、6 段的順序：字型家族、float 和 taffy 的快取、做 fragment 時的斷行；`BlockContext` 可以撤回延後**（2026-11-10）：照第 2 段量到的量（`progress.md` 的「量測」）。字型家族的名稱每頁都有，Speedometer 每個 inline formatting context 平均 8 個；float 旁邊不用快取的盒子只在有 float 的頁，但在那些頁佔 65% 到 96%；做 fragment 時再斷行只在 Perf-Dashboard 多（23%）。float 旁邊的行重斷在量的頁都是 0，所以 `BlockContext` 可以撤回不排進這份 RFC 的階段：要改 taffy 的 block layout 的核心，省不到量得到的工作；之後有頁面量到再做。不選照原本的分法（machopo 裡的先做、fork 裡的後做）：做 fragment 時的斷行會排在量比它大的 float 快取前面。

第 3 段（2026-11-10 確認）：

21. **分成 3a 和 3b**（2026-11-10）：3a 是 rebase 到上游的 `1b67b75`（2026-11-08）、合成分組、拿掉 `hanging_advance`、machopo 配合上游 API 的變化，排版和第 2 段的計數不變；3b 是每個 commit 的 fork 自己的測試、上游的草稿、拿掉 machopo 的 `text-indent` workaround。不選一段做完：改寫和新的測試混在一起，排版變了時分不清是哪一邊。
22. **先在舊的起點合成分組，再搬到新的 main**（2026-11-10）：第一步之後的 tree 要和重寫前的 `cefcbc5` 完全一樣（機械地驗證），第二步每組只解一次衝突。不選 16 個 commit 逐個 rebase 再合：試套時每一個都衝突，同一段程式要解好幾次。不選把整個 diff 一次套上再切開：切錯了沒有東西對照。Chromium 升級 `third_party` 時把 patch 依序重新套上，和第二步一樣。
23. **tab stop 和軟連字號保留 fork 的**（2026-11-10）：上游的 #934、#827 還沒合併，照新的 main 改寫；合併時再決定（「之後的可能」）。
24. **optical sizing 在 3a 原樣保留**（2026-11-10）：3b 再查能不能由 machopo 把 `opsz` 放進 `font_variations` 做到，能的話拿掉這個 commit。不選在 3a 就拿掉：3a 的排版要不變。
25. **tag 和改寫都先在本地做，由人審核之後 push**（2026-11-10）：舊的頂端的 tag 是 `machopo-2026-11-10`。審核期間 `third_party.toml` 釘本地的 commit，push fork 之前不 push machopo。

3a 的 inline box（2026-11-10 確認；上游的 #902 把 in-flow 的 inline box 改成文字裡的 U+FFFC，分組合好之後才看得出它對 fork 第一個 commit 的影響）：

26. **atomic inline 照上游：machopo 在給 parley 的文字裡放 U+FFFC**（2026-11-10）：每個 atomic inline 一個，byte offset 對回 DOM 時跳過它。Blink 的 `NGInlineItemsBuilder` 也放 object replacement character。不選在 fork 裡保留不佔字元的 in-flow box：等於撤回 #902，之後每次 rebase 都要和上游的斷行、選取、bidi 對著改。
27. **inline 元素的邊緣是不佔字元、有寬度、黏著前或後的內容的 box**（2026-11-10）：做成上游不佔字元的那一類項目（`OutOfFlowBox`）的延伸，對應 Blink 的 open tag / close tag item（有 inline size、不是字元）。不選邊緣也用 U+FFFC：多出的字元在 bidi 裡是中性字元、兩邊有斷行機會，都要另外壓掉。不選把寬度加在 span 第一個和最後一個 cluster 的 advance 上：空的 span 和跨行的 span 沒有 cluster 可以加。
28. **一次搬到 `1b67b75`，3a 的驗收是「除了 atomic inline 的 bidi 和斷行，排版不變」**（2026-11-10）：改掉第 21 條的「排版不變」。reftest 和 WPT 的差異逐項查，記進 `progress.md`。不選先搬到 #902 的前一個 commit 再跨過它：衝突要解兩次。

3b（2026-11-10 確認）：

29. **machopo 的 `parley_*` 測試搬進 fork，machopo 的刪掉**（2026-11-10）：照 parley 的測試工具改寫，放進各自的 commit；現做字型的測試讓 fork 多一個 `write-fonts` 的 dev-dependency。只有經過 machopo 的 layout 才看得到的留在 machopo。不選兩邊都留：同一件事測兩次。不選留在 machopo：fork 的 commit 沒有測試，不能直接送上游。做的時候多留了一類：要用 fork 的 repository 沒有的字型的（真的 Lato、WPT 的字型）。
30. **optical sizing 由 machopo 做，拿掉 fork 的 commit**（2026-11-10）：machopo 在給 parley 的 variation 裡加 `opsz`（Blink 也在自己這一側把 `opsz` 放進字型的 variation），先確認排版一樣，不一樣就留著 fork 的。不選留著：多一個只為 CSS 的 API 要說服上游。
31. **3b 補完每個 commit 的訊息，寫三份 PR 的草稿**（2026-11-10）：最後的強制換行、沒有 em 的字型、斷行表。邊緣、共用 shaped text、快取、第一個字型要先開 issue 討論，tab stop 和軟連字號要寫的是和上游還沒合併的 PR 的差異，都留到第 7 段。草稿在這份 RFC 的 `upstream/parley/`。不選 3b 全部寫完：大的幾份在第 5、6 段還會改。
32. **3a 的頂端打 tag（`machopo-2026-11-10-3a`），fork 現在 push**（2026-11-10）：3b 把測試加進各自的 commit，等於再改寫一次；3a 的 machopo commit 釘的是 3a 的頂端，靠 tag 留住。改掉第 25 條的「審核之後才 push」只有一次的假設：每次改寫前都在舊的頂端打 tag。

第 4 段（2026-11-10 確認）：

33. **taffy rebase 到上游 main 的頂端（`4aca4f49`，2026-11-06）**（2026-11-10）：float 和 BFC 的修正、slot 的高度都在 11 月的 commit 裡。不選 v0.15.1：沒有那些修正，這一段的目的大半達不到。不選等 #1396、#1400 合併：不知道要等多久，之後再 rebase 的成本很低。
34. **第 4 段分成 4a、4b**（2026-11-10）：4a 只 rebase 加上編得過所需的最小配合（`find_content_slot` 的高度傳 0），結果的變化全部歸因於上游；4b 用 `ContentSlot::height` 改寫 `float_bands`。Chromium roll 第三方的相依和配合它的改動也是分開的 CL。不選一次做完：WPT 有變化時分不出是上游還是 machopo 的改寫造成的。
35. **上游的變化讓測試退步時先查原因，照大小處理**（2026-11-10）：小而明確的在 fork 修成一個帶測試、可以送上游的 commit（每一個先列給人看），草稿留到第 7 段；大的更新預期結果、記進 `docs/limitations.md`，草稿寫成 issue。不選一律只記限制：退步留到上游修好。不選退回早一點的上游 commit：同時失去修正。
36. **第 4 段只拿掉 `float_bands` 自己找底**（2026-11-10）：#1303 的 `FloatMark`（行內的 float 不必 `revert_to`）和 float 旁邊的盒子進快取留到 6a（後來量過、都不做：第 44 條）；`last_baseline`（#1110）、`aspect-ratio`（#1347）等上游。`rfcs/0043-floats-text/taffy-float-root-inset-report.md` 要回報的問題上游已經修好（#1384），不用送。
37. **4b 取消，`float_bands` 留著不改**（2026-11-10，改掉第 34、36 條裡 4b 的部分）：上游的 `ContentSlot::height`（#1394）是這個寬度往下到被更窄的 segment 縮小為止的距離，不是 segment 的底：它不告訴你哪裡變寬，後面沒有更窄的 segment 時仍是無限大。`FloatBands::room` 要每個 band 的頂端和下一個 band 的頂端（包括變寬的地方，下一行才知道可以用更寬的空間），所以一段一段走 `find_content_slot`、最後用 `cleared_threshold(Clear::Both)`（O(1)）找最低的 float 的底，這兩件事都拿不掉；這個 workaround 的成本本來就少。不選現在在 fork 加露出 segment 範圍的 API（Blink 的 exclusion space 給的 layout opportunity 就有完整的範圍）：fork 多一個要送上游的 commit，而 6a 本來就要改 taffy 的 float，到那一段再列成要決定的事。不選每行直接問 `find_content_slot(top, line_height)`：要重做斷行和 `FloatBands` 的介面，而且仍然不知道哪裡變寬。

第 5 段（2026-11-10 確認）：

38. **字型家族的成本：machopo 快取 `font-family` 的值查到的 family，fork 讓 parley 接受 family 的 id**（2026-11-10）：`layout` 的 `FontFallbackMap` 記每個值在字型集合裡查到的 `FamilyId` 清單（generic 的也展開），值第一次用到時查一次，之後直接給 parley。對照 Blink：`FontFallbackMap` 照 `FontDescription` 快取 `FontFallbackList`，字型集合變動時整個作廢。不選只快取展開的名稱：parley 仍然每個 span 逐個名稱查。不選等上游的 #796、#943：都還沒合併，也不解決每個 span 重查。
39. **快取的 key 是清單的值，不是 `Arc` 的指標**（2026-11-10）：值一樣、來自不同的 style 的清單共用一筆（指標不同的清單很多：每個設了 `font-family` 的規則各一個）。註冊字型、改 generic 的對應時清掉整個快取，不逐筆作廢（和 Blink 一樣）。hash 一份清單的成本是幾個短字串，每個 span 一次，比每個名稱查一次 fontique 小。
40. **canvas 的文字走同一個快取，`shell` 的 `ui_text` 不動**（2026-11-10）：canvas 的字型和頁面的是同一個集合。網址列的文字只有一兩個固定的清單，不在 renderer 裡。`BoxTreeUpdate::font_family_names` 改成數真的照名稱查的次數（看過的值是 0）。
41. **第 5 段分成 5a、5b**（2026-11-10）：5a 是 fork 的 commit（machopo 只動 pin 和 patch），5b 是 machopo 的快取和改用新的 API。
42. **fork 的 API：只加 `StyleProperty::FontFamilyIds`，不改 `TextStyle`**（2026-11-10）：`Cow<[FamilyId]>`，設的是 `StyleProperty::FontFamily` 設的同一個東西，不經過名稱。不選在 `TextStyle` 加欄位：上游每個寫出整個 `TextStyle` 的地方都要改，送上游時改動大得多。
43. **tree builder 的 root 的字型家族用新的 `TreeBuilder::push_default` 給**（2026-11-10，fork 的另一個 commit）：對應上游已有的 `RangedBuilder::push_default`。原本的做法是 root 不帶家族、在所有文字外面包一個帶 id 的 span，結果變了：parley 的 root style 也是沒有文字的 layout 和空的行的 style（行高來自它的字型），span 管不到（`shell` 的 `focus::focusing_scrolls_into_view_if_needed` 差 1px 抓到的）。不選在 `TextStyle` 加欄位（同第 42 條）。不選 root 仍然給名稱：每個 inline formatting context 還是逐個名稱查一次。它在推了任何東西之後呼叫會 panic：已經推的 span 和 run 留著 root 原本的 style。

第 6 段（2026-11-10 確認）：

44. **float 旁邊的盒子進快取不做，`compute_box` 的做法和 `laid_out_beside_floats` 的計數留著**（2026-11-10，改掉第 20、36 條裡 6a 的這一部分）：動手前先量會命中的上限和時間（`progress.md` 的「量測」的「float 的快取會命中的上限」）。float 旁邊沒用快取的計算有 94% 到 99.5% 是同一個盒子、同樣的輸入又算一次，結果也都一樣，但每次命中只省那個盒子自己的一次計算：它的子項不是已經在快取裡（float 是 BFC 的根），就是自己也是這種盒子；inline 的內容本來就記著同樣寬度、同樣 float 旁邊的斷行，一次斷行都沒省到。計數上最該受益的三頁（blink_perf 的 `floats_show_hide`、`floats_2_100_nested`、`floats_20_100_nested`）machopo 是 Chrome 的 0.82 到 1.05 倍時間，沒有落後；真實的頁面（gutenberg）38 次 layout 共約三千次這種計算。代價是 fork 的兩個 API（取出盒子看得到的 float 狀態來比較、重放它放過的 float）、每個盒子多存一份狀態、和「狀態完全一樣」的論證。沒有取樣這些計算佔多少時間：之後有頁面慢在這裡時再量。當時考慮的做法（沒做）：key 是盒子看得到的 float 狀態、完全一樣才命中並重放 float（Blink 的 constraint space 帶 exclusion space，結果存著輸出的 exclusion space）；快取的項目放在 machopo 的 box 裡，fork 只加 `BlockContext` 的 API。用 `FloatMark` 讓行內的 float 不必 `revert_to` 也不做：第 2 段量到的重斷次數全部是 0。
45. **column flexbox 的 intrinsic 寬度做在 taffy 的 fork 裡，machopo 的 `column_flex_width` 拿掉**（2026-11-10）：fork 的 `compute_preliminary` 在只被問寬度、column、不換行、沒有子項帶 `aspect-ratio` 時，不量子項的高度，直接取子項的 hypothetical cross size 的最大值。不選留在 machopo：那是 190 行照著 taffy 內部重算一次的程式，加上 debug build 每次再跑 taffy 對照，taffy 改了就要跟著改；做在 taffy 裡用的是它自己的函式，其他用 taffy 的人也拿得到（上游的草稿在 `upstream/taffy/`，不送）。不加 trait 的開關：捷徑只在結果由子項的貢獻決定時走。
46. **捷徑量子項時給它自己的 style 的高度，不是 flex 之後的高度；和完整演算法的差異接受並記錄**（2026-11-10）：全部的 fixture 不變，但寬度跟著給的高度變、又不經由 `aspect-ratio` 的子項，結果和完整的演算法不同（`progress.md` 的「查到的事和踩過的坑」）。給 style 的高度是 Blink 的 `ComputeMinMaxSizes` 算子項貢獻的做法，蓋住「有固定高度、寬度由高度決定」的子項。不選為了完全等價在這些情況走完整的演算法：taffy 看不到 measure function 會不會用高度，只能全部不走。machopo 原本的捷徑有同樣的差異。
47. **寬度有限時 column 的 shrink-to-fit 由 machopo 的 `fit_content_width` 做，不放進 fork**（2026-11-10）：它本來就對 row、grid 和換行的 column 做這件事（量 min-content 和 max-content 再夾），兩次量測各走一次 fork 的捷徑。不選放進 fork：taffy 的其他 container 都不 shrink-to-fit，只改 column 會讓 fixture 變。
48. **做 fragment 用的是最後排版時斷的行，不從 border box 算回寬度再問一次**（2026-11-10）：`compute_inline` 在 `PerformLayout` 結束時記下斷行的寬度和旁邊的 float（`InlineContent::keep_laid_out`），`inline_fragments` 用記下的寬度問 `break_lines`，命中存著的結果。這是 Blink 的結構（行是 inline layout 的產出，不是之後再問一次）。量到的再斷行有 68%（343 次裡的 234 次）只是 content box 的寬度從 border box 減 padding 和 border 算回來時的浮點誤差（`204.79999` 對 `204.8`）。記下的寬度和算回來的差超過 1/64 px 時（盒子最後不是斷行時的寬度，例如 `min-width` 把它撐寬），照舊在盒子的寬度再斷一次。不選在 `break_lines` 比寬度時加容許值：那是把誤差蓋掉，在別的地方（量測之間）會把真的不同的寬度當成一樣。不選定點數（Blink 的 `LayoutUnit`）：taffy 和 parley 都是 f32。
49. **寬度由內容決定的盒子，行在 fit-content 的寬度斷，不在給它的空間斷**（2026-11-10）：盒子沒有被給寬度（taffy 沒有傳已知的寬度、style 的寬度是 `auto` 或 `fit-content`（button 的 `auto` 也是它）、沒有 `aspect-ratio`）時，它最後的寬度是量出來的 fit-content 寬度，行就在那裡斷；原本只有帶 float 的行內內容這樣做，其他的在空間的寬度斷、做 fragment 時再在盒子的寬度斷一次（量到的 101 次）。帶 float 的行內內容原本不管盒子有沒有被給寬度都在 fit-content 的寬度斷，現在也照這條規則：被給了寬度的盒子裡，行和 float 直接在那個寬度排，不先在 fit-content 的寬度排一次。高度也跟著對了：shrink-to-fit 的盒子裡有百分比寬度的 inline-block 時，原本高度是在空間的寬度量的、行是在盒子的寬度斷的，兩者不一致。不選「不是只問寬度時一律在 fit-content 的寬度斷」（原本確認的做法）：taffy 的 leaf 在 `PerformLayout` 時把盒子被給的寬度當成可用的空間傳進來、已知的寬度是空的，這時空間就是盒子最後的寬度；它比 min-content 窄或比 max-content 寬時 fit-content 的寬度和它不同，一律用 fit-content 會改變排版（`layout` 的測試會失敗：行數、list marker 的位置、斷行的次數）。
50. **斷行的結果被清掉之後的再斷（量到 8 次）不另外處理**（2026-11-10）：atomic inline 的大小變了、或量 min 和 max content 時 `broken` 被清掉，做 fragment 時再斷一次是必要的。計數留著。
51. **不多存一份斷行的結果**（2026-11-10，改掉第 6 段原本的計畫）：原本打算在 machopo 多存一份最後的寬度的斷行結果。分類之後，量到的再斷行沒有一次是「taffy 在最後的 layout 之後又量了別的寬度」，多存一份省不到。這種情況之後出現時（問高度的量測發生在最後的 layout 之後）仍然會再斷一次，`lines_broken_for_fragments` 數得到。不選照 constraint space 快取多份（Blink 的 `InlineNode`）：parley 的 `Layout` 一份就是整段的行和對齊，多存要複製它。

第 7 段（2026-11-10 確認）：

52. **剩下的草稿不先寫，只列每個 commit 的狀態和送之前的下一步**（2026-11-10，改掉第 31、35 條裡「留到第 7 段」的部分）：`upstream/parley/README.md` 的表一個 commit 一列。已經有的草稿（parley 三份、taffy 一份）留著；parley 剩下的八個 commit 等真的要送某一個時再寫那一份。fork 的 commit 訊息和 `README.machopo.md` 已經照可以送上游的樣子寫（每個 commit 一件事、帶測試、寫明理由），再寫一份草稿多半是重複；上游規定 PR 的說明要送出的人自己寫，草稿只是材料；要先開 issue 的四個（邊緣、第一個字型、共用 shaped text、shaping 的快取），做法要等上游的回應才定得下來；草稿對照的是寫的時候的上游，放久了要重寫。不選八份都寫：成本最高，上面的理由。不選只寫要判斷的那幾份（issue 的四份、和 #934、#827 的比較）：比較現在做了，它們合併時還要再做一次。代價：#934、#827 和 fork 的 tab stop、軟連字號差多少現在沒有查，它們合併、fork rebase 遇到時才知道衝突多大。

## 待決定

沒有。做完時還沒定的三件事不擋這份 RFC，移到「之後的可能」：跟上游的頻率、上游的 tab stop 和軟連字號合併時用哪一邊、fork 要不要有自己的 CI。原本的「parley 的 commit 怎麼合」在第 3 段定了（第 21 到 32 條；最後是 11 個 commit）。

## 之後的可能

- **跟上游的頻率**：這份 RFC 裡各跟了一次。之後是每月一次、上游合了 machopo 需要的修改時（taffy 的 #1347、#1110、#1396、#1400，parley 的 #796、#943），還是只在有需要時，還沒定。一次 rebase 的工夫：taffy 是配合 4 處 API，parley 的重寫是一次性的。
- **上游的 tab stop（#934）、軟連字號（#827）合併時**：改用上游的、在 machopo 這邊配合，還是保留 fork 的；看它們合併時的 API 和行為。fork 的和它們差多少沒有查過。
- **fork 要不要有自己的 CI**：現在 fork 的測試在改 fork 時在本機跑，machopo 的 CI 不跑它們。可以在 fork 上開 GitHub Actions，或由 machopo 的 CI 一起跑。
- **送上游**：草稿和每個 commit 送之前的下一步在 `upstream/`；由人審核、自己寫 PR 的說明之後送。taffy 的 commit 送之前用它的 `rust-version`（1.71）編一次。
- float 的狀態進 taffy 的快取、`BlockContext` 可以撤回、露出 float 的 segment 範圍的 API：量過不做（第 20、37、44 條）；之後有頁面慢在這裡時再量。
- 換成上游的 table（taffy #1380）：和 machopo 照 Blink 做的 table 比 WPT 的結果、效能，決定要不要換。
- rusty_v8、wasmtime 也改用 patch 檔的做法。
- parley 斷行時複製 `LineState` 的成本（`overflow-wrap: break-word` 每個字元一次）：要改 parley 斷行的核心，看上游的 reshaping 改動（#930）之後再決定。
- `min-*` / `max-*` 的 intrinsic 關鍵字：要改 taffy 的 style 型別，先寫上游的 issue。
