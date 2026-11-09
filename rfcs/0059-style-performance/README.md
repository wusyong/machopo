# RFC 0059：style 的效能

- 開始日期：2026-10-19
- 狀態：✅ 已完成：2026-10-19（style 每個 iteration 151 → 92 ms，−39%，Chrome 34 ms；祖先的 bloom filter 和名字預先算好的雜湊；SVG 的 presentation attribute 存在元素上加共用快取；`url()` 在解析時解析；script-blocking stylesheet；子 frame 第一次的 style 就有 iframe 的大小；stylesheet 改動和 class、id、attribute、狀態的改動用 invalidation set，照 Blink；matched properties cache 存調整完的 `Arc` 共用；`ComputedStyle` 少用的欄位分成三個共用的組；`ComputedStyle` 全部分組（5c）量過之後延到之後；在 macOS 上做）
- 細節：`details.md`（量測的方法和結果、RFC 開始前的程式、和 Chromium 的逐項對照、各段開始前的取樣）
- 進度和驗收：`progress.md`

## 摘要

Speedometer 裡 kun 的 style 每個 iteration 比 Chrome 多 117 ms（151 對 34 ms，Apple M4），分散在每個 suite。量出兩個原因：每個元素比較貴（主因：NewsSite 重算的元素數和 Chrome 一樣，時間卻是 4 倍），和有時重算太多（Complex-DOM 的 TodoMVC 每個 iteration 整份重算約 8 次）。這份 RFC 分六段做：

- **第 1 段，量測的工具**：`ci.py speedometer --trace` 的各階段報告（kun 和 Chrome 並列），stylesheet 算成「變了」的原因的 log；查出 `<link>` 晚到的整份重算是 `start_loads` 為了找背景圖片先算 style 觸發的。
- **第 2 段，便宜的**：雜湊換成 FxHash；render-blocking 的 stylesheet 沒到齊時 `start_loads` 不算 style；元素有沒有 render 跟著走訪帶下來；SVG 的 presentation attribute 存在元素上加共用快取；`url()` 在解析時解析（照 Chrome，`dom` 記住每份 document 的 base URL）。
- **第 3 段，selector 的比對**：祖先的 bloom filter（Stylo 的 `StyleBloom`），名字預先算好的雜湊；之後修了 `forget_moved` 每次 recalc 掃整個被移除的集合（jQuery 的 style 15 → 5 ms）。
- **第 4 段，stylesheet 和 invalidation**：script-blocking stylesheet（4a）；子 frame 的 media query 和第一次的 style 用 iframe 的大小（4b）；stylesheet 改動只重算多了、少了的規則可能符合的元素（4c document、4d shadow root 和 CSSOM 改動，照 Blink 的 `InvalidateForRuleSetChanges`、`RuleSetDiff`）；class、id、attribute、狀態的改動用後代的 invalidation set（4e，Blink 的 `RuleFeatureSet`）。
- **第 5 段，`ComputedStyle`**：matched properties cache 存調整完的 `Arc`、命中就共用，相同就沿用舊的（5a）；少用的欄位分成三個共用的 `Arc` 組（5b）。全部分組（5c）量過之後延到之後：複製只剩 style 的一成左右，卻要改約 640 處（第 57 條）。
- **第 6 段，量測和決定**：兩個便宜的熱點（6a），之後結束（第 59 條）。

結果（2026-10-19，`ci.py speedometer --trace`，3 個 iteration，`progress.md` 的「效能的量測」）：style 每個 iteration 151 → 92.1 ms（−39%，Chrome 34.5 ms）；Complex-DOM 的 TodoMVC 重算的元素少一半左右（Preact 51,254 → 23,989）。建 box tree、layout、script 不受影響。

## 動機

**量到的**（2026-10-19，`details.md` 的「量測」）：每個 iteration、20 個 suite 合計。

| | kun | Chrome | kun 多的 |
| --- | ---: | ---: | ---: |
| style | 151 ms | 34 ms | 117 ms |
| 建 box tree | 92 ms | 6 ms | 86 ms（51 ms 是 Editor-TipTap 的文字 shaping） |
| layout（Chrome 含文字的 shaping） | 92 ms | 114 ms | −22 ms |

style 差最多的 suite：NewsSite-Next 16 ms、NewsSite-Nuxt 13 ms、React-Stockcharts-SVG 12 ms、TodoMVC-jQuery 10 ms、TodoMVC-WebComponents 8 ms，十幾個 TodoMVC 各差 4 到 6 ms。

**每個元素比較貴**：4 個 suite 的 style 取樣（`details.md` 的「style 的取樣」）。

| 部分 | 佔 style | 怎麼回事 |
| --- | ---: | --- |
| 套用宣告（`cascade_blocks`） | 約 31% | `ComputedStyle` 是一個 1472 bytes 的結構，每個元素整個 clone、`memmove`（約 13%），裡面約 20 個 `Arc` 每次都要加減計數；還有用 SipHash 的雜湊 |
| selector 的比對 | 約 17% | 沒有祖先的 bloom filter：每個候選的規則都完整比對 |
| SVG 的 presentation attribute | 約 8% | 每次重算都用 CSS 的 parser 重新解析，連路徑的 `d` 都在這裡解析；集中在 Stockcharts |
| 其他 | | transition 的檢查約 4%；`url()` 每次重算都對 base URL 解析（佔 WebComponents 全部 CPU 的約 5%） |

**有時重算太多**：

| suite | Chrome 重算的元素 | kun | 倍數 |
| --- | ---: | ---: | ---: |
| NewsSite-Next | 19686 | 19599 | 1.0 |
| React-Stockcharts-SVG | 14656 | 16881 | 1.15 |
| TodoMVC-jQuery | 4596 | 6705 | 1.46 |
| TodoMVC-Preact-Complex-DOM | 17880 | 78044 | 4.4 |

（整次執行、3 個 iteration。）`recalc_style` 本身（不含它呼叫的 cascade）只佔 style 的 4.2%（第 1 段重新取樣）。Preact 的 24 次整份重算裡：
- 14 次是新的 document 第一次算 style（每個 iteration 約 5 次，大多是 `about:blank`、runner 的頁面這類元素很少的 document）；
- 7 次是 `<link>` 的 stylesheet 晚到：載入子資源的 `start_loads` 為了找背景圖片，每次都先把 style 算到最新，不等 render-blocking 的 stylesheet，之後每到一份 stylesheet 就整份重算（第 1 段查到的，`details.md` 的「整份重算的觸發點」）；
- 3 次是 `<style>` 改了。

**目標**：
- style 每個元素的成本接近 Chrome：同樣的元素數，時間降到現在的一半以下。
- 不必要的整份重算拿掉：stylesheet 晚到時，只重算受影響的元素，或根本不先算。
- 行為不變：reftest、WPT、情境照舊通過。增量的條件配 debug build 的交叉檢查（`docs/architecture.md` 的「快取和增量：加新東西時要跟著改的地方」）。

結果：每個元素的成本沒有降到一半（NewsSite-Next 16 → 13 ms，重算的元素數不變）：剩下的在套用宣告和 selector 的比對，分散在很多小地方（`details.md` 的「第 6 段前：style 的時間花在哪」）；不必要的整份重算拿掉了，`<link>` 到的時候和 Chrome 一樣只整份算一次；行為不變，每個增量的條件都有 debug build 的交叉檢查。

**不在範圍內**：
- **文字的 shaping**（Editor-TipTap 的建 box tree）：是文字的問題（harfrust、macOS 系統字型的 AAT `morx` 表；Chrome 有 `ShapeCache`），另外處理。
- **後代的 invalidation set**（RFC 0051 的 D5、D6）：NewsSite 重算的元素數和 Chrome 一樣，jQuery、WebComponents 多 1.4 到 1.5 倍，不是主因（stylesheet 改動用的在第 4c 段，class、id、attribute 改動用的在第 4e 段：第 35、39 條）。
- **script 和 DOM 那一側**：RFC 0058。
- **建 box tree 本身的增量**（RFC 0058 的「之後的可能」）。

要解決的 `docs/limitations.md` 裡的限制：「Style」的 `url()` 在算 style 時才對 base URL 解析（D 方向的小項，見「詳細設計」的「其他小項」）；「Invalidation」裡 style 的重算範圍那一條，只在 B 方向的一部分碰到，不解決（後來 D2 照後代的 invalidation set 改了一部分：stylesheet 和屬性的改動只重算規則可能符合的元素）。

## 設計概要

