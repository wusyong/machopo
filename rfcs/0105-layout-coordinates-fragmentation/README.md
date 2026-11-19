# RFC 0105：layout 的座標模型和分段

- 開始日期：2026-11-19
- 狀態：🚧 已排程：2026-11-19
- 進度：`progress.md`
- 細節：`details.md`

## 摘要

machopo 的 layout 現在整條假設 `writing-mode: horizontal-tb`、`direction: ltr`，也沒有分段（一個 box 只有一個 fragment）。這份 RFC 先評估：之後要加 `direction: rtl`、`writing-mode`（直排）、multi-column 和列印時，layout 的座標要用什麼模型、分段要用什麼機制，以及建在 taffy 的 fork 上撐不撐得起。選了 B（「已決定的事項」第 1 條）：照 Blink LayoutNG 的做法，machopo 自己的 layout 用邏輯座標寫、在 fragment 建好時一次換成物理座標，分段用 break token，taffy 長期留著負責 flex、grid 和 block 的演算法（直排經過轉置的那一層），分段等到要做 multi-column 時，再決定在 taffy 的上游做、只在 machopo 自己的接縫上斷，還是把 block layout 換成自己寫的。

## 動機

RFC 0103 的「架構上可能要先做的事」第 3 個：每個只照物理方向寫的 layout 功能，之後都要補邏輯方向和斷開的情形。Blink 重寫成 LayoutNG 的原因之一就是舊的 layout 補不上分段（`details.md` 的「Blink LayoutNG」）。現在 layout 的程式還不算多，越晚定，要回頭改的越多。

### 現況

2026-11-19 讀程式得到的（逐項的位置在 `details.md` 的「machopo 現在的 layout」）：

- **整條都是物理座標**。box tree 存 taffy 的 `Style`、`Layout`、`Cache`；fragment tree（照 Blink 的 `PhysicalBoxFragment`）存物理的大小、上右下左的 margin、border、padding 和相對父 fragment 的位移；paint、hit test、捲動、CSSOM 都讀物理的。`layout` 裡 `.left`、`.right`、`.top`、`.bottom` 約 500 處、`.width`、`.height` 約 470 處，paint 裡各約 190、280 處。
- **style 在 cascade 時就把邏輯屬性換成物理的**（「machopo 只有 `horizontal-tb` 和 `ltr`，block-start 是 top、inline-start 是 left」）；`direction`、`unicode-bidi`、`writing-mode`、`text-orientation`、`column-*`（`column-gap` 以外）、`break-*`、`orphans`、`widows`、`box-decoration-break`、`@page` 都沒有。
- **假設由左到右、由上到下的地方**：行從左邊開始、inline box 的開頭邊界是左邊、float 只有 left 和 right、`relative` 先看 `left`、list marker 放在左邊、`text-overflow` 從右邊截、table 的欄從左排、可捲動的範圍只往右下長、捲動的原點在左上。
- **bidi 只算了一半**：parley 預設用第一個強方向的字決定段落的方向、照 UAX #9 重排一行裡的字，但 machopo 自己的部分（inline box 的邊界、float、對齊、caret）都當成由左到右。
- **沒有分段**：沒有 break token、continuation、fragmentainer；唯一像分段的是 inline box 被斷成好幾行時的 `first`、`last`。
- **taffy 的上游已經有 `direction: rtl`**（block、flex、grid、out-of-flow 都照它鏡射，輸出照舊是物理座標），machopo 從來沒有設它。`writing-mode` 不在 taffy 的演算法裡：上游只加了把一個節點的輸入輸出轉置的工具，讓樹那一側自己處理直排（Blitz 這樣做了）；把它搬進 taffy 的提案還沒有維護者回覆。taffy 沒有任何分段的概念，也沒有人提過。
- **parley**：有自己的 UAX #9、RTL 的行重排和對齊；沒有直排（上游只有停住的草稿 PR）；斷行一次一行、可以退回，夠拿來在 fragmentainer 的邊界停下、之後接著排。

### 目標

