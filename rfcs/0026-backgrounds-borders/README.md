# RFC 0026：背景和邊框

- 開始日期：2026-10-09
- 狀態：✅ 已完成：2026-10-10（RFC 0012 路線圖的 S2，CSS 線的下一項；第 1 到 3 條 2026-10-09 接受，四段各自開始前確認第 4 到 8 條（2026-10-09）、第 9 條、第 10 到 12 條、第 13、14 條（2026-10-10）；四段都做完，驗收在 `progress.md`，限制在 `docs/limitations.md`）
- 細節：`details.md`
- 進度：`progress.md`

## 摘要

把盒子的裝飾畫完整：`background-image`（`url()` 的圖片和漸層）、`background-position` / `-size` / `-repeat` / `-clip` / `-origin` 和多層背景，`border-radius`（連帶圓角的 clip 和 hit test），`box-shadow`、`outline`，以及 `solid` 和 `inset` 以外的 border style。幾乎都在 `style` 和 `paint`，加上 `raster` 新的 shader 和模糊，以及 renderer 這一側載入 `background-image` 的圖片。

## 動機

**現況**（`docs/limitations.md`）：盒子的背景只有 `background-color`，邊框只有 `solid` 和 `inset`（`inset` 畫成 `solid`），沒有 `border-radius`、`box-shadow`、`outline`，也沒有任何背景圖和漸層；`<img>` 以外沒有東西會去載圖片。`getComputedStyle` 的 `background` shorthand 因此讀回空字串（少了 longhand）。

**問題**：這是現在和 Chrome 畫面差距最大的一塊。
- Speedometer 3.1 的 default suite 裡，背景的 longhand（`background-image`、`-size`、`-repeat`、`-clip`、`-origin`、`-position-x` / `-y`、`-attachment`）各有 18 個 suite 用到——比 `position` 的 17 個還多；`border-radius` 的四個角各 15 個，`box-shadow` 14 個（數字和盤點見 `../0012-speedometer/details.md`）。
- 一般網站的按鈕、卡片、頭像、分隔線幾乎都靠圓角、陰影和漸層；沒有它們時版面「位置對了但看起來不對」，`reftest` 也沒辦法再往前守住畫面。
- 圓角還牽動 clip：`overflow: hidden` 加 `border-radius` 的卡片在 kun 裡會漏出直角，hit test 也會在角落誤判。

目標：
- `background-color` 以外的背景：`background-image`（`url()`、`linear-gradient()`、`radial-gradient()`，含 `repeating-`）、`background-position`、`-size`（含 `cover` / `contain`）、`-repeat`、`-clip`、`-origin`，以及多層背景的疊法和 `background` shorthand。
- `border-radius`（四個角、兩個半徑、百分比、重疊時依規範縮放），背景、邊框、`overflow` 的 clip 和 hit test 都照圓角。
- `box-shadow`（外陰影和 `inset`、多重陰影、模糊和 spread）、`outline`（`outline-offset`，畫在版面之外）。
- border style：`dashed`、`dotted`、`double`，以及 `inset` / `outset` / `groove` / `ridge` 照規範的深淺（現在 `inset` 畫成 `solid`）。
- `background-image` 的圖片由 renderer 載入、和 `<img>` 共用一份快取和解碼。
- WPT 的 `css/css-backgrounds`、`css/CSS2/backgrounds`、`css/css-images` 的 reftest 子集。

不在範圍內：
- `border-image`（盤點的 16 個 suite 是 `border` shorthand 展開出來的重設值，不是真的用到）。
- `conic-gradient()`、`image-set()`、`cross-fade()`、`element()`、SVG 當背景圖（SVG 本身是雜項線的 O）。
- `filter`、`backdrop-filter`、`mix-blend-mode`、`background-blend-mode`、`mask`、`clip-path`。
- `background-attachment: fixed` / `local`（H1，第 5 條）。
- `text-shadow`（文字那條線，和 `box-shadow` 共用模糊的底層）。

## 設計概要

給網頁的作者：卡片有圓角和陰影、按鈕有漸層、頭像用 `background-image` 加 `background-size: cover`，圓角的 `overflow: hidden` 容器會把裡面的東西切圓。

給之後改 `style`、`paint` 和 `raster` 的人：
- `ComputedStyle` 多一份**背景層的清單**（Blink 的 `FillLayer`）和圓角、陰影、outline 的資料；背景圖的值是「還沒載入的 `url()`」或「算得出來的漸層」。
- renderer 收集 computed style 裡的 `url()` 去 fetch，拿到的 `ImageData` 和 `<img>` 走同一條路（同一份快取、raster 時才解碼）。
- `paint` 的 `BoxDecorationBackground` 從「一個 `DrawRect`」變成一串：陰影、每一層背景（各自的 clip、重複和縮放）、邊框、outline；圓角讓 clip 從矩形變成 rounded rect，`PropertyTrees` 的 clip node 和 `PaintOp` 都要能帶圓角。
- `raster` 多兩件事：Skia 的 shader（漸層和重複的圖片）和 blur mask filter（陰影）。`PaintOp` 照 cc 的做法帶著畫法的描述，raster 時才變成 Skia 的物件。

## 詳細設計