網頁的行為和畫面幾乎不變：style 算得比較快，stylesheet 到的時候和改的時候只重算受影響的元素。看得到的差別只有兩個，都和瀏覽器一致：parser 插入的 script 等它前面的 stylesheet 載入完才執行，子 frame 第一次的 style 就照 iframe 的大小判斷 media query。

給之後改這部分程式的人：
- **`ComputedStyle`**：少用的欄位在三個共用的組裡（`style.rare`、`style.backgrounds`、`style.svg`），寫的時候用 `rare_mut()` 這類，共用時才複製那一組；matched properties cache 存調整完的 `Arc`，連依位置的調整的輸入（`Position`）都在 key 裡，所以新的調整輸入要加進 `Position`。
- **selector 的比對**：自己那棵樹的規則比對前先查祖先的 bloom filter（`bloom` 模組），名字帶著預先算好的雜湊。
- **SVG 的 presentation attribute**：解析結果存在元素上，attribute 改了才重新解析，相同的值共用。
- **`url()`**：解析時就照當時的 base URL 解析好，算 style 時不再解析。
- **invalidation**：stylesheet 變了時照多了、少了的規則的特徵找要重算的元素（`rule_set_invalidation` 模組）；class、id、attribute、狀態改了時照後代的 invalidation set 找（`DescendantInvalidations`）。每個都有 debug build 的交叉檢查，新的 selector、at-rule、調整的輸入要照 `docs/architecture.md` 的「快取和增量：加新東西時要跟著改的地方」跟著改。

## 詳細設計

### A. `ComputedStyle` 分組共用（RFC 0051 的 D8）

開始前：106 個頂層欄位、1472 bytes 的一個結構；每個元素的 cascade 從父的繼承值或初始值 clone 一份再改（`details.md` 的「RFC 開始前的程式」）。

| | 做法 | 成本 | 效益 | 對照 |
| --- | --- | --- | --- | --- |
| A1 | **照 Blink 分組**：欄位分成約 8 到 10 組（常用的不繼承、box、surround、visual、背景和邊框、繼承的文字、繼承的 rare、不繼承的 rare、SVG），每組是 `Arc<…>`、寫的時候 `Arc::make_mut`；`ComputedStyle` 只剩幾個指標和最常用的欄位 | 大：`layout`、`paint`、`style` 約 640 處直接讀欄位，要改成 `style.box_().display` 或產生的方法 `style.display()`；cascade 的寫入改成 setter；欄位的分組要照讀寫的頻率排 | clone 從 1472 bytes 變成約 100 bytes；沒改的組和父、兄弟共用，記憶體也少 | Blink 的 `ComputedStyleBase`（`make_computed_style_base.py` 產生、`DataRef<…>`） |
| A2 | **照 Stylo 分成 style struct**：約 15 到 25 個 struct（`Font`、`Text`、`Box`、`Position`、`Background`、`Border`……），繼承的 struct 沒改就直接沿用父的 `Arc` | 和 A1 差不多大；分法照 CSS 的規格，比較好判斷一個 property 屬於哪一組 | 和 A1 相近；繼承的 struct 共用得更徹底 | Stylo 的 `ComputedValues`、`StyleBuilder::mutate_*` |
| A3 | **只把少用的欄位搬出去**：保留現在的結構，把少用的欄位（grid、counter、transform、filter、SVG、rare 的文字……）搬進 2 到 3 個 `Arc` 的 rare 組 | 中：只有搬走的欄位的讀寫要改 | clone 的大小約減半；常用的欄位照舊直接讀 | Blink 早期的 `StyleRareNonInheritedData` |
| A4 | **少 clone**：不改結構，cascade 改成在一份重複用的暫存上算、只有結果和上次不同時才建新的 `Arc<ComputedStyle>`；matched properties cache 命中時直接沿用 `Arc` | 小到中 | 只省 clone 的一部分；不省記憶體 | Blink 的 matched properties cache 命中時共用 |

原本的決定是 A4、A3 先做，接著做到分組（第 1 條）。做到 A3 為止：
- **A4（5a）**：matched properties cache 存調整完的 `Arc<ComputedStyle>`，命中就共用（第 47 條）。量過命中後依位置的調整改了哪些欄位，才把調整的輸入放進 key（S2-key），而不是命中後檢查。新的 style 和舊的相同就沿用舊的 `Arc`（第 48 條），cascade 的暫時配置放在 stack 上（第 49 條）。
- **A3（5b）**：量 Speedometer 裡每個欄位不是初始值的比例，分成三個共用的組（第 56 條）：`rare`（grid、`animations`、`border-radius`、counter、`content`）、`backgrounds`、`svg`。`transitions`、`box-shadow`、`transform` 在部分 suite 常設，留在主體；outline 每次 cascade 都從 `medium` 開始，也留在主體。複製的大小 1,472 → 約 610 bytes。
- **分組（5c）不做**（第 57 條）：5b 之後複製只剩 style 的一成左右，分組要改約 640 處；記在「之後的可能」。

### B. selector 的比對：祖先的 bloom filter

開始前：`RuleSet` 照 id、class、tag 分桶，但每個候選的規則都完整比對，不傳祖先雜湊、沒有 bloom filter。

| | 做法 | 成本 | 效益 | 對照 |
| --- | --- | --- | --- | --- |
| B1 | **bloom filter**：`recalc_style` 往下走時把每個祖先的 tag、id、class 的雜湊放進 `selectors::bloom::BloomFilter`，往上走時拿掉；`RuleData` 存每個 selector 的祖先雜湊（`selectors::parser::AncestorHashes`），比對前先用 bloom filter 排除；從樹的中間開始重算時，先把那個元素的祖先放進去 | 中：`selectors` crate 已經支援（`matches_selector` 收 `AncestorHashes`、`MatchingContext` 收 bloom filter），要接的是走樹時的 push、pop 和 flat tree（shadow tree、slot：祖先照 flat tree 的父） | 有後代 combinator 的規則大多在 bloom filter 這一步就被排除；比對佔 style 約 17% | Blink 的 `SelectorFilter`、Stylo 的 `StyleBloom` |
| B2 | B1，加上桶的 `HashMap` 換成 FxHash、key 換成 atom（`LocalNameAtom` 這類） | B1 再加一點 | 再省 `RuleSet::collect` 和 cascade 裡的 SipHash（約 3% 到 4%） | Blink 的 `RuleSet` 用 `AtomicString` 當 key |
| B3 | 不做 | 無 | 無 | |

採用 B2（第 2 條），細節在第 3 段定：名字帶著預先算好的雜湊（`css::selector::name_hash`，ASCII 小寫的 FxHash），取代 atom（第 19 條）；bloom filter 照 Stylo 的 `StyleBloom` 跟著走訪，只在比對前同步到元素的祖先（第 20 條）；只有元素自己那棵樹的規則用它（第 21 條）；先不放 attribute 的名字（第 22 條）；`:has()` 的 subtree filter（第 23 條）；debug build 對略過的照樣比對（第 24 條）。

### C. SVG 的 presentation attribute 的快取

開始前：每次 cascade 都用 CSS 的 parser 重新解析 SVG 的 presentation attribute，連路徑的 `d` 都在這裡解析（`hints::svg_attribute_declarations`）。

| | 做法 | 成本 | 效益 | 對照 |
| --- | --- | --- | --- | --- |
| C1 | **存在元素上**：presentation attribute 解析成的宣告存在元素上（像 inline style 的 `Arc<CssPropertyValueSet>`），attribute 改了才重新解析 | 小：`dom` 在 attribute 改變時標記；style 讀快取 | 每個元素只解析一次；Stockcharts 的 style 約省兩到三成 | Blink 的 `Element::UpdatePresentationAttributeStyle`（`presentation_attribute_style_is_dirty`） |
| C2 | **共用的快取**：用（tag、attribute 的名字和值）當 key 的表，相同的 attribute 共用解析的結果 | 小到中：要決定表的大小和什麼時候清 | C1 之外，同樣 attribute 的元素只解析一次（SVG 的圖表裡很多 `fill="none"`） | Blink 的 `PresentationAttributeCache` |
| C3 | C1 加 C2 | C1 加 C2 | 最多 | Blink 兩者都有 |

採用 C3（第 3 條）：解析結果存在 `dom` 的元素上（第 12 條），共用快取每個 attribute 一筆、`d` 不進去（第 13 條）。`d` 的解析還在每個元素第一次算 style 時做（Stockcharts 的 style 約 5%），「之後的可能」。

### D. stylesheet 晚到時不整份重算

