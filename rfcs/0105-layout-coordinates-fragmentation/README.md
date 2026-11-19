# RFC 0105：layout 的座標模型和分段

- 開始日期：2026-11-19
- 狀態：✅ 已完成：2026-11-19
- 進度：`progress.md`
- 細節：`details.md`

## 摘要

machopo 的 layout 原本整條假設 `writing-mode: horizontal-tb`、`direction: ltr`，也沒有分段。這份 RFC 定下座標模型（第 1 條）：照 Blink LayoutNG，machopo 自己的 layout 用邏輯座標寫、做完時一次換成物理的，taffy 長期留著負責 block、flex、grid。做了兩個階段：第 1 階段把 machopo 自己的演算法（inline、table、float、list marker、line clamp、置中、slider 的 thumb）改成邏輯座標，行為不變；第 2 階段接上 `direction: rtl`：style（`direction`、`unicode-bidi`、`dir`、`<bdi>`、`<bdo>`、`:dir()`、`dir=auto`）、邏輯屬性照元素的方向、layout 照方向、inline 的 bidi、捲動的原點（容器和視窗）、`text-overflow`、sticky、`<select>` 的清單。分段（入口的 break token、一個 box 好幾個 fragment）移到 multi-column 的 RFC，和分段走哪一條路一起決定（第 23 條）。

## 動機

RFC 0103 的「架構上可能要先做的事」第 3 個：每個只照物理方向寫的 layout 功能，之後都要補邏輯方向和斷開的情形。Blink 重寫成 LayoutNG 的原因之一就是舊的 layout 補不上分段（`details.md` 的「Blink LayoutNG」）。現在 layout 的程式還不算多，越晚定，要回頭改的越多。

### 開始時的狀況

2026-11-19 開始時讀程式得到的（逐項的位置在 `details.md` 的「machopo 現在的 layout」）：

- **整條都是物理座標**。box tree 存 taffy 的 `Style`、`Layout`、`Cache`；fragment tree（照 Blink 的 `PhysicalBoxFragment`）存物理的大小、上右下左的 margin、border、padding 和相對父 fragment 的位移；paint、hit test、捲動、CSSOM 都讀物理的。`layout` 裡 `.left`、`.right`、`.top`、`.bottom` 約 500 處、`.width`、`.height` 約 470 處，paint 裡各約 190、280 處。
- **style 在 cascade 時就把邏輯屬性換成物理的**（「machopo 只有 `horizontal-tb` 和 `ltr`，block-start 是 top、inline-start 是 left」）；`direction`、`unicode-bidi`、`writing-mode`、`text-orientation`、`column-*`（`column-gap` 以外）、`break-*`、`orphans`、`widows`、`box-decoration-break`、`@page` 都沒有。
- **假設由左到右、由上到下的地方**：行從左邊開始、inline box 的開頭邊界是左邊、float 只有 left 和 right、`relative` 先看 `left`、list marker 放在左邊、`text-overflow` 從右邊截、table 的欄從左排、可捲動的範圍只往右下長、捲動的原點在左上。
- **bidi 只算了一半**：parley 預設用第一個強方向的字決定段落的方向、照 UAX #9 重排一行裡的字，但 machopo 自己的部分（inline box 的邊界、float、對齊、caret）都當成由左到右。
- **沒有分段**：沒有 break token、continuation、fragmentainer；唯一像分段的是 inline box 被斷成好幾行時的 `first`、`last`。
- **taffy 的上游已經有 `direction: rtl`**（block、flex、grid、out-of-flow 都照它鏡射，輸出照舊是物理座標），machopo 從來沒有設它。`writing-mode` 不在 taffy 的演算法裡：上游只加了把一個節點的輸入輸出轉置的工具，讓樹那一側自己處理直排（Blitz 這樣做了）；把它搬進 taffy 的提案還沒有維護者回覆。taffy 沒有任何分段的概念，也沒有人提過。
- **parley**：有自己的 UAX #9、RTL 的行重排和對齊；沒有直排（上游只有停住的草稿 PR）；斷行一次一行、可以退回，夠拿來在 fragmentainer 的邊界停下、之後接著排。

### 目標

1. 定下 layout 的座標模型（第 1 到 3 條）：machopo 自己的演算法用邏輯座標，做完時換成物理的；fragment tree、paint、hit test、捲動、CSSOM 照舊是物理的。
2. 定下和 taffy 的關係（第 1 條）：taffy 長期留著負責 block、flex、grid，`rtl` 用 taffy 的 `direction`，直排之後經過轉置那一層。
3. 接上 `direction: rtl`（第 7 條）。
4. 分段的機制（入口的 break token、一個 box 好幾個 fragment）：移到 multi-column 的 RFC（第 23 條）。

### 不在範圍內

- `writing-mode`（直排）、multi-column、列印和 `@page`、分段：各自另開 RFC。
- 直排的文字 shaping（parley 沒有）。

### 解決的限制