- **style**：
  - 背景的 8 個 longhand（`background-image`、`-position-x`、`-position-y`、`-size`、`-repeat`、`-attachment`、`-clip`、`-origin`）各自解析、cascade，`ComputedStyle` 照宣告的樣子各存一份清單；`ComputedStyle::background_layers` 才照 `background-image` 的層數把其他清單循環補齊（css-backgrounds 3 § 2.1，Blink 的 `FillLayer::FillUnsetProperties`），`background-color` 畫在最底下。原本打算 cascade 時就合成一份 `Vec<BackgroundLayer>`，但每個 longhand 要能單獨 cascade、單獨 `copy`（`inherit`），而且 `getComputedStyle` 照宣告的長度讀回清單（WPT 的 `background-size-computed.html` 等的「multiple base values」：兩張圖、一個 `background-size` 讀回一個值），所以改成各存各的、用到時才合。`background-position` 是 `-x` / `-y` 的 shorthand（和 Chrome 一樣）。
  - `background` shorthand 照 CSS 的 layer 文法拆開（一個 box 同時是 origin 和 clip、顏色只在最後一層）；序列化照 Blink 的 `GetLayeredShorthandValue`：等於初始值的就不寫（有寫 `padding-box border-box` 也讀回 `none`），寫了 size 就連 position 一起寫；`getComputedStyle` 的 `background` 照 `ComputedStyleUtils::ValuesForBackgroundShorthand` 每一層每個值都寫、顏色在最後一層的最前面。
  - `<position>` 的 computed value：關鍵字和從另一邊量的百分比換成從左上量的百分比，從右、下量的長度留著 `from_end`（Chrome 寫成 `calc(100% - 10px)`；kun 沒有 `calc()`）。
  - 漸層是解析好的資料（角度或邊角、色停點、hint、`repeating`），不是字串：`paint` 要能算出每個色停點在盒子上的位置。`url()` 存寫的字串，cascade 時才對宣告所在的 base URL 解析（第 6 條：每個宣告區塊帶著它的 base URL，`ComputeContext` 多一個 `base_url`）。
  - `border-radius`：四個角的 longhand 各一到兩個非負的長度或百分比（水平、垂直；一個就兩個一樣），`ComputedStyle::border_radius` 是 `Corners<CornerRadius>`；shorthand 是一到四個水平值、可以再加 `/` 和一到四個垂直值，各自照 margin 的四邊補齊。序列化照 Blink：longhand 兩個相同時寫一個，shorthand 先寫水平的四邊（照 margin 省略），垂直的和水平的不同才寫 `/ …`。規範的「半徑總和超過邊長時一起縮放」留在 `paint`（要 used 的盒子大小）：`gfx::RRectF::new` 照 css-backgrounds 的「corner overlap」用同一個比例 f 縮放四個角（Skia 的 `SkRRect` 也是）。只改圓角的 style 只重畫、不重排。
  - `box-shadow`：`none` 或逗號分隔的陰影，每個是 `inset`、兩到四個長度（blur 不能是負的）和顏色，順序不拘（`shadow.rs`）；`element.style` 照 Blink 的 `CSSShadowValue` 寫顏色、長度、`inset`，沒寫的不寫，`getComputedStyle` 每個都寫四個長度、顏色解析過。`outline-color`、`-style`（`auto` 和邊框的樣式，不接受 `hidden`）、`-width`、`-offset` 和 `outline` shorthand，shorthand 照 Blink 的 longhand 順序寫顏色、樣式、寬度。`outline-width` 在樣式是 `none` 時不歸零（css-ui 4 和 Chrome；WPT 的 `outline-009` 找到的）。顏色多一個 `-webkit-focus-ring-color`（`rgb(16, 16, 16)`，Chrome 在 Windows 和 Linux 上的值）。cascade 從 `medium` 開始算邊框和 outline 的寬度：原本從 0 開始，`border-style: solid` 沒寫寬度時是 0px，Chrome 是 3px（這一段找到的）。只改陰影和 outline 只重畫。
  - 邊框的樣式（第 13 條）：`none`、`hidden`、`solid`、`dotted`、`dashed`、`double`、`groove`、`ridge`、`inset`、`outset`；`hidden` 和 `none` 一樣把寬度歸零（kun 沒有表格，`hidden` 在 border conflict 裡的意思用不到）。outline 的樣式是這些（不含 `hidden`）和 `auto`。