1. 定下 layout 的座標模型：演算法用什麼座標寫、什麼時候換成物理座標、paint 以後的部分要不要動。
2. 定下分段的機制：一個 box 怎麼有好幾個 fragment、layout 怎麼在 fragmentainer 的邊界停下再接著排。
3. 定下和 taffy 的關係：哪些演算法繼續用 taffy、哪些自己寫、要不要在上游做。
4. 照定下的方向排出之後的 RFC（`direction: rtl`、`writing-mode`、multi-column、列印）的順序，以及這份 RFC 自己先做的那一小段（如果有）。

### 不在範圍內

- 真的做出 `direction: rtl`、`writing-mode`、multi-column、列印：這份只定模型，各自之後的 RFC 做（第一段可能順便接上 taffy 已經有的 `direction`，見「分階段」）。
- 直排的文字 shaping（parley 沒有）。
- `@page`、列印的 UI。

### 要解決的限制

這份本身不刪 `docs/limitations.md` 的條目；之後的 RFC 照它定的模型做的時候刪（「只有由左到右」那一條、table 的欄從左排、sticky 沒有 `direction`、float 沒有 `inline-start`、`text-overflow` 只有橫排、捲動的原點在左上、`:dir()`）。

## 設計概要

對之後改 layout 的人（照「已決定的事項」定下來之後）：

- 新的 layout 功能用 inline 和 block 的方向寫（`LogicalSize`、`LogicalOffset`、writing mode 跟著 constraint space 走），不寫 left、top。
- 一個 box 排好時，它的子 fragment 的位移先是邏輯的，等它自己的大小定了，一次換成物理的（Blink 的 `PhysicalBoxFragment` 的建構子）；fragment tree、paint、hit test、捲動、CSSOM 照舊是物理的。
- layout 的入口多一個 break token（先一律是沒有），fragment 可以有好幾個；要做 multi-column 時再把分段接上。

對網頁的作者：這份本身沒有看得到的改變（除非第一段接上 `direction: rtl`）。

## 詳細設計

草稿：列選項，細節等決定之後才寫。三家怎麼做、machopo 和 taffy、parley 的現況在 `details.md`。

### 選項

- **A：維持物理座標，需要時在邊界轉換**。taffy 照舊；`direction: rtl` 用 taffy 已經有的；直排照 Blitz 在每個 writing mode 的根轉置輸入輸出；分段之後在 machopo 自己的 block 和 inline 上加。
  - 優點：現在幾乎不用改；RTL 很快能接上。
  - 缺點：machopo 自己寫的部分（inline、table、out-of-flow 的接法、sticky、list marker、overflow）每個都要各自補 RTL 和直排的分支；Gecko 在 reflow 時存物理的位置，vertical-rl 要事後修正（`nsBlockFrame` 的假的 container 大小），Servo 在 fragment 建立時就換、還沒拿到父 frame 的大小，直排的 block 方向至今沒做完。分段沒有地方接，到時候整條入口都要改。
- **B（選了，第 1 條）：machopo 自己的 layout 用邏輯座標，fragment 建好時換成物理的；taffy 留著，經過一層接上**。照 Blink LayoutNG：
  - 邏輯的型別（`LogicalSize`、`LogicalOffset`、`LogicalRect`、writing mode 和 direction 合成一個值、`WritingModeConverter`）放在 machopo；
  - machopo 自己的演算法（inline、table、定位、float 的接法）和 fragment 的 builder 用邏輯座標，builder 存邏輯的子位移，父 fragment 的大小定了才一次換成物理的（Blink 不用事後修正，正是因為這樣）；
  - taffy 的 flex、grid、block 照舊用，RTL 直接設 taffy 的 `direction`，直排在 writing mode 的根轉置（Blitz 的做法，也是上游現在給的答案），之後上游的 #752 進來就換掉這一層；
  - 入口多一個 break token、fragment 可以有好幾個，先一律不分段；真的分段到做 multi-column 時，在第 1 條的三條路裡選。
  - 優點：之後的每個功能只寫一份；轉換只在一個地方；和 Chromium 同一個模型，查 Blink 的程式能直接對照；taffy 留著，不用一次重寫 flex 和 grid。
  - 缺點：要先改現有的 inline、table 和 fragment builder 的寫法（約 500 處物理的欄位，大多在 `layout` 裡）；taffy 那一層要維護轉置；taffy 本身還是物理的，它的 block 不能分段。