`docs/limitations.md` 拿掉的：「只有由左到右」（inline box 的開頭邊界一律是左邊）、table 的欄從左排、sticky 沒有 `direction`、`text-overflow` 只有由左到右、捲動的原點在左上（容器和視窗）、`<select>` 的清單一律對齊左邊、`:dir()`、`direction` 和 `unicode-bidi` layout 不看。

## 設計概要

對之後改 layout 的人：

- 新的 layout 功能用 inline 和 block 的方向寫（`layout::logical` 的 `LogicalSize`、`LogicalOffset`、`LogicalRect`、`BoxStrut`，方向從 `ComputedStyle::writing_direction()` 拿），不寫 left、top；一個 box 的子位移先是邏輯的，等它自己的大小定了，經過 `WritingModeConverter` 一次換成物理的。
- `direction` 照 CSS 和 Blink：段落的方向是 block 的；`unicode-bidi` 是插進文字裡的 bidi 控制字元；行內盒子的邊界在 bidi 重排之後照元素的方向擺；可捲動範圍可以往起點延伸，main thread 的 scroll offset 從原點量（`scrollLeft` 可以是負的），compositor 的 position 從範圍的起點量。
- 一個 box 現在只有一個 fragment，入口沒有 break token；新的程式盡量不要再加「一個 box 一個 fragment」的假設（`docs/architecture.md`），multi-column 時再接上分段。

對網頁的作者：`direction: rtl` 和 bidi 照 Chrome（限制在 `docs/limitations.md`）。

## 詳細設計

三家怎麼做、machopo 和 taffy、parley 開始時的狀況在 `details.md`。

### 座標模型

- 方向的型別在 `computed_style`（`WritingMode`、`WritingDirectionMode`），幾何的在 `layout::logical`（第 2 條）。`WritingModeConverter` 照 Blink：給外層的大小，把邏輯的位移、大小、rect、四邊換成物理的，反過來也行。
- machopo 自己的演算法裡面用邏輯座標，做完時換成物理的寫進 taffy 的 `Layout`（第 3 條）：inline 的內容區的起點和第一行的 baseline（第 5 條）、table 的 cell、row、row group、caption 和 collapsed border（第 6 條，邏輯的格子給 paint 時翻轉、轉置成物理的格線）、float 的起點、list marker、line clamp、置中、range 的 thumb。行裡面那一層（parley 給的位置）照舊是物理的，直排時再換（第 5 條）。
- taffy 照舊是物理的：`rtl` 設它的 `direction`（block、flex、grid、out-of-flow 照它鏡射）。

### `direction: rtl`

- style：`direction`（繼承）、`unicode-bidi`（不繼承，在 `ComputedStyle` 本身）；HTML 的 `dir` 照 Blink 是 presentational hint（第 9 條），UA 樣式照 Blink 的 `html.css` 給 bidi 的規則；directionality 快取在元素上，`:dir()` 和 `dir=auto` 照它（第 10 條）。邏輯屬性在 cascade 時照元素自己的方向對應（第 11 條）。
- layout：段落的方向是 block 的（`plaintext` 是第一個強方向字元的，整個當成一段，第 14 條）；flex、grid、table 從右排；靜態位置、outside marker 在 inline-start；視窗照根元素的方向（`Document::PropagateStyleToViewport`）。
- inline 的 bidi：`unicode-bidi` 照 Blink 在文字裡插入 bidi 控制字元（第 12 條），強制換行前後關掉、重開，一段裡沒有字元的元素不插（第 16 條）；行內盒子的邊界照元素的方向給 parley 斷行，parley 重排一行之後照視覺順序重新擺（第 13、15 條，Blink 的 `ComputeInlinePositions`）。
- 捲動：可捲動範圍是從 padding box 量的矩形，往起點延伸的軸照 style（Blink 的 `HasLeftOverflow`、`HasTopOverflow`），起點那一側加 padding、捲不到的裁掉（第 18 條）；main thread 存從原點量的 scroll offset，commit 時加上原點給 compositor（第 17 條）；視窗也一樣（第 19 條）。`scrollIntoView` 的 inline 的 start、end 照捲動容器的方向。
- `text-overflow` 和 line clamp 的省略號在行的結尾（第 20 條）、sticky 的 inset 放不下時讓結尾那一邊（第 21 條）、`<select>` 的清單對齊 select 的起點、項目照自己的方向（第 22 條）。

### 測試

邏輯和物理的換算照 Blink 的 `WritingModeConverter` 寫單元測試（五種 writing mode 乘兩種 direction）；第 1 階段的行為不變由現有的 reftest、WPT 守著；第 2 階段的行為由 `layout`、`style`、`dom`、`renderer`、`compositor`、`paint`、`shell` 的測試，WPT 的 bidi、directionality、`css-logical`、`cssom-view`、`css-overflow`，和情境 `rtl-scroll`、`direction` 守著。驗收表和檢查在 `progress.md`，每一段的 mutation test 的 spec 是這個資料夾的 `mutants-*.toml`。

### 分階段