- **背景圖的載入**：`FrameView::start_loads` 先把 style 算到最新（第 7 條），再走 computed style 收集每個元素的 `url()`（`display: none` 的子樹不收，Chrome 也不載），用 `RequestDestination::Image` fetch，存進現有的 `images` 表；同一個 URL 只載一次，`<img>` 和背景共用，算進 pending、延後 `load`。stylesheet 載到時，在下一次 `start_loads` 看過它的規則之前 `load` 不會觸發（`background_images_unchecked`）。還沒載到的那一層先不畫（Chrome 也是）；載到時用到它的元素交給 `PaintController::invalidate`（第 8 條），inline 元素的背景在它那個 block 的行裡，所以那個 block 也重畫。
- **paint**：`BoxDecorationBackground` 這個 phase 內部的順序照 CSS 2 附錄 E 和 Blink 的 `BoxPainterBase`：外陰影 → 背景色和每一層背景圖（由底層往上）→ 內陰影 → 邊框 → outline。
  - 每一層的 clip 是 `background-clip`（border / padding / content box），位置和大小是 `background-origin` 加 `background-position` / `-size`，重複由 `background-repeat` 決定——重複交給 raster 的 shader（tile mode），不展開成很多個 `DrawImage`。每一層是一個 display item：`DrawRect`（要畫的範圍：clip 的盒子，不重複的那一軸只到 tile 為止）加上 `PaintFlags::shader`（`PaintShader`：圖片或漸層、第一個 tile 的位置和大小、`space` 的間隔、兩軸重不重複）。幾何照 Blink 的 `BackgroundImageGeometry`：padding box 和 content box 從對齊 device pixel 的 border box 扣掉「當成 border 寬度取整」的邊框（和邊框的畫法一致，WPT 的 `background-clip-padding-box-001` 找到的：原本用 layout 沒取整的寬度，背景和邊框之間差一個 pixel）；tile 的位置對齊 device pixel、大小取整到 device pixel（至少一個，WPT 的 `subpixel-repeat-no-repeat-mix`）；`round` 照規範縮放，只有一軸 `round`、另一軸 `auto` 時保持比例；`space` 放得下兩個以上才平均分開，否則照 `background-position` 放一個。
  - 根元素（或 `<body>`）的背景畫滿整個 canvas，圖照根元素的盒子定位（「as if specified on the root element」）；根元素只有背景圖、沒有顏色時也是它的。inline 元素的背景每一行各自定位（沒有 `box-decoration-break: slice`）。
  - 漸層（`paint` 的 `background.rs`，Blink 的 `CSSGradientValue` 和 `Gradient`）：線性的方向一律換成角度（邊角是垂直於相鄰兩角連線的方向），線長讓兩端的垂線碰到角落；放射的大小照 `radial-size` 的四個關鍵字（橢圓的 corner 照 side 的比例）。色停點照 css-images 的 fixup（第一個、最後一個補 0 和線長，往回的拉到前一個，沒有位置的平均分），hint 換成九個色停點近似（Blink 的 `ReplaceColorHintsWithColorStops`）。非 repeating 的漸層用它自己的線（停點在線外時延長）：原本把線縮到第一個到最後一個停點之間，停點都在同一處時線短於一個 pixel，pixel 在銳利邊緣的哪一邊變成捨入的問題（WPT 的 `linear-gradient-non-square` 找到的）。repeating 的線是一個週期；週期是 0 時整片是最後一個顏色（css-images 4；原本照舊的規範取平均，WPT 的 `normalization-*-degenerate` 找到的）。
  - 圓角（第 9 條）：`gfx::RRectF` 是矩形加四個角的半徑（Blink 的 `FloatRoundedRect`、Skia 的 `SkRRect`，角的順序左上、右上、右下、左下），往內縮（`inset`）時半徑減掉那兩邊的寬度、不小於 0。
    - **clip**：`PropertyTrees::ClipNode` 多四個角的半徑（Blink 的 `ClipPaintPropertyNode` 帶 rounded rect），display list 是 `ClipRRect`。`overflow` 的 clip 是 padding edge 的曲線：對齊 device pixel 的 border box 扣掉取整過的邊框（和背景的 padding box 同一個算法）。沒有 `overflow` clip 的圖片和 iframe 的內容另外切成 content box 的曲線（Blink 的 `InnerBorderRadiusClip`）。
    - **背景**：圓角的範圍用 `DrawRRect`：背景色一個，每一層圖片或漸層也一個，畫滿整個圓角範圍，shader 的 `decal` 讓不重複的那一軸在 tile 外面什麼都不畫（Skia 的 `TileMode::Decal`；矩形的範圍照舊只畫到 tile、tile mode 是 clamp，第 1 段的畫面不變）。邊框不透明、每邊都在、而且是 solid 時，clip 到 border box 的背景只畫到邊框的一半（Blink 的 `kBackgroundBleedShrinkBackground`）：邊框和背景的外側曲線落在同樣的 pixel 上，邊框抗鋸齒的半透明 pixel 會讓底下的背景透出來（WPT 的 `background-clip-*-with-border-radius-003` 和 `background-image-centered-with-border-radius` 找到的）。
    - **邊框**：照下面「邊框」的畫法，環是外側曲線和 padding edge 曲線之間的 `DrawDRRect`。
    - **inline 元素**：每一行各自畫（`box-decoration-break: slice`），開頭的兩個角只在它開始的那一行、結尾的兩個角只在它結束的那一行；layout 的 `InlineBoxFragment` 為此多帶 `first`、`last` 和那一行的 padding（不在頭尾的那一側是 0，百分比由 layout 算好），content box 因此也對了。
    - **hit test**：盒子的圓角外面、和圓角 clip 的外面都打不到（`RRectF::contains`：點在某個角的範圍裡時看它在不在橢圓裡，Blink 的 `FloatRoundedRect::IntersectsQuad`）。
  - **compositor 和 Viz**：compositor 端的 `ClipNode` 也帶半徑（`PropertyTreeManager` 照抄）；畫 quad 時，layer 的 clip 鏈裡最內層的圓角換成 target 的座標（乘上縮放、減掉捲動位移）放進 `SharedQuadState::rounded_clip`（Viz 的 `mask_filter_info`，Chromium 的 fast rounded corners）。`SurfaceAggregator` 嵌入 surface 時，quad 自己沒有圓角的就沿用嵌入它的 quad 的。`WgpuRenderer` 把圓角放進頂點、在 fragment shader 裡照 pixel 的中心到曲線的距離算覆蓋率（橢圓用 `f/|∇f|` 近似距離），`SoftwareRenderer` 逐個 pixel 算一樣的值（`rounded_corner`）。每個 quad 只帶一個圓角，所以 `layerize` 的捲動容器只在它的 overflow clip 鏈裡最多一個圓角時才成為 layer（Chromium 這時改用 render surface 和 mask，kun 還沒有 render surface）。
  - 陰影（第 10 條）：外陰影在背景之前、內陰影在背景之後、邊框之前（Blink 的 `BoxPainterBase::PaintNormalBoxShadow`、`PaintInsetBoxShadow`），最後一個先畫，第一個在最上面。外陰影是 border box 移動 offset、再照 spread 放大（`RRectF::spread`：圓角照 css-backgrounds 的公式，比 spread 小的半徑長得少，直角不變），只畫在 border box 外面；內陰影是 padding box 減掉「padding box 移動 offset、縮小 spread」的洞，只畫在 padding box 裡面。模糊是 `PaintFlags::blur`（標準差，blur 半徑的一半），一個陰影一個 `DrawShadow`。inline 元素每一行各畫完整的陰影（沒有 `slice`）。
    - **outline**（第 11 條）：每個 stacking context 在行之後、`z-index` 0 以上的子 stacking context 之前畫一輪（Blink 的 `PaintPhase::kOutline`；不是 Appendix E 的最後一步），照樹的順序，每個盒子和它的行上 inline 元素的；整個畫的 flex item 和 atomic inline 在它畫的地方畫自己的。outline 每次重畫、不沿用上一次的 display item：它少見，而盒子自己的和行上 inline 元素的在不同的 property state 下，沿用要記每一個的 state。實線、虛線等的 outline 照下面「邊框」的畫法，四邊同一個樣式，圓角跟著邊框（`spread`）；負的 offset 讓它翻過來時，每個方向至少是兩倍的寬度、置中（css-ui 4；WPT 的 `outline-013` 到 `-016` 找到的）。`auto` 是 Chrome 的 focus ring：2px 的 outline 顏色貼著盒子、外面 1px 白色，盒子是直角時角的半徑是 3px（Blink 的 `PaintFocusRing`、`DefaultFocusRingCornerRadius`）。UA stylesheet 照 Chrome 加 `:focus-visible { outline: auto 1px -webkit-focus-ring-color }`。
    - **畫到盒子外面的範圍**（第 12 條）：外陰影的 item 的 `visual_rect` 是形狀加上三個標準差，outline 的是外框；`layerize`、`RasterInvalidator` 和 tile 的 interest rect 照舊用它。
  - **邊框**（第 13、14 條；Blink 的 `BoxBorderPainter`）：每一邊的寬度照邊框的寬度取整到 device pixel，外框對齊 device pixel。每一邊是填滿的條紋或虛線：
    - 條紋是「從外緣往內，各邊寬度的幾分之幾到幾分之幾」，四邊照同一個比例接成環：`solid` 一條；`inset` 上和左變暗、下和右變亮，`outset` 相反（Blink 的 `CalculateBorderStyleColor` 和 `Color::Dark`、`Color::Light`：已經夠暗的不變暗、夠亮的不變亮，黑色變亮是 `rgb(84, 84, 84)`）；`double` 兩條，寬度照 Blink 的 `GetDoubleBorderStripeWidths` 用整數的 device pixel 分三份，不到 3 個 device pixel 時一條；`groove` 外半是 `inset`、內半是 `outset`，`ridge` 相反。
    - `dashed` 和 `dotted` 是沿著那一邊中線的 stroke（圓角時是整圈的中間曲線），切成那一邊的梯形（`DrawClipped`）：dash 是寬度的兩倍、間隔一倍（3px 以下是三倍和兩倍），間隔照 Blink 的 `SelectBestDashGap` 調到兩端落在 dash 上（圓角的整圈是 closed path，dash 和間隔一樣多），放不下兩段加一個間隔時是實線；點是長度 0 的圓頭 dash，相隔約兩個寬度，第一個和最後一個在離角半個寬度的地方。
    - 每一邊有寬度的都畫得一樣（直角的盒子少一邊也算；圓角的要四邊都在，否則少的那邊的角會畫出一條細縫）時，每一條條紋畫成整個環：直角是一個帶洞的 path，圓角是 `DrawDRRect`，沒有對角線的接縫讓抗鋸齒漏出底下的東西（Blink 的 `DrawDRRect` fast path）。否則每一邊各畫：直角的盒子同一個顏色的梯形合成一個 path（一樣沒有接縫），圓角的每一條是切成那一邊梯形的 `DrawDRRect`。
    - 背景的 bleed avoidance 只在每一邊都是填滿的樣式（不含虛線、點線和 `double`）時才算邊框蓋住背景的邊（Blink 的 `ObscuresBackgroundEdge`）。