開始前：stylesheet 變了就整份重算、清掉 matched properties 的快取；script 讀幾何時用手上已有的 stylesheet 先算 style，`<head>` 的 stylesheet 不擋 parser 插入的 script。`start_loads` 每次都先算 style 找背景圖片，不等 render-blocking 的 stylesheet：jQuery 的頁面每個 iteration 在有 0、1、2、3 份 stylesheet 時各整份算一次（第 1 段查到的，`details.md` 的「整份重算的觸發點」）。

| | 做法 | 成本 | 效益 | 對照 |
| --- | --- | --- | --- | --- |
| D1 | **script-blocking stylesheet**：照 HTML 的「a style sheet that is blocking scripts」，parser 插入的 script 等到之前的 stylesheet 都載入才執行 | 中：`html` 的 parser 和 renderer 的載入；網頁看得到的行為變了（更像瀏覽器），WPT 有這部分的測試 | script 不會在 stylesheet 到齊前讀幾何，少一次整份重算；也修掉 script 讀到半套 style 的相容性問題 | Blink 的 `HTMLParserScriptRunner`、`Document::IsScriptExecutionReady` |
| D2 | **照規則的差異只重算受影響的元素**：stylesheet 變了時比較新舊的 rule set，只標記符合新增、刪除的規則的元素（照它們的 rightmost 的 id、class、tag 找） | 大：要 rule set 的 diff 和照 selector 找元素的 invalidation；`:has()`、`@media`、`@keyframes`、`@font-face` 這類要退回整份 | 所有 stylesheet 的改變都受益（也包括 `<style>` 改了） | Blink 的 `RuleSetDiff`、`StyleEngine::InvalidateForRuleSetChanges` |
| D3 | D1 加 D2 | 兩者 | 最多 | Chrome 兩者都有 |
| D4 | 先查清楚：第 1 段先確認 Complex-DOM 裡 `<link>` 晚到的那幾次是哪一種情況觸發的（script 讀幾何、還是別的），以及新 document 的第一次 style 為什麼每個 iteration 有 5 次 | 小 | 決定 D1、D2 的前提 | |
| D5 | **render-blocking 的 stylesheet 沒到齊時，`start_loads` 不算 style**：還有 stylesheet 在載入時跳過找背景圖片（記下還沒找，document 的 `load` 照樣等它），最後一份到了（或失敗）之後再算一次、找一次（第 1 段查到之後加的） | 小：`FrameView::start_loads` 一處，加上 stylesheet 到齊時要再跑一次 `start_loads` 的保證；網頁看不出差別（背景圖片晚一點開始載入，和 Chrome 一樣） | jQuery 每個 iteration 從 4 次整份重算變 1 次；Preact 的 `<link>` 晚到的那幾次（script 沒讀幾何的話）不再發生 | Blink 的背景圖片在 lifecycle 的 style recalc 裡發現（`ElementStyleResources::LoadPendingResources`），render-blocking 的 stylesheet 沒到之前不跑 lifecycle |

採用 D4（第 4 條）先查清楚。查到的觸發點是 `start_loads`，不是 script 讀幾何，所以加上 D5；之後選了 D5 加 D3（第 11 條）：
- **D5（第 2 段）**：拿掉 Speedometer 裡大部分的整份重算（第 17 條）。
- **D1（4a）**：照規範等的 script（第 26 到 30 條）：parser 插入、`media` 符合、還在載入的 `<link>` 擋 parser 插入的 script 和 `defer`、module；哪些擋照 Blink。
- **D2 前先查剩下的整份重算（4b）**：Complex-DOM 多的那次是 `matchMedia` 為了知道還沒排版的子 frame 的大小，把子 frame 用還不齊的 stylesheet 算了 style。改成子 frame 在等 stylesheet 時只排版外面的 frame（第 31 條，K1a），以及 `start_loads` 算子 frame 的 style 前先給它 iframe 的大小（第 33 條，L1）：子 frame 第一次的 style 就有對的大小，和 Blink 的 frame 在父 frame 排版時就有大小一樣。
- **D2（4c 到 4e）**：照 Blink 的 `InvalidateForRuleSetChanges` 和 `RuleSetDiff`（第 32、34 到 44 條）：多了、少了的 sheet 的規則（CSSOM 改過的 constructed sheet 只算增減的規則）照特徵找要重算的元素，最右邊那一節沒有特徵時找左邊最近有的那一節；shadow root 的也做，`:host` 重算 host、`::slotted()` 重算分配到 slot 的節點。同一套後代的 invalidation set 也用在 class、id、attribute、狀態的改動（4e），取代原本「規則在後代位置用到就重算整棵子樹」。整份重算只剩：選擇器沒有特徵、`:has()`、`@keyframes`、留下的 sheet 順序變了、media 的結果變了。
- 4b 帶出一個原本就有的錯：pseudo-element 用到 viewport 單位時，viewport 改了不會重算它（以前碰巧有別的整份重算蓋過去），5a 時修好。

### 其他小項

- **`url()` 的解析**：算 style 時對 base URL 解析，結果沒有快取（WebComponents 全部 CPU 的約 5%）。照 Chrome 改成在解析 stylesheet、inline style、presentation attribute 時就解析好（第 15 條）。
- **transition 的檢查**（`ElementAnimations::style_change`，cascade 的 5%）：沒有 transition 的元素跳過。
- **`recalc_style` 本身**：草稿時量到的 16% 是舊的 build 的 inline 方式造成的，第 1 段重新取樣只有 4.2%（`Arc` 的計數、`ScopeCascade` 的 drop、旗標的寫入），不單獨處理。

### 量測

每段做完用 `ci.py speedometer --trace` 和 `--sample` 量 style 的時間和元素數，方法照 `details.md` 的「方法」。`--trace` 的報告多一份各階段的比較（第 6 條，第 1 段做）：每個 suite 計時區間裡 style、建 box tree、layout 等的 self time，kun 和 Chrome 並列。重算的元素數照舊手動跑 debug log：kun 的 trace 只記 span 的名字，要記欄位得改 rusty_v8 fork 的 C++、重編 V8。

### 測試

- 每個方向的結果不變：現有的 `style` 的單元測試、reftest、WPT（`css/` 底下的 selector、cascade、SVG 的 presentation attribute）、情境。
- 增量和快取都有 debug build 的交叉檢查，像 RFC 0058 第 2 段的做法：matched properties cache 命中時照樣 cascade 加上調整再比（A4）；presentation attribute 的快取對照重新解析（C1、C2）；bloom filter 略過的規則照樣比對、確認不符合（B1）；stylesheet 改動和後代的 invalidation set 漏掉的元素，用不套用 set 的走訪找出來（D2）。
- 單元測試涵蓋 flat tree（shadow tree、slot）、從中間開始重算、`:host`、`::slotted`、CSSOM 的增減、被移除又插回的子樹的動畫。
- script-blocking stylesheet（D1）：WPT 的 `html/semantics/document-metadata/the-link-element`、`the-style-element` 和 `html/semantics/scripting-1/the-script-element/` 的相關測試，加上 `renderer` 的測試。
- 每一段都跑了 mutation test（`mutants-*.toml`），結果在 `progress.md` 的「做過的檢查」。

### 分階段

1. **查清楚和量測的工具**：D4、`recalc_style` 本身的時間（不改行為）；`ci.py speedometer --trace` 的各階段報告。
2. **便宜的**：C1、C2、B2 的雜湊、`url()` 在解析時解析（U2a）、transition 的跳過、D5。
3. **bloom filter**（B1）和名字預先算好的雜湊（B2 的另一半，取代原本的 atom）。
4. **stylesheet**：4a D1；D2 分成 4b 子 frame 的環境（K1a、L1），4c document 的 stylesheet 改動用 invalidation set，4d shadow root 和 constructed stylesheet 的 CSSOM 改動，4e class、id、attribute、狀態的改動也用 invalidation set（第 39 條）。
5. **`ComputedStyle`**：5a A4、5b A3；5c 分組（A1 或 A2）沒有做（第 57 條）。
6. **量測和決定**：6a V2、V4（第 58 條），之後結束（第 59 條）。

先做便宜的（第 5 條），每段量一次（`progress.md` 的「效能的量測」）。

## 缺點

- **每個元素的成本沒有減半**（目標的第一條）：NewsSite-Next 的 style 16 → 13 ms。剩下的時間分散在套用宣告、selector 比對、找候選的規則，沒有單一的大頭（`details.md` 的「第 6 段前：style 的時間花在哪」）。
- **快取和增量多了很多條件**：C、D2、A4 都是「什麼時候可以不重算」，漏標一處就是畫面不更新。新的 CSS property、at-rule、selector 的種類、依位置的調整的輸入都要跟著改對應的地方；靠 debug build 的交叉檢查守，release build 不檢查。
- **D1 改變網頁看得到的行為**：script 執行的時機變了（和瀏覽器一致）。
- **bloom filter 的維護**：走樹時要正確地同步祖先，flat tree 的祖先和 DOM 的祖先不同；只用在元素自己那棵樹的規則。
- **`ComputedStyle` 的讀法不一致**：少用的欄位經過組（`style.rare.x`），其他的直接讀；新的欄位放哪裡要照量到的比例判斷。