1. **邏輯的型別和轉換**（2026-11-19 做完）：machopo 自己的演算法改用邏輯座標，行為不變。分五段（第 4 條）：1a 型別和 converter；1b inline 的行在 box 裡的位置；1c table，連 collapsed border；1d float、list marker、line clamp、置中、slider 的 thumb；1e 檢查和文件。
2. **接上 `direction: rtl`**（2026-11-19 做完；第 7 條）：分五段（第 8 條）：2a style；2b 邏輯屬性和 layout 用 `direction`；2c `unicode-bidi` 在 inline 內容裡；2d 捲動的原點、sticky、`text-overflow`、`<select>`；2e 檢查和文件。
3. 入口的 break token 和「一個 box 好幾個 fragment」：移到 multi-column 的 RFC（第 23 條）。

## 缺點

- 兩種座標並存（machopo 的演算法裡是邏輯的、fragment 之後是物理的），邊界要守住，不然又會在某處偷偷假設物理方向。
- 行裡面那一層（parley 的位置）和 taffy 照舊是物理的，直排要另外換和轉置。
- 分段還沒有地方接；之後的 layout 程式可能又假設「一個 box 一個 fragment」。

## 理由和其他做法

選 B（第 1 條）：之後的每個 layout 功能只寫一份，轉換只在一個地方，和 Chromium 同一個模型，taffy 留著不用重寫 flex、grid。比較過的做法（完整的在 `details.md` 的「做法的比較」）：

- **A：維持物理座標，需要時在邊界轉換**：machopo 自己寫的部分（inline、table、sticky、list marker、可捲動的範圍）每個都要各自補 `rtl` 和直排的分支；Gecko 存物理的位置、vertical-rl 要事後修正，Servo 在 fragment 建立時就換、直排的 block 方向至今沒做完；分段沒有地方接。
- **C：整個換成自己寫的、LayoutNG 那樣的演算法**：最接近 Chromium，但等於重寫 layout，taffy 的 flex、grid 的 WPT 通過率要重新追；當成分段的三條路之一保留（第 1 條）。
- **D：在 taffy 的上游做**（writing mode 的 #752 和分段）：時程不在 machopo 手上；B 的轉置那一層是上游現在建議的做法，上游有了就換。

做的時候換掉的做法：行內盒子的邊界先試過改 parley 的 fork、照 bidi level 決定邊界在哪一邊（第 15 條不選的那個）；捲動的位移先記成兩邊都存 position，同一天改成 main thread 存 scroll offset（第 17 條）。

## 先例

- **Blink LayoutNG**（2026-11-19 查過原始碼和文件）：演算法用邏輯座標（`ConstraintSpace` 的大小是 `LogicalSize`，writing mode 跟著它走），builder 存邏輯的子位移，`PhysicalBoxFragment` 的建構子一次換成物理的；fragment tree、paint、hit test 都是物理的。分段用 break token（`BlockBreakToken`）和 constraint space 裡的 fragmentainer 大小，一個 box 可以有好幾個 fragment，multi-column 和列印都是 fragmentation context。舊的 layout 把分段的內容排成一條、在 paint 時切開，變形、flex、grid 的分段都做不到，所以重寫（Chrome 102 到 108）。照做的方向：B。
- **Gecko**：logical 的型別（`WritingMode`、`LogicalRect`）在 reflow 用，frame 存物理的 `mRect`，vertical-rl 要事後修正位置；分段用 continuation（frame 被分成好幾個）。不照做：要修正、而且一棵會被改的 frame tree。
- **Servo**：用 stylo 的 `WritingMode`、自己的邏輯型別，fragment 建立時換成物理的，但用的是不看父 frame 大小的轉換，直排的 block 方向還沒做完（`writing-mode` 在 pref 後面）；沒有 multi-column、沒有分段（2018 年的 #22397 討論過，選兩棵樹的模型，沒有做）。
- **Blitz**（taffy 和 parley 的用戶）：直排在樹那一側轉置、不改 taffy。B 的 taffy 那一層照這樣做。

## 已決定的事項

1. **做法是 B，長期留著 taffy；分段到時候在三條路裡選**（2026-11-19）：machopo 自己的 layout 用邏輯座標，fragment 建好時換成物理的，入口帶 break token；flex、grid、block 的演算法照舊交給 taffy，經過一層接上（RTL 設 taffy 的 `direction`，直排在 writing mode 的根轉置）。第 1 階段的邏輯型別、builder 存邏輯的位移，和第 3 階段的 break token、一個 box 好幾個 fragment，下面三條分段的路都需要。分段到要做 multi-column 或列印時（另一份 RFC）在三條路裡選：
   1. 在 taffy 的上游加分段：最乾淨，taffy 的 block、flex、grid 都能斷開；但上游沒有人提過，要自己提案、自己做，時程不在 machopo 手上。能走的話優先走這條。
   2. 只在 machopo 自己的接縫上分段：inline 的行之間、machopo 控制的 block 子元素之間可以斷，taffy 排的部分當成不能斷的整塊。做得出來，但 flex、grid 或 taffy 的 block 裡面要斷開時不對，multi-column 和列印的品質有上限。
   3. 把 block 換成自己寫的：分段最需要 block（Blink 的分段也是一個演算法一個演算法換上去的，Chrome 102 到 108），flex、grid 照舊用 taffy。

   不選 A：machopo 自己寫的部分（inline、table、sticky、list marker、可捲動的範圍）每個都要各自補 RTL 和直排的分支，分段沒有地方接。不選 C 當長期的目標（自己寫整套 LayoutNG 那樣的 layout，最接近 Chromium、分段和直排沒有限制）：要自己寫、自己維護 block、flex、grid，taffy 已經做好、上游持續在改的部分全部重來；B 的第一段和 C 的一樣，真的需要時第 3 條路就是往 C 走一部分。D 不單獨選：上游的 writing mode 還沒有維護者回覆，可以和 B 並行，上游有了就換掉轉置那一層。延後時（同一天）的評估是現在的架構不會卡死這些功能、只是越晚做越貴：會變貴的正是 machopo 自己寫、只照物理方向的部分，所以現在就做。