- **raster**：`PaintOp` 照 cc 帶畫法的描述，raster 時才變成 Skia 的物件。
  - `PaintFlags::shader`（cc 的 `PaintShader`）：圖片是 `SkImageShader`，漸層是 `SkGradientShader` 的 linear 和 two-point conical（同一個圓心，起訖兩個半徑，橢圓用 local matrix 縱向縮放），顏色照規範和 Chrome 以 premultiplied 內插。不重複的軸，tile mode 是 clamp（op 的範圍本來就只到 tile），`decal` 時是 `TileMode::Decal`（圓角的背景畫滿整個範圍）。有間隔的 tile、要重複的和 `decal` 的漸層畫成一個 picture（tile 加間隔），再用 picture shader 重複（cc 的 `ImageTilingInfo` 的做法），因為漸層自己的 tile mode 只沿著它的線。
  - `PaintFlags::blur` 是 `SkMaskFilter::MakeBlur`（跟著目前的矩陣縮放），`PaintFlags::dash` 是 `SkDashPathEffect`（點是圓頭）。
  - `DrawRRect` 是四個角各自半徑的 rrect（瀏覽器的 UI 也改用它）；`ClipRRect`、`DrawDRRect`（帶 clip 的多邊形時先 `clip_path`）；`DrawShadow` 是 save、`clip_rrect`（外陰影用 `Difference`）、`draw_rrect` 或 `draw_drrect`、restore；`DrawClipped` 是 save、`clip_path`、裡面的 op、restore。

