# RFC 0105：layout 的座標模型和分段

- 開始日期：2026-11-19
- 狀態：⏸️ 延後：2026-11-19（評估的結論：現在的架構不會卡死 `direction: rtl`、`writing-mode`、分段，只是越晚做越貴；要做其中任何一個時再回來，從「待決定」記下的看法開始）
- 細節：`details.md`

## 摘要

machopo 的 layout 現在整條假設 `writing-mode: horizontal-tb`、`direction: ltr`，也沒有分段（一個 box 只有一個 fragment）。這份 RFC 先評估：之後要加 `direction: rtl`、`writing-mode`（直排）、multi-column 和列印時，layout 的座標要用什麼模型、分段要用什麼機制，以及建在 taffy 的 fork 上撐不撐得起。草稿列出選項和建議；照 Blink LayoutNG 的做法，建議 machopo 自己的 layout 用邏輯座標寫、在 fragment 建好時一次換成物理座標，分段用 break token，taffy 留著負責 flex、grid 和 block 的演算法（直排經過轉置的那一層），分段等到要做 multi-column 時再決定 block layout 要不要自己寫。

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
- **B（建議）：machopo 自己的 layout 用邏輯座標，fragment 建好時換成物理的；taffy 留著，經過一層接上**。照 Blink LayoutNG：
  - 邏輯的型別（`LogicalSize`、`LogicalOffset`、`LogicalRect`、writing mode 和 direction 合成一個值、`WritingModeConverter`）放在 machopo；
  - machopo 自己的演算法（inline、table、定位、float 的接法）和 fragment 的 builder 用邏輯座標，builder 存邏輯的子位移，父 fragment 的大小定了才一次換成物理的（Blink 不用事後修正，正是因為這樣）；
  - taffy 的 flex、grid、block 照舊用，RTL 直接設 taffy 的 `direction`，直排在 writing mode 的根轉置（Blitz 的做法，也是上游現在給的答案），之後上游的 #752 進來就換掉這一層；
  - 入口多一個 break token、fragment 可以有好幾個，先一律不分段；真的分段到做 multi-column 時決定（block layout 可能要自己寫，見「待決定」）。
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

等第一條決定之後再分。初步：

1. 邏輯的型別和轉換、fragment builder 改成存邏輯的位移，現有的行為不變（只有 `horizontal-tb`、`ltr`）。
2. （可選）接上 taffy 已經有的 `direction: rtl` 和 style 的 `direction`、`unicode-bidi`，machopo 自己的部分照邏輯座標鏡射。
3. 入口的 break token 和「一個 box 好幾個 fragment」，先不分段。

直排、multi-column、列印各自另開 RFC。

## 缺點

- 第一段是只改寫法、行為不變的大改動，期間和其他 layout 的工作會衝突。
- 兩種座標並存（layout 裡邏輯的、之後物理的），邊界要守住，不然又會在某處偷偷假設物理方向。
- taffy 照舊是物理的，直排要靠轉置那一層；上游方向改了要跟。

## 理由和其他做法

建議 B（第 1 條還沒決定）。完整的比較在 `details.md`。不做的話，之後每個 layout 功能都只照物理方向寫，等到要做 RTL、直排、multi-column 時，要回頭改的就是全部（Blink 的舊 layout 正是這樣才重寫）。

## 先例

- **Blink LayoutNG**（2026-11-19 查過原始碼和文件）：演算法用邏輯座標（`ConstraintSpace` 的大小是 `LogicalSize`，writing mode 跟著它走），builder 存邏輯的子位移，`PhysicalBoxFragment` 的建構子一次換成物理的；fragment tree、paint、hit test 都是物理的。分段用 break token（`BlockBreakToken`）和 constraint space 裡的 fragmentainer 大小，一個 box 可以有好幾個 fragment，multi-column 和列印都是 fragmentation context。舊的 layout 把分段的內容排成一條、在 paint 時切開，變形、flex、grid 的分段都做不到，所以重寫（Chrome 102 到 108）。照做的方向：B。
- **Gecko**：logical 的型別（`WritingMode`、`LogicalRect`）在 reflow 用，frame 存物理的 `mRect`，vertical-rl 要事後修正位置；分段用 continuation（frame 被分成好幾個）。不照做：要修正、而且一棵會被改的 frame tree。
- **Servo**：用 stylo 的 `WritingMode`、自己的邏輯型別，fragment 建立時換成物理的，但用的是不看父 frame 大小的轉換，直排的 block 方向還沒做完（`writing-mode` 在 pref 後面）；沒有 multi-column、沒有分段（2018 年的 #22397 討論過，選兩棵樹的模型，沒有做）。
- **Blitz**（taffy 和 parley 的用戶）：直排在樹那一側轉置、不改 taffy。B 的 taffy 那一層照這樣做。

## 已決定的事項

無。

## 待決定

延後時（2026-11-19）的看法，回來時從這裡開始：

- **不會卡死**：`direction: rtl` taffy 已經有，接上就好；直排有 Blitz 證明過的轉置做法，taffy 的上游也往這個方向走；分段 taffy 沒有，之後把 block 換成自己寫的再加，成本變高但做得到（Blink 也是後來補上的）。會慢慢變貴的是 machopo 自己寫、只照物理方向的部分（inline、table、sticky、list marker、可捲動的範圍），每多一個這類功能，之後要回頭改的就多一份。
- **最正確的目標是 C**（自己寫 LayoutNG 那樣的 layout：分段是每個演算法的一部分、正交的 writing mode 在 constraint space 裡處理、和 Blink 一對一對照、只有一套座標），**照 B 的階段漸進過去**：先做邏輯的型別、constraint space、break token 的入口、builder 存邏輯的位移（這些 C 也要），再照需要一個演算法一個演算法換成自己寫的（分段最需要 block，先換它），taffy 全部換完就拿掉（瀏覽器 UI 的 `crates/ui` 照舊可以用 taffy）。Blink 的分段也是一個演算法一個演算法換上去的（Chrome 102 到 108）。
- taffy 現在負責 block（margin collapsing、大小的限制、固有大小）、flex、grid、out-of-flow、float 的排列、replaced element 的大小，和 box 的快取、遞迴的骨架；machopo 自己寫的是 inline 和文字（parley）、table、sticky、list marker、可捲動的範圍、fragment tree。

還沒回答的：

1. 選哪個做法（A、B、C、D；上面的看法是 C 為目標、經過 B）。
2. 選 B 時：block layout 要不要為了分段自己寫（taffy 沒有分段），還是先在 taffy 的 block 外面包一層、只在 machopo 自己的 inline 和 block 的接縫上停下；第一段要不要順便接上 `direction: rtl`。

## 之後的可能

- `direction: rtl` 和 bidi（style 的 `direction`、`unicode-bidi`、`:dir()`，parley 的 L1 規則和行內的方向覆寫）。
- `writing-mode` 和直排的文字（parley 沒有直排的 shaping，要另外想辦法或等上游）。
- multi-column、列印和 `@page`。
- 在 taffy 的上游做 writing mode（#752），有了就拿掉 machopo 的轉置那一層。