2. **邏輯的型別：方向放 `computed_style`，幾何放 `layout`**（2026-11-19，第 1 階段開始前）：`WritingMode`（`horizontal-tb`、`vertical-rl`、`vertical-lr`、`sideways-rl`、`sideways-lr`，CSS Writing Modes 4）和 `WritingDirectionMode`（writing mode 加上現有的 `Direction`）放在 `computed_style`；`LogicalSize`、`LogicalOffset`、`LogicalRect`、`BoxStrut`（邏輯的四邊）和 `WritingModeConverter` 放在 `layout` 自己的模組。值用 `f32`，和現在的 layout 一樣（Blink 用定點的 `LayoutUnit`，不跟）。轉換現在就做完 4 種物理的排法乘 2 種 direction，每種都有單元測試；第 1 階段 style 不解析 `writing-mode` 和 `direction`，`ComputedStyle::writing_direction()` 一律回答橫排、`ltr`，之後支援時只改那裡。理由：方向是 style 算的值，layout 和之後的文字都讀它；幾何只有 layout 用，paint、compositor 照舊只看得到物理的。Chromium 一樣分：`WritingMode`、`WritingDirectionMode` 在 `platform/text/`，`LogicalSize`、`WritingModeConverter` 在 `core/layout/geometry/`。不選全部放 `gfx`：每個 crate 都看得到邏輯的型別；不選全部放 `layout`：`computed_style` 不能依賴 `layout`，style 解析 `writing-mode` 時又要搬。
3. **machopo 自己的演算法裡面用邏輯座標，做完時換成物理的寫進 taffy 的 `Layout`**（2026-11-19，第 1 階段開始前）：inline（行和 atomic inline）、table、float、list marker、line clamp、置中、slider 的 thumb，各自在知道自己的 box 大小時用 `WritingModeConverter` 換一次，像每個演算法有一個自己的 builder；`Layout`、`fragment()` 和 fragment tree 照舊是物理的，taffy 那一側不動。理由：中間共用的 `Layout` 是 taffy 的、物理的，taffy 放的和 machopo 放的子元素同一種座標，layout 到 fragment 之間讀 `Layout.location` 的程式（baseline、line clamp、table 的 row 位置）只看到一種。Chromium 只在建 `PhysicalBoxFragment` 時換一次（`BoxFragmentBuilder` 存邏輯的位移），machopo 換的地方多幾個，但用同一個 converter、同一種寫法。不選另外存邏輯的位移、在 `fragment()` 統一換：`fragment()` 要分辨每個位置是哪一種，中間讀 `Layout.location` 的程式同時看到兩種；不選連 taffy 的輸出都換成邏輯的存：從物理換成邏輯也要父 box 的大小，taffy 寫子元素時它還沒定，每個 box 要換兩次。
4. **第 1 階段分五段；行為不變靠現有的測試和 converter 的單元測試，`rtl` 和直排的路記進 `docs/test-gaps.md`**（2026-11-19，第 1 階段開始前）：1a 是型別和 converter（`computed_style` 的 `WritingMode`、`WritingDirectionMode`、`writing_direction()`，`layout` 的邏輯幾何型別和 `WritingModeConverter`，8 種組合的單元測試，還沒有程式用）；1b 是 inline 的行在 box 裡的位置（第 5 條）；1c 是 table；1d 是 float、list marker、line clamp、置中、slider 的 thumb；1e 是整個階段的檢查和文件。只有橫排、`ltr` 時 converter 原樣複製，inline 和 block 寫反了現有的測試抓得到，`rtl` 該翻的地方沒翻抓不到：演算法有沒有在對的地方呼叫 converter，第 2 階段接上 `direction: rtl` 時才有真的測試，期間靠審查，mutation test 把翻轉的錯誤記成等價。不選現在就讓測試對單一 box 設 `rtl` 或 `vertical-lr`、手算預期的位置：沒有能跑 `rtl` 的 machopo 對照，算錯了反而鎖住錯的行為；float 的 `left` 是物理的，規則多；第 2 階段之後這些大多換成 WPT 的。
5. **inline 分兩層換，1b 只做行在 box 裡的那一層**（2026-11-19，1b 開始前）：內容區的起點用 `BoxStrut` 的 inline-start、block-start，經過 box 的 converter 換成物理的（`content_box_origin`）；行從內容區的 block-start 往下排（parley 的 `block_min_coord`），行裡面的東西（run、inline box、decoration、ellipsis、atomic inline）照 parley 的位置，從內容區的 line-left 量。橫排時這和每一行各自經過 converter、行裡面的東西相對於那一行再加回去的位置一樣，但逐行換要先加再減 `block_min_coord`，浮點數多一次捨入，行為就不是完全不變了（2026-11-19 做 1b 時改的）；直排時再換成逐行經過 converter。行裡面那一層的轉軸（直排時對調）留給直排的文字：parley 不會直排，那一層現在用不到、也測不到。照 Blink 的 `FragmentItemsBuilder::ConvertToPhysical`：行用 box 的 writing direction 換，行裡面的東西用行的 writing mode 加上固定的 `ltr` 換，因為 bidi 之後它們的位置是視覺的、由左到右量；所以 `rtl` 時行裡面不翻，parley 給的位置已經是這樣。不選兩層都做：直排時位置換對了、字還是 parley 排的橫排，那一層全部測不到，`LineFragment` 的每個欄位都要改；不選 1b 跳過 inline：內容區的起點、atomic inline 的 margin、第一行的 baseline 會一直照物理方向寫。
6. **1c：table 全部換，連 collapsed border；直排才需要的新東西留給 `writing-mode`**（2026-11-19，1c 開始前）：caption、row group、row、cell 的位置，out-of-flow box 的靜態位置，`vertical-align` 的位移（`shift_content`），欄的背景，和 collapsed border 都用邏輯座標。row 放 cell、row group 放 row 時外層的大小已經知道，馬上換成物理的；table 放 caption 和 row group 時 table 的大小還沒定，先記下邏輯的位置，table 的大小定了再一起換（Blink 的 builder 存邏輯的位移也是為了這個）。collapsed border 照 Blink 的 `TableBorders`：格子的邊是 inline-start 和 block-start 的，每條邊取它的來源的 style 的哪一個物理的邊照 table 的 writing direction 決定；給 paint 的照舊是物理的格線，layout 把邏輯的格子照 writing direction 翻轉、轉置成物理的，paint 不用改。大小的算法不改名：width 就是 inline size、height 就是 block size，模組的開頭寫明。第 1 階段做完時，machopo 自己的 layout 裡沒有只照物理方向寫的現有程式；直排才需要的新東西不在這一階段：inline 行裡面那一層（第 5 條）和 taffy 的輸入輸出的轉置，parley 不會直排、style 也還不解析 `writing-mode`，寫了沒有輸入走得到，留給 `writing-mode` 的 RFC（「之後的可能」）。不選只換放置、collapsed border 留到 `rtl`：它正是只照物理方向寫的現有程式，越晚改越貴；不選連直排的新東西一起做：測不到，真的做直排時多半要照當時的需要重寫。
7. **做第 2 階段：接上 `direction: rtl`**（2026-11-19，第 1 階段做完時）：style 解析 `direction`（和 `unicode-bidi`，做到哪裡在那一段開始前決定），HTML 的 `dir` 對應到它，`:dir()`；parley 拿到段落的方向，taffy 設 `direction`；第 1 階段照 `ltr` 留下的地方補上。理由：阿拉伯文、希伯來文的頁面要它，第 1 階段只靠審查的 `rtl` 那一半有了真的測試，剛寫好的程式回頭改最便宜。不選先做第 3 階段（break token 的入口）：現在沒有功能用到它；不選做到第 1 階段就收尾：第 2、3 階段的設計在這份裡，拆出去要搬。
8. **第 2 階段分五段**（2026-11-19，第 2 階段開始前）：2a 是 style（`direction`、`unicode-bidi`，HTML 的 `dir` 照 Blink 的 presentational hint 對應，`<bdi>`、`<bdo>`，`:dir()`；`writing_direction()` 這一段照舊回答 `ltr`）；2b 是 layout 用 `direction`（`writing_direction()` 讀 style，parley 的段落方向，taffy 的 `direction`，`text-align: start`、`end`，第 1 階段照 `ltr` 留下的靜態位置的對齊和 marker 的 inline-start）；2c 是 `unicode-bidi` 在 inline 內容裡的作用（要 parley 支援，做到哪裡在那一段開始前決定）；2d 是其他用到方向的地方（捲動的原點、sticky、`text-overflow`、`<select>` 的下拉清單）；2e 是整個階段的檢查和文件。理由：混排的文字和 `rtl` 的捲動容器是 `rtl` 的頁面最常碰到的，只做段落的方向排出來的頁面還是錯的。不選只做 style、layout 和收尾：`<bdi>` 這類 embedding 和捲動的起點都不對；不選把 parley 那段放到最後：`text-overflow` 碰到 bidi 的情況要回頭補。
9. **HTML 的 `dir` 照 Blink 用 presentational hint**（2026-11-19，2a 開始前；同一天做 2a 時照 Blink 現在的程式改正了分工）：hint 給 `dir=ltr`、`rtl` 的 `direction`，不合法的 `dir` 在 `<body>` 給 `ltr`；`dir=auto` 的 `unicode-bidi`（`isolate`，search、tel、url、email 的 `<input>` 給 `plaintext`；`<bdo>`、`<textarea>`、`<pre>` 的由 UA 樣式給），和照元素的 directionality 給的 `direction`（Blink 的 `StyleResolver::MatchPresentationalHints` 加的 `LeftToRightDeclaration`、`RightToLeftDeclaration`）。`unicode-bidi: isolate` 由 UA 樣式給：一串 block 元素、`<bdi>`、`<output>` 和 `[dir=ltr i]`、`[dir=rtl i]`、`[dir=auto i]`；`<bdo>` 是 `isolate-override`，`dir=auto` 的 `<textarea>`、`<pre>` 是 `plaintext`，`input[type=tel i]` 是 `direction: ltr`（Blink 的 `html.css`）。照 Chromium 的 `HTMLElement::CollectStyleForPresentationAttribute`。理由：和作者的 CSS、使用者樣式、UA 樣式的層級和 Chrome 一樣；只在元素自己的 attribute 變了時重算；`hints.rs` 已經有這個機制。不選 HTML 規範的 UA 樣式（`[dir]:dir(rtl) { direction: rtl }` 這些）：層級比 presentational hint 低，和 Chrome 不同，而且 `:dir()` 的比對進到每個頁面的 style。
10. **directionality 照 Blink 快取在元素上**（2026-11-19，2a 開始前）：元素存著算好的 directionality，和「自己或祖先有 `dir=auto`」的旗標；`dir` 改了、`dir=auto` 的元素裡的文字改了、子節點進出時，照 HTML 的「directionality 可能改變」更新，方向真的變了的元素才讓它和後代的 `:dir()`、`dir=auto` 的 `direction` 重算；沒有旗標的子樹改文字直接跳過。`unicode-bidi` 照 Chrome 收 6 個值（`normal`、`embed`、`isolate`、`bidi-override`、`isolate-override`、`plaintext`），2a 只存下來，作用在 2c。照 Chromium 的 `Element::CachedDirectionality`、`kSelfOrAncestorHasDirAutoAttribute`、`Element::UpdateDirectionalityAndDescendant`。理由：比對是 O(1)，成本只落在真的有 `dir=auto` 的子樹。不選不快取、比對時現算：每次比對往上走、每次重算 `dir=auto` 都重掃文字，而且改文字這條熱的路上每次都要往上找。
11. **邏輯屬性照 `direction` 對應，放進 2b，2b 分兩部分**（2026-11-19，2b 開始前）：第一部分是 style：cascade 照元素的 writing direction 把邏輯屬性對應到物理的（Blink 的 `CSSDirectionAwareResolver`，`direction` 先決定），`getComputedStyle` 和動畫裡的邏輯屬性也照它，UA 樣式裡照 `ltr` 寫成物理的改回邏輯的；第二部分是 layout（第 8 條給 2b 的那些）。理由：只有 layout 照方向、邏輯屬性照 `ltr` 的話，`rtl` 的頁面文字從右排、清單的縮排還在左邊。不選另外排一段：2b 做完時 `rtl` 的頁面比現在更不一致。
12. **`unicode-bidi` 照 Blink 在文字裡插入 bidi 控制字元**（2026-11-19，2c 開始前）：建 inline 內容時，在每個 inline 元素的開頭、結尾照它的 `unicode-bidi` 和 `direction` 插入 LRI、RLI、FSI、PDI、LRE、RLE、LRO、RLO、PDF，block 自己的套在整段上，交給 parley 的 bidi 演算法（它照 UBA 的 X1–X10 處理這些字元）。插入的字元沒有 DOM 的來源，文字位置的換算（選取、caret、hit test）跳過它們，像生成內容。照 Chromium 的 `InlineItemsBuilder::EnterBidiContext`。理由：parley 不用改，和 Chromium 一樣。不選在 parley 加指定一段文字的 embedding 的 API：要改 parley 照字元做的 bidi 演算法本身，照 fork 的規則要寫成能送上游的樣子，上游收不收不一定。
13. **行內元素的盒子在 `rtl` 的開頭、結尾邊界放進 2c**（2026-11-19，2c 開始前）：開頭、結尾的邊界項目的寬度照元素自己的方向取 inline-start、inline-end 的 margin、border、padding，畫盒子時開頭在右邊的照右邊算（Blink 的 `InlineBoxState`）。理由：2c 改 inline 的 bidi，`rtl` 文字裡有 padding、border 的行內元素一起改才放對邊。不選留到之後：2c 做完時它們還放錯邊。
14. **`unicode-bidi: plaintext` 的 block 整個當成一段**（2026-11-19，2c 開始前）：段落方向設成 parley 的 `Auto`（整段的第一個強方向字元）；只有一段時和 Chrome 一樣，以強制換行分開的好幾段由第一段決定全部，記進 `docs/limitations.md`。行內元素的 `plaintext` 是 FSI…PDI，沒有這個問題。不選改 parley、照 UBA 的 P1 在換行處分段（每段各自的方向和對齊）：要改 parley 的 bidi 和對齊兩處，2c 的範圍擴大；需要時另外做成 fork 的一個 commit。
15. **行內盒子的邊界在 bidi 重排之後照視覺順序擺**（2026-11-19，2c 進行中）：邊界的寬度照元素自己的方向交給 parley 斷行（第 13 條，一行的總寬度不變），parley 排好一行之後，照它的視覺順序重新算每個 glyph run 和 atomic inline 的 x：每個盒子在這一行最左邊的項目前面加它左邊的邊界、最右邊的項目後面加右邊的；哪一邊在這一行由元素的方向和開頭、結尾在不在這一行決定（`ltr` 的開頭在左邊）。照 Chromium 的 `InlineLayoutStateStack::ComputeInlinePositions` 和 `BoxData` 的 `has_line_left_edge`。理由：parley 放邊界項目的位置跟著 bidi level 走（開頭的邊界拿前面那段的 level，在 embedding 外面），`embed`、`isolate` 的盒子兩個邊界會排到同一邊；照視覺順序重擺就和邊界拿到什麼 level 無關，巢狀的 embedding 和內容方向和元素相反、跨行的盒子也對，之後把被 bidi 拆開的盒子做成幾個 fragment 也是在這上面加。不選改 parley 的 fork、讓開頭的邊界拿後面那段的 level、照 level 的奇偶決定邊界在哪一邊（試做過，bidi 的 WPT 結果和這個一樣）：巢狀而且方向混合的 embedding 拿錯 level；內容和元素方向相反的盒子跨行時，邊界畫在相反的一邊（CSS 2 § 8.6 照元素的方向）；還要多一個 fork 的 commit。
16. **強制換行前後關掉、重開 embedding；一段裡沒有內容的元素不插控制字元**（2026-11-19，2c 進行中）：強制換行（`<br>`、保留的換行）前照由內到外關掉打開的 embedding（block 的 override 也算），之後照由外到內重開，照 css-writing-modes 3 的「bidi embedding breaks」和 Chromium 的 `InlineItemsBuilder::AppendForcedBreak`；一個元素在兩個強制換行之間的那一段沒有字元（文字、atomic inline）時，那一段不插它的控制字元。理由：沒有前者，`<br>` 之後的內容掉出元素的 embedding；後者是因為 parley 給有文字 run 的行 strut 的高度，只有控制字元的行（只放 out-of-flow 盒子的 `<span dir=rtl>`、`<br>` 結尾的 `<span dir=rtl>` 之後）多出一行的高度，空的 embedding 在 bidi 裡不影響任何字元，所以結果一樣。不選照 Blink 照插、改 parley 的 fork 讓只有 bidi 控制字元的 run 不算內容（不給行高度、最後的強制換行之後不因為它們多一行，Blink 的 `LineInfo::IsEmptyLine`）：和 Blink 的模型一致，但要多一個 fork 的 commit。
17. **捲動的原點照 Blink 和位移分開**（2026-11-19，2d 開始前；同一天改成 main thread 存 scroll offset）：layout 記下每個捲動容器的可捲動範圍和它的原點（範圍往左、往上超出 padding box 的那一段）。main thread 存的是 scroll offset（從原點量，可以是負的，`scrollLeft` 讀的就是它；沒有紀錄是 0，`rtl` 的在最右邊；原點改變時 offset 不動），夾在 `-原點..=max-原點`；compositor 存的是 position（從範圍的左上角量的 `0..=max`），commit 時加上原點換過去（compositor 交回的只有 delta）；paint 的 composited 容器沒捲動時的平移是原點。照 Chromium：Blink 的 `PaintLayerScrollableArea` 存 `ScrollOffset`，cc 用 scroll position（`ScrollOrigin` 加 offset）。理由：用位移移動內容的地方（CSSOM 的幾何、sticky、scroll anchoring、intersection、paint 的平移、`scrollIntoView`）都不用改，換算只在 commit 一個地方，compositor 的夾住、delta 和同步不用改，捲動的熱路徑上不多做事。不選兩邊都存 position（最初記下的做法）：那些移動內容的地方都要加上原點，沒有紀錄時要當成原點，原點改變時位置要跟著移；不選兩邊都存 scroll offset：compositor 的夾住和同步要改成夾在 `-原點..=max-原點`，和 cc 的模型不同。
18. **可捲動範圍由 machopo 自己在 fragment 上算，往左、往上也算；2d 分兩部分**（2026-11-19，2d 開始前）：`scrollable_overflow` 從大小改成從 padding box 的原點量的矩形，哪一軸的原點在另一頭照 style 判斷（Blink 的 `LayoutBox::HasLeftOverflow`、`HasTopOverflow`：橫排的 `rtl`；flex 另外看 `row-reverse`、`column-reverse`、`wrap-reverse`，`LayoutFlexibleBox` 的），反過來的軸把起點那一側的 padding 算進去，捲不到的那一側裁掉。理由：一份計算，行內內容的行和 table 的格子都在裡面（它們對 taffy 是 leaf），和 Blink 的 `ScrollableOverflowCalculator` 一樣在 fragment 上算。不選改用 taffy 的 `ScrollableOverflow`：machopo 的 leaf（inline 內容、table、表單控制項）要另外算出裡面的溢出交給 taffy，margin collapsing 那幾個照 Blink 補的細節要搬進 taffy。2d 的第一部分是捲動的原點（第 17、18 條），sticky、`text-overflow`、`<select>` 的下拉清單是第二部分，那一部分開始前再決定。
19. **視窗的捲動原點也照根元素的方向，放進 2d 的第一部分**（2026-11-19，捲動容器做好之後）：文件的範圍可以往左（往上）延伸，視窗的原點照根元素的 `direction`（Blink 的 `LayoutView` 的 scroll origin，`scrollX` 是負的），和捲動容器同一個模型：main thread 存從原點量的 offset，commit 時加上原點。理由：`rtl` 的文件往左溢出的內容原本被切掉、捲不到，`rtl` 的網站有寬的內容時就會碰到。不選先記成限制：要多一個階段回頭改同一批程式。
20. **`text-overflow` 和 line clamp 的省略號照段落的方向放在行的結尾那一邊**（2026-11-19，2d 第二部分開始前）：結尾在哪一邊照 parley 的段落方向（block 的 `direction`，`plaintext` 是第一個強方向字元的），從那一邊照視覺順序截，省略號放在那一邊；line clamp 的省略號和為它移開行的那段也是。照 Chromium 的 `LineTruncator::TruncateLine`（照 `LineInfo::BaseDirection`）。理由：`rtl` 的行從左邊溢出，原本只截右邊的、省略號放在右邊；和 Chromium、css-overflow（省略號在行的結尾邊）一樣，一個函式照方向處理兩邊。不選照文字順序截掉邏輯上最後的文字：和 Chromium、規範不同，bidi 排到行中間的情況更複雜。
21. **sticky 的左右 inset 都放不下時，照 containing block 的方向讓結尾那一邊**（2026-11-19，2d 第二部分）：`ltr` 的 containing block 讓 `right`，`rtl` 的讓 `left`；上下照舊讓 `bottom`（只有橫排）。照 Chromium 的 `LayoutBoxModelObject::UpdateStickyPositionConstraints` 和 css-position 3（起點那一邊優先，照 containing block 的書寫方向）。不選照 sticky 元素自己的方向：和 Chromium、規範都不同。
22. **`<select>` 的下拉清單照 select 的方向對齊，每個項目照它自己的方向**（2026-11-19，2d 第二部分）：清單照 select 的 `direction` 對齊它的右邊（`right_aligned`），protocol 的 `MenuItem` 帶每個 option（或 optgroup）的 computed `direction`，shell 照它把文字和縮排放在起點那一邊、段落方向也照它。照 Chromium 的 `ExternalPopupMenu`（`right_aligned` 和 `MenuItem` 的 `text_direction`）。不選所有項目照 select 的方向：option 自己的 `dir` 和 select 不同時和 Chromium 不同。