## 理由和其他做法

- **為什麼先處理每個元素的成本，不先做後代的 invalidation set**：NewsSite 重算的元素數和 Chrome 一樣，時間卻是 4 倍；invalidation set 只減少要算的元素，對這類頁面沒有幫助。後代的 invalidation set 後來在 4c 到 4e 做了，是為了 stylesheet 的改動不再整份重算，順便用在 class、id、attribute、狀態的改動。
- **為什麼不先做文字**：TipTap 的建 box tree 是文字的 shaping，和 style 是不同的程式，量測也要用不同的方法；放進同一份 RFC 會讓範圍太大。
- **不做**：style 的差距（117 ms）是 Speedometer 裡 kun 和 Chrome 的差距中，script 以外最大的一塊。
- **為什麼停在 5b，不做全部分組**：5b 之後複製 `ComputedStyle` 只剩 style 的一成左右（效益上限約 9 ms），分組要改約 640 處讀欄位的地方（第 57 條）。
- **為什麼在 6a 之後結束**：剩下最大的一塊是套用宣告（`Recascade::cascade`，style 的 13% 到 38%），但分散在 `cascade_blocks`、`cascade_start`、`apply`、`Arc` 的釋放、自訂屬性，要先細查才知道能拿到多少；其他候選每個都在 1 ms 上下。這些記在「之後的可能」，附上數據（第 59 條）。

## 先例

`details.md` 的「和 Chromium 的對照」。
- **Blink**：分組的 `ComputedStyle`（`DataRef`）、`SelectorFilter`、`PresentationAttributeCache`、script-blocking 的 stylesheet、`RuleSetDiff`。
- **Stylo**（Firefox）：分成 style struct 的 `ComputedValues`、`StyleBloom`，同樣用 kun 現在用的 `selectors` crate。

## 已決定的事項

2026-10-19 接受。