### 分階段

四段（第 1 條），每段開始前再決定那一段的細節：
1. **背景圖和漸層**：style 的背景層、renderer 載圖、raster 的 shader、`background-position` / `-size` / `-repeat` / `-clip` / `-origin` 和多層。
2. **`border-radius`**：style 的半徑、圓角的背景和邊框、clip node 和 `overflow` 的圓角、hit test。
3. **陰影和 outline**：`box-shadow`（含 `inset`）、`outline`、`outline-offset`，以及 visual rect / layer / 失效範圍的放大。
4. **border style**：`dashed`、`dotted`、`double`、`groove` / `ridge` / `outset`，以及圓角邊框的接縫。

四段都照這個順序做完，每一段的驗收和檢查在 `progress.md`。

### 測試

- WPT 的 reftest：照 RFC 0003 的做法用候選腳本挑，每一段放行那一段的屬性和值，在整個 `css/` 上重跑，新的候選全部收進來（四段共 501 個），過不了的記進 `reftest-expectations.txt` 附上原因；testharness 的 parsing 測試也一樣。
- 自己的 reftest（`tests/reftests/backgrounds/`）：頭像的 `cover` / `contain`、漸層、多層和 box、`space` / `round`；圓角的 clip、邊框和成為 layer 的捲動容器；陰影；outline；邊框的樣式。
- 情境：`about:demo` 的卡片（漸層、圓角、陰影，「Restyle the card」換成兩層）和圓的頭像（`backgrounds`）；圓的元素的 hit test 和圓角的捲動容器（`rounded`）；focus ring 和陰影（`focus-ring`）。
- `style`、`paint`、`raster`、`compositor`、`viz` 的單元測試：驗收的清單在 `progress.md`。

## 缺點

- 這份的範圍很大（背景一段就有 8 個 longhand 和多層的疊法），分段之後每一段都還不小。
- `PaintOp` 和 `PropertyTrees` 的資料結構要擴充，所有既有的 paint 測試都會碰到（圓角讓 clip 從 `RectF` 變成帶半徑的型別）。
- 陰影和 outline 打破「盒子畫在自己的 border box 裡」這個現在到處成立的假設，layer 和失效範圍要一起改。
- raster 開始用 Skia 的 shader 和 mask filter，和 Chrome 的差異會從「少畫東西」變成「畫得不太一樣」（抗鋸齒、模糊的實作差異），reftest 的容差要重新看。

## 理由和其他做法

2026-10-09 定了 A、B、C（「已決定的事項」第 1 到 3 條）；第 1 段開始前定了 G、H，和那一段多出來的 I、J、K（第 4 到 8 條）；第 2 段開始前定了 D（第 9 條）；第 3 段開始前定了 F，和那一段多出來的 L、M（第 10 到 12 條）；第 4 段開始前定了 E，和那一段多出來的 N（第 13、14 條）。完整的比較見 `details.md`。