23. **第 3 階段（入口的 break token、一個 box 好幾個 fragment）移到 multi-column 的 RFC，這份在第 2 階段結束**（2026-11-19，第 3 階段開始前）：break token 和好幾個 fragment，和分段走第 1 條的哪一條路一起決定、一起做。理由：沒有東西會產生第二個 fragment，現在搭的骨架只走得到「一個」的路，測試證明不了它對，mutation test 大多會是等價的；token 的形狀取決於還沒選的路（在 taffy 的上游做，token 進 taffy；只在 machopo 的接縫斷，只在 inline、table 那一層；自己寫 block，入口整個換掉），Blink 先搭骨架是因為它自己寫 block，入口在自己手上。補救：`docs/architecture.md` 提醒新的程式不要再加「一個 box 一個 fragment」的假設。不選照原計畫現在做（骨架測不到、形狀可能要重做）；不選只做讀 fragment 的那一側（CSSOM、paint、hit test 處理一個節點好幾個 fragment：一樣沒有第二個 fragment 可以測）。

## 待決定

無。

## 之後的可能

- `writing-mode` 和直排的文字（parley 沒有直排的 shaping，要另外想辦法或等上游）：行裡面那一層（每一行經過 converter，行的位置和 atomic inline、float 在行間的位置）、taffy 的轉置那一層，和第 1 階段留給直排的新東西。
- multi-column、列印和 `@page`，連同分段：入口的 break token、一個 box 好幾個 fragment，在第 1 條的三條路裡選（第 23 條）。
- 在 taffy 的上游做 writing mode（#752），有了就拿掉 machopo 的轉置那一層。
- bidi 還沒做到的（`docs/limitations.md`）：被 bidi 拆成不相連的幾段的行內盒子做成好幾個 fragment；`plaintext` 每一段各自的方向；`text-align: match-parent`。