1. **`ComputedStyle`：先 A4、A3，再做到分組**（2026-10-19，使用者）：A4（matched properties cache 命中時沿用 `Arc`、少 clone）和 A3（少用的欄位搬進 2 到 3 個 rare 組）成本小、效益看得到，先做；接著做到分組（A1 照 Blink 或 A2 照 Stylo，到第 5c 段再選）。使用者預計全做，不在 A3 之後停下。
2. **selector 的比對：B2**（2026-10-19，使用者）：祖先的 bloom filter（`selectors` crate 已經支援），加上 `RuleSet` 的桶換成 FxHash、key 換成 atom。不選只做 bloom filter（B1：SipHash 的雜湊照樣佔 3% 到 4%）；不選不做（B3）。
3. **SVG 的 presentation attribute：C3**（2026-10-19，使用者）：解析結果存在元素上、attribute 改了才重新解析（C1），加上用（tag、attribute 的名字和值）當 key 的共用快取（C2），和 Blink 的 `Element::UpdatePresentationAttributeStyle` 和 `PresentationAttributeCache` 一樣。不選只做其中一個。
4. **stylesheet 晚到：先查清楚（D4）再選**（2026-10-19，使用者）：第 1 段確認 Complex-DOM 裡 `<link>` 晚到的整份重算是什麼觸發的，以及新 document 的第一次 style 為什麼每個 iteration 有 5 次，再在 D1（script-blocking stylesheet）、D2（照規則的差異只重算受影響的元素）、D3（兩者）之間選。
5. **先做便宜的**（2026-10-19，使用者）：照「分階段」的順序，風險低的先拿到效果，每段量一次；A 放在第 5 段。不選先做 A（改動最大，前面幾項做完之後 A 的佔比也會變）。
6. **量測做成 `ci.py speedometer --trace` 的報告**（2026-10-19，使用者）：第 1 段把分析的腳本做進 `--trace`：每個 suite 計時區間裡各階段的 self time，kun 和 Chrome 並列；以後的效能 RFC 也用得到。重算的元素數照舊手動跑 debug log（kun 的 trace 只記 span 的名字）。不選每段手動跑暫時的腳本（量法可能每次不同）。
7. **A 的讀寫方法留到第 5 段決定**（2026-10-19，使用者）：要不要先把 640 處讀欄位的地方改成方法（`style.display()`），到第 5 段量過 A4、A3 之後再決定。
8. **各階段的報告用固定的分類（P1）**（2026-10-19，使用者，第 1 段）：script、style、建 box tree、layout、paint、parse、GC、canvas、其他；kun 和 Chrome 各自的 slice 名稱對到分類，對照表在 `speedometer.py`。`--trace` 跑完寫出 `phases.md`：每個 suite 一列，kun 和 Chrome 並列、差多少，加上合計，最後列出沒分類到的前幾名 slice。不選不分類、只列兩邊的前幾名（P2：兩邊的名字不同，要自己對）。
9. **stylesheet 算成「變了」的原因留成 `style=debug` 的 log；觸發點用暫時的 log 查（Q1）**（2026-10-19，使用者，第 1 段）：原因的 log 便宜，以後查同樣的問題用得到；找觸發點的 log 和 backtrace 查完拿掉。不選全部用暫時的 log（Q2：下次要再加一次）。
10. **`recalc_style` 本身的 16% 用暫時的 `#[inline(never)]` 拆**（2026-10-19，使用者，第 1 段）：重新取樣看時間落在哪個 helper，查完還原、不 commit。不加 span：每個元素進出 span 的成本會影響量到的數字。後來沒用到：用現在的 build 重新取樣，`recalc_style` 自己只剩 4.2%，用 `atos -i` 把位址對回 inline 的函式就拆得開，不必改程式（`details.md` 的「style 的取樣」）。
11. **stylesheet 晚到：D5 加 D3（D1 和 D2）**（2026-10-19，使用者，第 1 段之後）：D5 便宜、效果最大，放進第 2 段；D1、D2 在第 4 段，4a D1（比較小）、4b D2。D2 開始前先用各階段的報告和 `style=debug` 的 log 看 D5 之後還剩哪些整份重算，再決定涵蓋哪些情況、哪些退回整份重算。D1 對 Speedometer 的效果小（觸發點不是 script 讀幾何），做它是為了相容性、和 Chrome 一致。D1、D2 各自的實作選擇到那一段開始時再決定。不選只做 D5（script 讀到半套 style 的問題留著，`<style>` 改了照樣整份重算）。
12. **第 2 段：presentation attribute 的解析結果存在 `dom` 的元素上（C1a）**（2026-10-19，使用者）：和 inline style 的 `Arc<CssPropertyValueSet>` 一樣；元素的 attribute（id、class、style 以外的）改了就標成要重算，style 在 cascade 時沒有快取才解析、存回去。只快取元素自己的 attribute 決定的部分，HTML 的 table 讀祖先的 `cellpadding`、`border` 的那部分照舊每次算。debug build 每次重新解析、和快取比對。和 Blink 的 `presentation_attribute_style_`、`presentation_attribute_style_is_dirty` 一樣。不選放在 `StyleEngine` 的 `NodeIdHashMap`（C1b：元素移除時要自己清、多一次查表）。
13. **第 2 段：共用快取每個 attribute 一筆（C2a），`d` 不進去**（2026-10-19，使用者）：key 是（tag、attribute 的名字、值），每個 `StyleEngine` 一份、有上限、超過就清掉；新建的元素（React 每個 iteration 重建的 SVG）也能共用。`d` 的值幾乎都不同，只存在元素上（C1a）。不選照 Blink 用整組 attribute 當 key（C2b：一個 attribute 不同就不命中）。做的時候發現 key 不必有 tag：哪個元素拿哪個 attribute 在查快取之前就決定了，解析只看名字和值，所以 key 是（attribute 的名字、值），不同元素的 `fill="none"` 也共用。
14. **第 2 段：rule set 和快取的雜湊換成 FxHash，名字預先算好的雜湊移到第 3 段（B2a）**（2026-10-19，使用者）：`RuleSet` 的 id、class、tag 的桶，matched properties cache 的 `entries`，`cascade_blocks` 的 `custom_seen`。class 和 id 預先算好雜湊改動面大，第 3 段的 bloom filter 也需要它，一起做。不選這一段就做（B2b）。原本寫的是「key 換成 atom」：第 3 段開始時查到 `string_cache` 的動態 atom 不能用（全域的表，drop 時上鎖，而 DOM 的名字和 `css` 的值可能在 GC 的 finalizer 裡 drop），改成名字帶著預先算好的雜湊（第 19 條）。
15. **第 2 段：`url()` 照 Chrome 在解析時就解析好（U2）**（2026-10-19，使用者）：stylesheet、inline style、presentation attribute 在解析時就對當時的 base URL 解析，算 style 時不再解析；同時修掉 `docs/limitations.md` 的「Style」裡 `url()` 在算 style 時才解析的那一條。css 的 parser 和 `dom` 的 `inline_style_of` 要拿到 base URL。和 Blink 的 `CSSUrlData` 一樣。不選在宣告上記住上次的解析結果（U1：便宜、行為不變，但限制留著）。
16. **第 2 段：`rendered` 跟著走樹帶下來（T1）**（2026-10-19，使用者）：`recalc_style` 往下走時帶著父有沒有 render，不再為每個元素往上走祖先；animations 的表裡沒有這個元素、style 也沒有 animation 或 transition 時直接返回（做的時候改成借表、查一次就返回：查表裡有沒有這個元素本來就要借表）。debug build 照原本的做法比對。不選只加提早的檢查（T2：O(深度) 的成本留著）。
17. **第 2 段：render-blocking 的 stylesheet 沒到齊時 `start_loads` 不算 style（D5a）**（2026-10-19，使用者）：只記下背景圖片還沒找，`load` 照樣等；最後一份到了之後，`did_receive_resource` → `advance_document` → `start_loads` 會再跑一次。不選把找背景圖片搬進 rendering update（D5b：最像 Chrome，但背景圖片要等下一個 frame 才開始載入，`load` 的時機會變；留到之後）。
18. **第 2 段：`url()` 完全照 Chrome 在解析時解析（U2a）**（2026-10-19，使用者）：`dom` 記住每份 document 的 base URL（`<base>`、document 的 URL、`about:blank` 和 `about:srcdoc` 從父 frame 來的 fallback），`style` attribute 在設定時就用它解析；CSSOM 和 Web Animations 的 keyframe 用當時的 document base URL；`<link>` 的 stylesheet 用它自己的 URL，`<style>` 用解析時的 document base URL；presentation attribute 在解析時（C1a）用當時的 base URL。算 style 時不再解析，base URL 變了也不再走過每個元素（拿掉 `uses_document_url`）。和 Blink 的 `CSSUrlData`、`CSSParserContext` 的 base URL 一樣。不選第一次算 style 時才解析 `style` attribute 的 `url()`（U2b：只改 `css`、`style`，但設了 attribute、還沒算 style 之前 base URL 就變了的話和 Chrome 不同）。
19. **第 3 段：名字帶著預先算好的雜湊，取代 atom（H1）**（2026-10-19，使用者）：一律是 ASCII 小寫之後的雜湊（`css` 的一個函式），bloom filter 和 `RuleSet` 的 id、class 的桶共用；`dom` 在設定 `id`、`class` 時算好存著，`CssIdent` 在要的時候算（只有建 rule set 時要），tag 和 namespace 在 push 時算。小寫的雜湊兩邊一致，quirks 模式的文件（id、class 不分大小寫）也能用同一份。不選每份 document 一個 interner（H2：stylesheet 和 document 無關，要再對應一次）、`string_cache` 的動態 atom（H3：違反 GC finalizer 的規則）。對照：Blink 的 `AtomicString`（每個 thread 一張表、預先算好雜湊）。
20. **第 3 段：bloom filter 照 Stylo 的 `StyleBloom` 跟著走訪，只在比對前同步（W1 加上 lazy）**（2026-10-19，使用者）：留一個已經 push 的祖先的 stack，要比對一個元素之前，pop 到它的 DOM 父元素（shadow tree 裡到 shadow root 為止），沒有的就從那裡往上補；只是走過、不用比對的元素沒有成本。不選照 Blink 的 `SelectorFilterParentScope` 在進出子樹時 push、pop（W2：kun 的走訪是用 stack 的 DFS，沒有離開節點的時機，要改走訪的結構）；Blink 的 lazy（`EnsureParentStackIsPushed`）這裡也有。
21. **第 3 段：只有元素自己那棵樹的規則用 bloom filter（F3）**（2026-10-19，使用者）：`:host`、`::slotted()`、UA shadow tree 的 pseudo 跨過 shadow 的邊界，祖先不是 bloom 裡的那些，整批不用；`<use>` 的 copy 和 `querySelector` 這類單次的比對也不用。不選照 Blink 讓祖先鏈接到 host、跨邊界的 selector 不給雜湊、所有來源都用：要自己判斷跨邊界的 combinator（`selectors` 的 `AncestorHashes` 遇到 slot 的 combinator 會繼續收），那幾個來源的規則又通常很少；以後要做，加上那個判斷、打開那幾個來源即可。
22. **第 3 段：bloom 裡先不放 attribute 的名字（F4）**（2026-10-19，使用者）：只放 tag、namespace、id、class，和 `selectors` 預設收的一致。Blink 和 Stylo 這幾年都加上了 attribute 的名字，第 3 段量過之後有需要再加。
23. **第 3 段：做 `:has()` 的 subtree filter（F5）**（2026-10-19，使用者）：`add_element_unique_hashes` 放進同樣的雜湊，對照 Blink 的 `:has()` fast reject filter。
24. **第 3 段：bloom filter 排除的規則，debug build 照樣完整比對、確認不符合（F6）**（2026-10-19，使用者）：push、pop 錯了現有的測試就會抓到，和第 2 段的做法一樣。
25. **第 3 段之後：`forget_moved` 只看這次 recalc 走到的被移除的根**（2026-10-19，使用者）：量測發現 TodoMVC-jQuery 的 `recalc_style` 七成以上花在 recalc 結尾的 `Animator::forget_moved`，它每次走過整個 `Animations::removed`（有過動畫的頁面上每個被移除的子樹的根，GC 回收之後才拿掉）。改成走訪時 `moved()` 找到的根記下來，結尾只檢查它們：插回 document 的根一定會被重新算 style、被走到。debug build 照原本的做法確認這份 document 裡沒有漏的。對照：Blink 沒有這個 set，元素離開 document 時（`DetachLayoutTree`）就清掉 computed style，插回來時沒有移除前的 style。不選照 Blink 在移除時清掉 style（移除時要走過整棵子樹，讀移除前的 style 的地方也要改，改動大）。
26. **第 4a 段：照規範等的 script（J1）**（2026-10-19，使用者）：parser 插入、會擋住 parser 的 classic script（external 和 inline）和 parsing 結束時才跑的那一批（`defer`、module）等到 script-blocking stylesheet 都載入（或失敗）才跑，`DOMContentLoaded` 跟著延後；`async` 和 script 插入的不等。對照 Blink 的 `HTMLParserScriptRunner`、`Document::IsScriptExecutionReady()`。
27. **第 4a 段：哪些 stylesheet 擋 script 照 Blink（J2）**（2026-10-19，使用者）：parser 插入的 `<link rel=stylesheet>`、`media` 在開始載入時符合環境、還連在 document 上、還在載入中；被移除或載入失敗就不再擋。`<link>` 的「parser 插入的」記在每個元素都有的 `ScriptFlags` 的 `parser_inserted`（Blink 的 `created_by_parser_`）。不選所有載入中的 stylesheet 都算（就是現在 render-blocking 的條件：script 插入的、`media` 不符合的、被移除的也會擋，比 Chrome 等得多）。kun 沒有 `@import`，`<style>` 不會擋。
28. **第 4a 段：父 frame 的 stylesheet 不算（J3）**（2026-10-19，使用者）：和 Blink 一樣只看自己的 document；規範的「container document has a style sheet that is blocking scripts」不做（子 frame 要知道父 frame 的載入、父 frame 的 stylesheet 到了也要推進子 frame）。
29. **第 4a 段：render-blocking 不跟著改（J4）**（2026-10-19，使用者）：畫面、背景圖片（D5a）和 `load` 照舊等所有載入中的 stylesheet；改成 J2 的條件會動到畫面出現的時機，另外再看。
30. **第 4a 段：測試照 J5**（2026-10-19，使用者）：shell 的測試用延遲回應的 stylesheet，涵蓋該等的（inline、external、`defer`、module）和不該等的（`async`、script 插入的 script 和 `<link>`、`media` 不符合、中途移除、載入失敗）；`geometry::a_render_blocked_document` 改成新的行為；照 WPT 的結果更新預期（css-tables、`script-text-modifications`、`moving-between-documents` 的不穩定）和 `docs/limitations.md`。
31. **第 4b 段：media query 只排版父 frame 來知道 iframe 的大小（K1a）**（2026-10-19，使用者）：4b 開始前查剩下的整份重算（`details.md` 的「4b 前剩下的整份重算」）：Complex-DOM 的 TodoMVC 每個 iteration 多一次整份重算，是 `matchMedia` 這類 media query 的判斷（`dom_api::media_query_list::environment`）為了知道還沒排版的子 frame 的大小，把子 frame 排版、用還不齊的 stylesheet 算了它的 style。改成只排版父 frame，iframe 的大小從父 frame 的排版拿，不碰子 frame 的 style。對照 Blink：frame 的大小來自 iframe 在父 document 裡的排版。不選子 frame 還在等 stylesheet 時用 0×0 或上次的大小（K1b：等待期間的 `matchMedia`、`<img srcset>` 拿到錯的大小）。實作時縮小成只在子 frame 等 stylesheet（render-blocked）時這樣做，其他還沒排版的子 frame 照舊排版：只排版父 frame 拿到的是 iframe 現在的大小，script 改了 iframe 的大小之後讀 `matches` 就先拿到新的結果、記下來，下一次 rendering update 不再發 `change`（WPT 的 `picture-resize-complete.html` 逾時）；排版過的子 frame 在下一次 rendering update 前照上次排版的大小判斷，`change` 照常發。等待中的子 frame 照舊是讀到新結果就不再發 `change`（`docs/limitations.md` 的「Style」）。
32. **第 4b 段：照規則的差異只重算受影響的元素，照 Blink 的 `RuleSetDiff`（K2a）**（2026-10-19，使用者）：K1a 做完量過之後做（細節改由第 34 到 39 條決定，分到 4c 到 4e）。stylesheet 變了時找出新增和移除的規則，用它們最右邊那一節的 id、class、tag、attribute 找出可能受影響的元素，只重算那些；最右邊那一節沒有這些特徵、有 `:has()`、`@keyframes` 改了、media 變了時退回整份重算。對照 Blink 的 `StyleEngine::InvalidateForRuleSetChanges`。不選只處理多了一份 stylesheet（K2b）、先不做（K2c）。app 每次 render 時 Complex-DOM 約 6,650 個元素的子樹重算屬於後代的 invalidation set（「之後的可能」），這一段不處理。
33. **第 4b 段：`start_loads` 算子 frame 的 style 前，先從父 frame 的排版拿 iframe 的大小（L1）**（2026-10-19，使用者）：K1a 量過之後（`details.md` 的「4b 前剩下的整份重算」），整份重算的次數沒變，只換了位置：stylesheet 到了之後，`FrameView::start_loads` 為了找背景圖片，用還沒排版的子 frame 的 0×0 算了 style，排版之後 media 變了又整份重算。改成 render thread 開始載入時，子 frame 還沒排版、這次會算它的 style（不在等 stylesheet）的，先排版外面的 frame、把 iframe 的內容框給它當 viewport，第一次的 style 就照它的大小。對照 Blink：子 frame 的 `LocalFrameView` 在父 frame 排版時就從 iframe 拿到大小，算 style 時大小是對的。不選 `start_loads` 不算還沒排版的子 frame 的 style、背景圖片等第一次排版之後再找（L2：子 frame 的 `load` 要等一次 rendering update，頁面隱藏時可能一直不發），也不選先不處理（L3：K2a 遇到 media 變了退回整份重算，省不掉這次）。
34. **K2a 的比較單位：整份比較，加上 CSSOM 改動時逐條比較（N1c）**（2026-10-19，使用者）：照 Blink 的兩層。一，比較改動前後 tree scope 的 stylesheet 清單（`CompareActiveStyleSheets`）：新增、移除的 stylesheet 用它的全部規則判斷；`<style>` 的文字改了就是舊的整份移除、新的整份加入（Blink 換一份新的 sheet，不比對文字）。二，同一份 constructed stylesheet 被 `insertRule`、`deleteRule` 這類 CSSOM 改動時，只用增減的規則判斷（Blink 的 `RuleSetDiff`；kun 只有 constructed stylesheet 有 CSSOM，新舊兩份 contents 的規則多半是同一批物件，比較指標就知道增減了哪些）。不選只做第一層（N1a），也不選連 `<style>` 的文字都逐條比較（N1b：比 Blink 多做）。
35. **找出受影響的元素：照 Blink 建後代的 invalidation set（N2b），這一階段先只用在 stylesheet 改動（甲）**（2026-10-19，使用者）：照 Blink 的 `RuleFeatureSet`，每個 id、class、attribute、tag 對到它的 invalidation set（自己要不要重算，和後代裡要重算的特徵）；stylesheet 改動時走過 tree scope 的每個元素，用元素自己的 id、class、attribute、tag 查變動規則的特徵表（hash，不逐一比較），把查到的 set 排下去（`StyleEngine::InvalidateForRuleSetChanges`、`ScheduleRuleSetInvalidationsForElement`，和只有 tag 的規則從根往下套的 type set）。兄弟的 invalidation set 沿用 kun 現有的。不選只看最右邊那一節的特徵（N2a：最右邊那一節沒有特徵的規則，`.a *`、`.a > :first-child`，只能整份重算；當時說成「`.a span` 會重算所有 `span`」是錯的：Blink 在 stylesheet 改動時，只有標籤的最右邊那一節也是從根往下找，一樣重算所有 `span`）。N2b 在 stylesheet 改動多出來的只有這類規則，主要的效益在 4e。class、id、attribute 的改動也換成這些 set（取代 `AncestorFeatures` 的整棵子樹重算）在 4e，不和 stylesheet 的一起做（乙：牽動的測試多，出錯難分辨是哪一邊）。
36. **特徵從哪裡來、什麼時候退回整份重算（N3）**（2026-10-19，使用者）：特徵照 Blink 從每一節收集：最右邊那一節是「自己」，descendant、child combinator 左邊的是「後代」，sibling combinator 左邊的用兄弟的 set，`:is()`、`:where()`、`:not()` 裡面的也收集。退回整份重算：選擇器每一節都沒有 id、class、attribute、tag（`*`、`:root`、只有狀態；Blink 的 `needs_full_recalc_for_rule_set_invalidation`）、有 `:has()`、變動的 stylesheet 有 `@font-face`、`@layer`、`@keyframes`、`@property`，和 media 的結果變了。和 Blink 不同的三處（`:has()` 有專門的 invalidation、`@keyframes` 只重算有動畫的元素、media 變了照變動的規則判斷）先整份重算：Speedometer 裡現在沒有這三種的整份重算，量到需要時再做。
37. **範圍包括 shadow root（N4b）**（2026-10-19，使用者）：shadow root 的 stylesheet 改動也用 invalidation set，照 Blink 對 shadow tree 的處理：變動的規則有 `:host` 的重算 host，有 `::slotted()` 的重算指派到這棵 shadow tree 的 slot 的元素，其他從 shadow root 往下找。同一份 constructed stylesheet 被幾個 scope adopt，CSSOM 改動時每個 scope 各自照第 34 條判斷。不選 shadow root 照舊整棵 shadow tree 重算（N4a）。
38. **測試（N5）**（2026-10-19，使用者）：`style` 的單元測試涵蓋只重算符合的元素（自己、後代、兄弟）、移除 stylesheet、`<style>` 的文字改了、`insertRule` 和 `deleteRule`、每一種退回整份重算、`:host` 和 `::slotted()`、重算的元素 style 變了後代照樣繼承；shell 的測試陸續插入小的 `<style>`；debug build 照「測試」一節的交叉檢查，部分重算之後再整份算一次比對；mutation test；WPT 的 `css/css-cascade/`、`css/selectors/`、`css/cssom/`、`css/css-scoping/`、`css/css-transitions/`；做完再統計一次各 suite 的重算數。
39. **D2 重新分段**（2026-10-19，使用者交給 AI 評估）：第 34 到 38 條比第 32 條大很多，分成：4b 子 frame 的環境（K1a、L1，已做完）；4c document 的 stylesheet 改動用 invalidation set（第 34 條的第一層、第 35、36 條，document 的 scope）；4d shadow root（第 37 條）和 constructed stylesheet 的 CSSOM 逐條比較（第 34 條的第二層）；4e class、id、attribute 的改動也用 invalidation set（第 35 條的後半，Svelte 那次子樹重算靠它）。每一段可以單獨審核、commit；第 4 階段的檢查在 4e 做完時跑，4c、4e 之後各量一次。
40. **第 4d 段：shadow root 的 stylesheet 改動退回全部重算時的範圍照 Blink（P1a）**（2026-10-19，使用者）：整棵 shadow tree、host 和指派到它的 slot 的元素；host 的 light DOM 更深的後代只在繼承的值變了時跟著重算。不選維持現狀（P1b：host 的整棵子樹連 light DOM 的所有後代都重算，一個包住整個 app 的 web component 等於整頁重算）。
41. **第 4d 段：CSSOM 改動比較最上層的規則（P2a）**（2026-10-19，使用者）：第 34 條的第二層的做法：多了、少了的 sheet 裡同一個 `Arc` 的規則互相抵消，只剩真的增減的；抵消的規則在新舊兩邊的位置（前面留下的 sheet 有幾份）和先後要一樣，否則整份重算。改了 `@media`、`@supports` 裡面的規則時整個 group rule 算變動（Blink 的 `RuleSetDiff` 記的是被改的規則，改 group rule 裡面的規則時記那個 group rule）。document 和 shadow root 的 adopted stylesheet 都適用。不選往 group rule 裡面逐條比較（P2b：比 Blink 細）。
42. **第 4d 段：測試（P3）**（2026-10-19，使用者）：`style` 的單元測試涵蓋 shadow tree 裡的規則只重算 shadow tree 裡符合的、`:host` 的只重算 host、`::slotted()` 的重算指派到 slot 的元素、退回全部重算時照第 40 條的範圍、document 和 shadow root 的 adopted stylesheet 被 `insertRule`、`deleteRule`、`replaceSync` 改了、同一份 constructed stylesheet 被幾個 scope adopt；debug build 的交叉檢查擴大到 shadow tree；mutation test；WPT 的 `css/css-scoping/`、`css/cssom/`、`shadow-dom/`。4d 不量（第 39 條）。
43. **第 4e 段：元素的狀態也用 invalidation set（R1a）**（2026-10-19，使用者）：class、id、attribute（第 35 條）之外，`:hover`、`:focus`、`:checked` 這類狀態改了也照 invalidation set 只重算規則可能配到的後代（Blink 的 pseudo-class invalidation set）。4e 開始前查到（暫時的 log）：Svelte-Complex-DOM 那次約 6,650 個元素的子樹重算來自 `<ul class="todo-list show-priority">` 的 class 改變（底下 200 個 `li`），`html` 的 `data-visible-section` 改了也是整份 document 的子樹重算。不選狀態維持整棵子樹重算（R1b）。
44. **第 4e 段：跨 tree scope 的規則照 Blink（R2 甲）**（2026-10-19，使用者）：shadow tree 的規則用到 host 的特徵（`:host(.a) .b`）時，後代的 set 套進 host 的 shadow tree；`::slotted()` 前面用到 slot 的特徵時照 slot 找指派到它的元素；UA shadow tree 的部件（`input.x::placeholder`）和 list box 的規則維持重算整棵子樹（Blink 也是另外的路徑，子樹很小）。不選跨 tree scope 的都維持整棵子樹（乙）。
45. **第 4e 段：debug build 的交叉檢查（R3）**（2026-10-19，使用者）：照「測試」一節，舊的做法（整棵子樹）會重算、invalidation set 跳過的元素，在 debug build 重新 cascade 一次，確認和留著的 style 相同。
46. **第 4e 段：測試和量測（R4）**（2026-10-19，使用者）：`style` 的單元測試涵蓋自己、後代、主體沒有特徵時整棵子樹、`:is()`、狀態、`:host()`、`::slotted()`、維持整棵子樹的情形；mutation test；WPT 的 `css/selectors/invalidation/`、`css/selectors/`、`css/css-scoping/`、`shadow-dom/`；做完統計各 suite 的重算數，並用 `ci.py speedometer --trace` 量 style 的時間（第 39 條）。第 4 階段的檢查（`cargo test --workspace`、`cargo fmt --check`、clippy）在 4e 做完時跑。
47. **第 5a 段：matched properties cache 命中時共用 `Arc`，先量再決定（S2a）**（2026-10-19，使用者）：S2 是快取存 `Arc<ComputedStyle>`，命中時先只檢查依位置的調整（blockification、root、parent 帶下來的 `text-decoration`、`appearance`、flex item 的 `z-index` 等）會不會改到欄位，不會就直接共用那個 `Arc`、零複製，會才複製一份再改；cascade 和動畫的 `style_change` 改成傳 `Arc`，debug build 照樣做完整的調整比對。對照 Blink 命中時複製分組的指標、Stylo 用 style sharing cache 共用整個 `Arc`；kun 還沒分組，共用整份最接近它們的效果，5c 分組之後照樣用（只有調整的檢查要跟著改寫）。先暫時加計數量各 suite 命中快取的比例和其中調整不改的比例，夠高才做 S2，否則只把快取的存放改成 `Arc`（S2'）。不選不量直接做（S2b）。量的結果（`details.md` 的「5a 前：matched properties cache 的命中」）：TodoMVC、NewsSite 有 70% 到 88% 的 cascade 命中快取，Charts、Stockcharts 只有 7% 到 8%；命中後調整改的多半是 `original_display`（和位置無關，只是快取存的是調整前的值），其次是 flex、grid 的子元素的 blockification 和 `effective_z_index`。因此選 **S2-key**（2026-10-19，使用者）：把調整用到的輸入（是不是 root、layout parent 是不是 flex 或 grid、哪一種表單控制項，和調整讀到的 parent 的值）放進快取的 key，快取存調整完的 `Arc<ComputedStyle>`，命中就共用；debug build 照樣重新 cascade 加調整比對。不選 S2-檢查（存之前先算好和位置無關的調整，命中時再檢查和位置有關的、不改才共用：調整和檢查兩套要對齊，Svelte、NewsSite 只有約三分之二的命中能共用）。做完用同樣的計數再量一次命中率。
48. **第 5a 段：新的 style 和舊的相同時沿用舊的 `Arc`（S1a）**（2026-10-19，使用者）：元素重算完，結果和舊的相同（本來就比較過）就留舊的 `Arc`，和 pseudo-element 一樣；S2-key 命中拿到的共用 `Arc` 和舊的相同時也留舊的。子元素的 matched properties cache 比對 parent 時更常走指標比較，paint 的快取（`Arc::ptr_eq(&a.style, &b.style)`）更常沿用畫過的 item；自訂屬性「和舊的相同就換回舊的那份」只在 style 是自己獨有時做（共用的 `Arc` 改寫會逼出整份複製）。`style` 的 invalidation 測試改成從 `StyleEngine` 拿這次重算了哪些元素（測試用的紀錄），不再靠 `Arc` 換了沒。不選照舊看 `Arc`、改掉受影響的預期（S1b），也不選不做（S1c）。
49. **第 5a 段：cascade 的暫時配置放在 stack 上（S3a）**（2026-10-19，使用者）：每個 longhand 一格的 `winners`（181 格）改成 stack 上的固定陣列，排序用的宣告區塊改成 `SmallVec`；自訂屬性的 `Vec` 和 set 維持（沒用到時不配置）。S2-key 之後 TodoMVC、NewsSite 多半命中快取、走不到這裡，受益的主要是命中率只有 7% 到 8% 的 SVG 為主的 suite。對照 Blink 的 `CascadeMap` 對每個 property 用固定大小的陣列、Stylo 的 cascade 用 `SmallVec`。不選跨元素重複使用的 heap 暫存（S3b：`winners` 存的是指向這個元素的宣告的參考，要改成存索引，`style` 不能用 `unsafe` 繞過 lifetime），也不選不做（S3c）。
50. **第 5a 段：同一個 parent 的子元素共用繼承後的起點（S4）等 5b、5c 之後再評估**（2026-10-19，使用者）：分組之後起點本來就變便宜，到時候量過再決定要不要做。
51. **第 5a 段：測試和量測**（2026-10-19，使用者）：debug build 在 S2-key 共用 `Arc` 時照樣重新 cascade 加調整比對；mutation test；`cargo test -p style -p layout -p paint -p shell`，相關的 reftest 和 WPT（`css/css-cascade/`、`css/css-display/`、`css/css-transitions/`、`css/css-animations/`）；做完用同樣的計數量命中率，用 `--sample` 比較 NewsSite-Next、React-Stockcharts-SVG、TodoMVC-jQuery、TodoMVC-WebComponents 的 `recalc_style` CPU，用 `--trace` 量 style 的時間。
52. **第 5b 段照第 1 條做（T0a）**（2026-10-19，使用者）：先把少用的欄位搬進 2 到 3 個 `Arc` 的 rare 組，再做 5c 的分組；5b 做完先量，5c 帶著數據決定 A1、A2 和讀寫方法。對照 Blink 從 `StyleRareNonInheritedData`、`StyleRareInheritedData` 演進到產生的分組。不選跳過 5b 直接做 5c（T0b：一次改完才知道效果，改動和審查集中在一次）。
53. **第 5b 段：照量到的比例挑欄位（T1a）**（2026-10-19，使用者）：暫時加計數，跑一輪 Speedometer，統計每個欄位在多少比例的元素上不是初始值（繼承的欄位看和 parent 相不相同），量完拿掉；照比例分組，結果和建議先給使用者確認才搬，和 Blink 的分類明顯不同的地方標出來。對照 Blink 照各 property 在網路上的使用率排名、按百分比分進 rare 組。不選不量照大小和用途直接分（T1b），也不選照 Blink 的排名資料（T1c：手邊沒有那份資料）。
54. **第 5b 段：搬出去的欄位直接經過組讀寫（T2a）**（2026-10-19，使用者）：讀 `style.rare.x`，寫 `style.rare_mut().x`（共用時才複製那一組）；只改搬出去的欄位被讀寫的地方。全面改成讀寫方法（決定 7）等 5c 選了 A1 或 A2 再一起決定。不選只為搬出去的欄位加方法（T2b：寫法混在一起），也不選現在就把 105 個欄位都改成方法（T2c：5c 等級的工作量）。
55. **第 5b 段：測試和量測（T3）**（2026-10-19，使用者）：靠編譯器擋住漏改；matched properties cache 的 debug 交叉檢查照樣比整個 style；`Arc` 包的組的 `PartialEq` 比內容；單元測試涵蓋沒設的元素共用同一份組、設了的各自一份、繼承的組和 parent 相同時沿用；mutation test；`cargo test -p computed_style -p css -p style -p layout -p paint -p renderer -p shell`、kun 的 reftest、照搬了哪些欄位跑 WPT 的相關目錄；做完用 `--sample --against`（5a 之後）和 `--trace` 量。第 5 階段的檢查在 5c 做完時跑。
56. **第 5b 段的分組**（2026-10-19，使用者）：照量到的比例（`details.md` 的「5b 前：`ComputedStyle` 的欄位」）分三個 `Arc` 的組：rare 非繼承（`animations`、grid 的 11 個、`border_radius`、outline 的 4 個、counter 三個和 `content`，約 540 bytes，各 suite 不是初始值的比例多在 1% 到 10%）、`svg`（整個 `SvgStyle`，216 bytes，HTML 為主的頁面 6% 到 13%，Complex-DOM 35%，SVG 為主的 45% 到 86%，和 parent 相同時沿用 parent 的那份）、背景的 8 個清單（128 bytes，0% 到 17%）。`transitions`（各 TodoMVC、NewsSite 13% 到 23%）、`box_shadow`（jQuery、Backbone 16%）、`transform`（observable-plot 13%）留在主體：和 Blink 的分類不同（Blink 把它們放在 rare 非繼承的資料裡），因為它們在部分 suite 常設，放進組會讓整組常被複製。複製的大小估計從 1,472 降到約 610 bytes。實作時 outline 的 4 個欄位留在主體：`cascade_start` 讓每個元素的 `outline-width` 從 `medium` 開始（最後 `outline-style: none` 的再歸零），放在組裡每個元素都要複製整組；它們只有 14 bytes。
57. **第 5 階段停在 5b，5c 延到第 6 段重排（U0b）**（2026-10-19，使用者；修改第 1 條的「不在 A3 之後停下」）：5b 之後取樣，複製 `ComputedStyle`（`clone`、`memmove`、`inherit`）只占 style 的一成左右（NewsSite-Next 10.6%、React-Stockcharts-SVG 9.7%、Charts-observable-plot 13.5%，Svelte 幾乎沒有），5c 的效益上限約 9 ms，卻要改 `layout`、`paint`、`style` 約 640 處讀欄位的地方；同時 selector 比對占 style 的 14% 到 19%、cascade 套用宣告 11% 到 19%。第 5 階段跑階段的檢查後結束，第 6 段把 5c 和這些熱點一起列出效益和成本再決定。5b 的三組到時直接成為分組的一部分。不選照第 1 條接著做 5c（U0a）。
58. **第 6a 段：invalidation 的查表不配置（V2）、`Animator::moved` 跟著走訪往下帶（V4），屬性選擇器（V1）不做**（2026-10-19，使用者）：第 6 段開始前取樣（`details.md` 的「第 6 段前：style 的時間花在哪」）並換算成毫秒：V2 是 `AncestorFeatures` 每次查都把名字轉小寫、配置字串（React-Stockcharts-SVG 每個 iteration 約 0.9 ms、Charts-observable-plot 約 0.4 ms），改成名字已經是小寫時直接查；V4 是頁面有過被移除的元素時，每個重算的元素都沿著祖先走到根找自己是不是在被移除的子樹裡（Svelte 約 0.25 ms、NewsSite 約 0.3 ms，取樣裡是 `flat_tree_parent`），改成走訪時往下帶，和 `rendered_above` 一樣。V1 是 `[dir="rtl"] .x` 這類規則沿著祖先找 `dir`（Svelte 一個 iteration 約 39 萬次呼叫，每個 Complex-DOM suite 約 0.4 ms）：要比得更快得把 selector 的名字改成 atom，Blink 對這種規則也一樣往上找，不做，記進「之後的可能」。測試：V2 靠現有的 invalidation 測試，V4 補被移除又插回的子樹的動畫測試；mutation test；相關的 `cargo test` 和 WPT 的 `css/css-transitions/`、`css/css-animations/`；做完用 `--trace` 量。