- **A. 怎麼分段**：A1 四段（背景 → 圓角 → 陰影和 outline → border style）；A2 兩段（背景；圓角和裝飾）；A3 五段（把漸層和 `url()` 的圖片分開）。決定：A1（第 1 條）。
- **B. 背景圖誰去載**：B1 renderer 走 computed style 收集 `url()`（和現在收 `<img>`、stylesheet 一樣）；B2 `style` 在 cascade 時產出一份請求清單給 renderer；B3 `dom_api` 這一側載。決定：B1（第 2 條）。
- **C. 漸層在哪裡算**：C1 `PaintOp` 帶漸層的描述，raster 用 Skia 的 shader 畫（照 cc 的 `PaintShader`）；C2 `paint` 自己 raster 成 bitmap 再 `DrawImage`；C3 `style` 把漸層展開成一堆色塊。決定：C1（第 3 條）。
- **D. 圓角的 clip 做到多深**：D1 clip node 和 `PaintOp` 都帶圓角，`overflow` 和 hit test 都照圓角（D1a：成為 layer 的捲動容器不能有圓角，有的就不成為 layer；D1b：compositor 和 Viz 也會切圓角）；D2 只有背景和邊框畫成圓角，clip 仍是矩形（`overflow: hidden` 的卡片會漏直角，記成限制）。決定：D1b（第 9 條）。
- **E. border style 做多少**：E1 全部（含 `groove` / `ridge` / `outset` 的深淺）；E2 只做 `dashed`、`dotted`、`double`，其餘畫成 `solid`（和現在 `inset` 的做法一致）。決定：E1（第 13 條）。
- **F. 陰影的模糊**：F1 raster 用 Skia 的 blur mask filter；F2 `paint` 自己算模糊、產出 bitmap。決定：F1（第 10 條）。
- **G. 多層背景**：G1 完整的層清單；G2 只支援一層（第二層以後丟掉）。決定：G1（第 4 條）。
- **H. `background-attachment`**：H1 只做 `scroll`，`fixed` 和 `local` 記成限制；H2 連 `fixed` 一起做（要像 `position: fixed` 那樣牽動 compositor）。決定：H1（第 5 條）。
- **I. `url()` 的 base URL**：I1 解析時帶 base URL（外部 stylesheet 用它自己的網址），computed value 是絕對網址；I2 存寫的字串，renderer 一律用文件的 base URL 解析。決定：I1（第 6 條）。
- **J. 背景圖什麼時候發現**：J1 `start_loads` 先把 style 算到最新再收集，背景圖延後 `load`；J2 只在 lifecycle 的 style 之後收集，不延後 `load`。決定：J1（第 7 條）。
- **K. 載好的圖怎麼交給 paint**：K1 paint 依 URL 查 renderer 的圖片表，圖載到時只讓用到它的元素重畫；K2 layout 把圖放在 fragment 上，載到時重建盒子（和 `<img>` 一樣）。決定：K1（第 8 條）。
- **L. `outline-style: auto` 和 focus ring**：L1 `auto` 畫成 Chrome 的 focus ring，UA stylesheet 加 `:focus-visible` 的規則；L2 `auto` 解析但畫成 solid，沒有 UA 的規則；L3 不接受 `auto`。決定：L1（第 11 條）。
- **M. 陰影和 outline 畫到盒子外面的範圍（ink overflow）在哪裡算**：M1 `paint` 放大 display item 的 `visual_rect`；M2 layout 在 fragment 上算 ink overflow。決定：M1（第 12 條）。
- **N. 虛線和點線怎麼畫**：N1 raster 用 Skia 的 dash path effect（cc 的 `PaintFlags::setPathEffect`）；N2 `paint` 自己算出每一段虛線和每一個點。決定：N1（第 14 條）。

## 先例

- **Chromium**：`ComputedStyle` 的 `FillLayer` 鏈（背景的每一層）、`BoxPainterBase::PaintFillLayers`（順序和 clip）、`BoxBorderPainter`（邊框的多邊形和樣式）、`BoxPainterBase::PaintNormalBoxShadow` / `PaintInsetBoxShadow`、`FloatRoundedRect`（圓角的 clip 和命中判斷）、`cc::PaintShader`（漸層和 tile）、`ClipPaintPropertyNode` 的 rounded rect。
- **Servo**：`display_list_builder` 的背景層、`webrender` 的 `GradientBuilder` 和 `BorderRadius`。
- **Skia**：`SkGradientShader`、`SkImageShader`（tile mode）、`SkMaskFilter::MakeBlur`、`SkRRect`。

## 已決定的事項

1. **分成四段：背景圖和漸層、`border-radius`、陰影和 outline、border style（A1）**（2026-10-09）：每一段自己就能驗收（自己的 reftest 加那一段放行的 WPT 候選），第 1 段最大、也最早把畫面補起來；`border-radius` 單獨一段，因為它把 clip 從 `RectF` 換成帶圓角的型別，改動集中在一段裡比較好審。不選兩段（A2）：第 2 段會同時動 clip 的型別、「item 畫在自己盒子裡」的不變式和邊框的畫法，一次要驗收的面太大；不選五段（A3，把 `url()` 的圖片和漸層分開）：兩段都要碰同一批 `background-*` longhand 和同一組測試，等於做兩次。
2. **`background-image` 的圖片由 renderer 走 computed style 收集（B1）**（2026-10-09）：和現在收 `<img>` 的 `src`、stylesheet 的 `href` 同一個地方（`FrameView::start_loads`）、同一張 `images` 表，所以同一個 URL 只載一次、`<img>` 和背景共用解碼的結果；`style` 只負責「這一層的圖是這個 URL」。Blink 也是由 document 的 fetcher 去載，不是 `StyleEngine` 自己。不選「`style` 在 cascade 時產出請求清單」（B2）：`style` 會多一個對外要資源的角色，和 kun 其他地方（`style` 只做解析和 cascade）不一致，省下的只是 renderer 走一次樹，而它為了 `<img>` 本來就要走。
3. **漸層和重複的背景圖交給 raster 的 Skia shader（C1）**（2026-10-09）：`PaintOp` 帶漸層的描述（線性或放射、角度或邊角、色停點、`repeating`）和重複的方式，raster 時才做成 `SkShader`——照 cc 的 `PaintShader`。display list 保持小、可以重放，換了縮放重新 raster 也不失真；重複用 shader 的 tile mode，不展開成很多個 `DrawImage`。不選「paint 先 raster 成 bitmap 再 `DrawImage`」（C2）：縮放會模糊，每次失效都要重算一張 bitmap；不選「`style` 展開成色塊」（C3）：只能近似。