- **C：整個換成自己寫的、LayoutNG 那樣的演算法**（block、flex、grid 都自己寫，邏輯座標和 break token 從頭就有）。
  - 優點：最接近 Chromium，分段和直排沒有限制。
  - 缺點：成本最高，等於重寫 layout；taffy 的 flex、grid 現在 WPT 的通過率要重新追。
- **D：在 taffy 的上游做**（#752 的 writing mode 和分段）。
  - 優點：社群一起維護。
  - 缺點：writing mode 的提案還沒有維護者回覆，分段上游沒有人提過；時程不在 machopo 手上，fork 要盡量小（CLAUDE.md）。可以和 B 並行：B 的轉置那一層是上游現在建議的做法，上游有了就換。

比較的細節（每個選項要改的範圍、和三家的對照）在 `details.md` 的「做法的比較」。

### 測試

評估本身沒有程式。定下來之後：邏輯和物理的轉換照 Blink 的 `WritingModeConverter` 的情況寫單元測試（四種 writing mode 乘兩種 direction）；換寫法的那一段要和現在的 reftest、WPT 結果一樣（行為不變）。

### 分階段

做法定了（第 1 條）；每個階段開始前，只決定那個階段要決定的事：

1. **邏輯的型別和轉換**，machopo 自己的演算法改用邏輯座標，現有的行為不變（只有 `horizontal-tb`、`ltr`）。決定是「已決定的事項」第 2 到 4 條。分五段：1a 是型別和 converter；1b 是 inline 的行在 box 裡的位置（第 5 條）；1c 是 table，連 collapsed border（第 6 條）；1d 是 float、list marker、line clamp、置中、slider 的 thumb；1e 是整個階段的檢查和文件。
2. （第 2 階段開始前決定做不做）接上 taffy 已經有的 `direction: rtl` 和 style 的 `direction`、`unicode-bidi`，machopo 自己的部分照邏輯座標鏡射。
3. 入口的 break token 和「一個 box 好幾個 fragment」，先不分段。

直排、multi-column、列印各自另開 RFC。

## 缺點

- 第一段是只改寫法、行為不變的大改動，期間和其他 layout 的工作會衝突。
- 兩種座標並存（layout 裡邏輯的、之後物理的），邊界要守住，不然又會在某處偷偷假設物理方向。
- taffy 照舊是物理的，直排要靠轉置那一層；上游方向改了要跟。

## 理由和其他做法

選 B（第 1 條）。完整的比較在 `details.md`。不做的話，之後每個 layout 功能都只照物理方向寫，等到要做 RTL、直排、multi-column 時，要回頭改的就是全部（Blink 的舊 layout 正是這樣才重寫）。

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

## 待決定

- taffy 現在負責 block（margin collapsing、大小的限制、固有大小）、flex、grid、out-of-flow、float 的排列、replaced element 的大小，和 box 的快取、遞迴的骨架；machopo 自己寫的是 inline 和文字（parley）、table、sticky、list marker、可捲動的範圍、fragment tree。

還沒回答的：

1. 第 2 階段做不做（接上 `direction: rtl`），第 2 階段開始前決定。

## 之後的可能

- `direction: rtl` 和 bidi（style 的 `direction`、`unicode-bidi`、`:dir()`，parley 的 L1 規則和行內的方向覆寫）。
- `writing-mode` 和直排的文字（parley 沒有直排的 shaping，要另外想辦法或等上游）。要接手第 1 階段留下的：inline 行裡面那一層（每一行經過 converter，行裡面的東西照行的 writing mode 加上 `ltr` 換，第 5 條），和 taffy 的 block、flex、grid 在 writing mode 的根轉置輸入輸出（第 1 條）。
- multi-column、列印和 `@page`。
- 在 taffy 的上游做 writing mode（#752），有了就拿掉 machopo 的轉置那一層。