59. **6a 之後結束這份 RFC（W1）**（2026-10-19，使用者）：style 每個 iteration 151 → 92 ms（Chrome 34 ms），stylesheet 晚到和改動的整份重算拿掉了。剩下的候選（套用宣告、5c 的分組、`d` 的解析、屬性選擇器、S4）記進「之後的可能」，附上數據。不選先細查套用宣告（W2：取樣裡分散、沒有單一的大頭，要細查才知道能拿到多少），也不選接著做 5c（W3：效益上限約 9 ms，要改約 640 處）。

## 待決定

無。

## 之後的可能

數據都是 6a 之前（5b 之後）的取樣，見 `details.md` 的「第 6 段前：style 的時間花在哪」。

- **套用宣告（`Recascade::cascade`）**：style 最大的一塊（NewsSite-Next 37.9%、Charts-observable-plot 31.6%、Stockcharts 和 Svelte 13%），但分散在 `cascade_blocks` 的 closure（4% 到 6%）、`cascade_start`（2% 到 3%）、`apply`（2% 到 4%）、舊 style 的 `Arc` 釋放（2.7%）、自訂屬性的 `resolve`（2.4%）。要先細查每一塊（`atos` 對回 inline 的函式和行號，像第 1 段查 `recalc_style` 自己的時間），才知道能拿到多少。
- **`ComputedStyle` 全部分組（5c）**：照 Blink（A1，產生的 `DataRef` 組）或照 Stylo（A2，約 25 個 `Arc` 的 style struct），要不要先產生讀寫的方法把 `layout`、`paint`、`style` 約 640 處讀欄位的地方集中起來（第 1、7、57 條）。效益上限是複製 `ComputedStyle` 的時間：style 的一成左右（NewsSite-Next 10.6%、Stockcharts 9.7%、Charts-observable-plot 13.5%），每個 iteration 約 9 ms。5b 的三組直接成為分組的一部分。
- **同一個 parent 的子元素共用繼承後的起點（S4）**：分組之後起點本來就變便宜，做完 5c 再量（第 50 條）。
- **SVG 的 `d` 在 attribute 設定時就解析**：現在每個元素第一次算 style 時解析（`parse_d_attribute`，Stockcharts 和 Charts-observable-plot 的 style 約 5% 到 8%，每個 iteration 約 1 ms），共用快取不放 `d`（第 13 條）。
- **屬性選擇器在祖先位置的比對**：`[dir="rtl"] .x` 這類規則沿著祖先找屬性，Complex-DOM 的 TodoMVC 每個 iteration 約 0.4 ms；selector 的名字改成 atom 可以比得更快（第 58 條）。
- **文字的 shaping**：Editor-TipTap 的建 box tree（`ShapeCache` 這類的快取）。
- **建 box tree 的增量**。
- **style sharing**：兄弟元素的 style 相同時直接共用（Blink 以前的 style sharing、Stylo 的 style sharing cache）。