4. **多層背景照完整的層清單做（G1）**（2026-10-09，第 1 段開始前）：`ComputedStyle` 帶一份背景層的清單（Blink 的 `FillLayer` 鏈），每個 longhand 是逗號分隔的清單，層數照 `background-image`，其他 longhand 不夠時循環補上（css-backgrounds 3 § 2.1）。`background-image: url(icon.png), linear-gradient(...)` 這種「圖示 + 漸層」的寫法常見，`background` shorthand 的解析和序列化本來就要處理多層。不選只做一層（G2）：省下的工很少，而且資料結構之後還是要改成清單。
5. **`background-attachment` 只畫 `scroll`（H1）**（2026-10-09，第 1 段開始前）：三個值都解析、cascade、序列化（`background` shorthand 和 CSSOM 要讀得回來），但 `fixed` 和 `local` 畫成 `scroll`，記成限制——和現在 `inset` 的邊框畫成 `solid` 一樣。`fixed` 要讓背景畫在 viewport 的大小上、不隨文件捲動，等於另一套 `position: fixed` 的 layer 和 compositor 的處理；Speedometer 的 18 個 suite 用到的都是 shorthand 展開的 `scroll`。不選連 `fixed` 一起做（H2）：工作量和這一段其他部分加起來差不多，用的人很少。
6. **`url()` 在解析時對 base URL 解析（I1）**（2026-10-09，第 1 段開始前）：照 Blink 的 `CSSParserContext`，外部 stylesheet 的 `url()` 對那份 stylesheet 的網址（轉址之後的回應網址）解析，`<style>` 和 `style` attribute 對文件的 base URL；`ComputedStyle` 存絕對網址，`getComputedStyle` 讀到 `url("http://…")`，和 Chrome 一樣（`element.style` 讀回寫的字串）。所以 `LinkedStyleSheets` 多存回應的網址，`StyleEngine::recalc_style` 多拿文件的 base URL，`style` 多依賴 `url` crate；base URL 變了（`<base>`）時整份重新解析，和 stylesheet 的文字變了一樣。不選「存寫的字串、renderer 解析」（I2）：外部 stylesheet 裡的相對網址會被解析錯（`css/style.css` 裡的 `url(img.png)` 指的是 `css/img.png`），`getComputedStyle` 也讀不到絕對網址。
7. **`start_loads` 先把 style 算到最新，再收集背景圖（J1）**（2026-10-09，第 1 段開始前）：`start_loads`（parser 停下、script 跑完時）先跑一次 style recalc（只有 style，不排版），再走 computed style 收集 `url()`，和 `<img>` 一樣算進 pending、延後 `load`。Chrome 和 Firefox 的 `load` 也等已經要求的 CSS 圖片，reftest 因此不用 `reftest-wait` 就截得到背景圖。style recalc 是增量的，之後 lifecycle 的那一次大多沒事可做。不選「只在 lifecycle 的 style 之後收集」（J2）：`load` 可能在背景圖被要求之前就觸發，reftest 和頁面的 `onload` 都會看到沒有背景圖的畫面。
8. **paint 依 URL 查 renderer 的圖片表，圖載到時只重畫用到它的元素（K1）**（2026-10-09，第 1 段開始前）：照 Blink 的 `StyleImage`（style 指著 `ImageResourceContent`）和 `ImageChanged` → `SetShouldDoFullPaintInvalidation`：`ComputedStyle` 只存網址（純資料，`NoTrace` 的條件不變），paint 從每個 frame 的圖片表（和 `<img>` 共用，`ImageState`）查到圖和它的自然大小；圖載到時，renderer 把 style 用到那個網址的元素交給 paint，`PaintController` 不沿用它們上一次的 display item，其他的照舊沿用。不重建盒子、不重排：背景圖不影響 layout。不選「layout 把圖放在 fragment 上、載到時重建盒子」（K2）：一張背景圖載到就要重建盒子和重排，而背景圖比 `<img>` 多很多（一個 sprite 可能被上百個元素用到）。

9. **圓角一路做到 Viz（D1b）**（2026-10-10，第 2 段開始前）：`border-radius` 進 `PropertyTrees::ClipNode` 和 `PaintOp`（圓角的 clip、四個角各自半徑的 rrect），`overflow` 的 clip、`<img>` 和 iframe 的內容、背景和邊框都照圓角，hit test 用同一份資料判斷點在不在圓角裡（Blink 的 `FloatRoundedRect`、`ClipPaintPropertyNode` 帶 rounded rect）。成為 layer 的捲動容器的圓角由 compositor 交給 Viz：`SharedQuadState` 多一個圓角矩形，wgpu 的 renderer 在 fragment shader 裡算覆蓋率，software 的逐個 pixel 算（Chromium 的 fast rounded corners，`SharedQuadState::mask_filter_info`），所以圓角的捲動容器照樣不等 main thread 就能捲。一個 layer 只帶一個圓角：同時在兩層以上的圓角 clip 裡的捲動容器不成為 layer（Chromium 那時改用 render surface 加 mask，kun 還沒有 render surface），記成限制。不選 D1a（圓角的捲動容器一律不成為 layer）：圓角的捲動區（聊天、清單、卡片裡的內容）很常見，它們會全部退回 main thread 捲；也不選 D2（clip 仍是矩形）：圓角加 `overflow: hidden` 是最常見的卡片寫法，漏直角很明顯，之後補 D1 時 `ClipNode` 的型別還要再改一次。

10. **陰影的模糊用 Skia 的 blur mask filter（F1）**（2026-10-10，第 3 段開始前）：`PaintFlags` 帶一個 blur（cc 的 `PaintFlags::setMaskFilter`），raster 時才變成 `SkMaskFilter::MakeBlur`，sigma 是 blur 半徑的一半（Blink 的 `BlurRadiusToStdDev`）。和 Chrome 用同一套模糊，結果幾乎一樣，WPT 的容差吸收得了；display list 只帶描述，縮放之後重新 raster 也不失真。不選 `paint` 自己算（F2）：要自己寫高斯模糊、產出 bitmap，縮放會糊、每次失效都重算，結果也和 Chrome 不同。
11. **`outline-style: auto` 畫成 Chrome 的 focus ring（L1）**（2026-10-10，第 3 段開始前）：`auto` 照 Blink 的 `FocusRingPainter` 畫（外圈深色、內圈白色、跟著 `border-radius`），UA stylesheet 照 Chrome 加 `:focus-visible { outline: auto 1px -webkit-focus-ring-color }`。kun 已經有 `:focus-visible` 和 Tab 的循序導覽，卻看不到 focus 在哪裡，這是最直接的補法。focus ring 的樣子照平台，reftest 和用 `auto` 的參考頁比對，外觀在視窗裡看。不選畫成 solid、不加 UA 的規則（L2）：鍵盤的 focus 還是看不見，寫 `outline: auto` 的頁面也和 Chrome 不同；不選不接受 `auto`（L3）：寫了 `auto` 的宣告整個丟掉。
12. **畫到盒子外面的範圍由 `paint` 放大 `visual_rect`（M1）**（2026-10-10，第 3 段開始前）：陰影的 blur、spread、offset 和 outline 的 offset、寬度算進 display item 的 `visual_rect`（Blink 每個 item 的 visual rect）。`layerize`、`RasterInvalidator`、tile 的 interest rect 和「成為 layer 的捲動容器被蓋到」的判斷本來就都用 `visual_rect`，所以 layout 不用改；照規範也不算進捲動範圍。不選 layout 在 fragment 上算 ink overflow（M2，Blink 的 `NGInkOverflow`）：要改 layout 和它的增量更新，眼前的好處和 M1 一樣，等之後要用它來 cull 再做。

13. **border style 全部做，包括深淺（E1）**（2026-10-10，第 4 段開始前）：`dashed`、`dotted`、`double`、`groove`、`ridge`、`inset`、`outset`（和 `hidden`），深淺照 Blink 的 `BoxBorderPainter`：`inset` 上和左變暗、下和右變亮，`outset` 相反，`groove` 和 `ridge` 是兩半各一種（`Color::Dark()`、`Color::Light()`，已經夠暗或夠亮的顏色不變）。outline 用同一套畫法（Blink 的 `OutlinePainter` 也是）。UA stylesheet 的 `hr` 和 iframe 本來就是 `inset`，fieldset 的 `groove` 因為不支援而拿掉，做完都和 Chrome 一樣。不選只做虛線、點線和雙線（E2）：那三個 UA 的邊框還是和 Chrome 不同，省下的只是一個調顏色的函式。
14. **虛線和點線用 Skia 的 dash path effect（N1）**（2026-10-10，第 4 段開始前）：`PaintFlags` 帶 dash 的長度和間隔（cc 的 `PaintFlags::setPathEffect`），raster 用 `SkDashPathEffect` 沿著每一邊的中線畫，點線是長度 0、圓頭的 dash；間隔照 Blink 的 `StrokeData::SelectBestDashGap` 調整，讓兩端落在角上。和 Chrome 用同一套畫法，display list 也小。不選 `paint` 自己算每一段和每一個點（N2）：一條長的邊框就是幾百個 item，圓角時幾何很麻煩，抗鋸齒也和 Chrome 不同。

## 待決定

（沒有。）

## 之後的可能

- `conic-gradient()`、`image-set()`、`background-blend-mode`、`mask` 和 `clip-path`。
- `border-image`。
- `background-attachment: fixed` 和 `local`（H1 沒做的）。
- `text-shadow`（和 `box-shadow` 共用模糊）。
- SVG 當背景圖（等雜項線的 SVG）。
- `filter` 和 `backdrop-filter`（要 compositor 的 render surface）；有了 render surface，兩層以上圓角 clip 底下的捲動容器也能成為 layer。
- inline 元素跨行時的 `box-decoration-break: slice`（背景照連成一個的盒子定位、陰影在斷行處切掉、outline 連成一個外框）。
- outline 沿用上一次的 display item（每個 item 記它的 property state）。
- `<legend>` 的排版，之後 `<fieldset>` 才有 UA 的 `groove` 邊框。
