# RFC 0078 的細節

## Chrome 152 的探測

2026-10-23，Fedora 44（AMD Ryzen AI 9 HX 370），Chrome for Testing 152.0.7977.82（`--headless=new`），經過 `scripts/bench/common.py` 的 `ChromePipe`（CDP）；同樣的 script 經過 `KunPipe` 在 kun（`0078` 上第 1 段的實作，從 `main` 的 `4c92c7a0` 開始）跑。

**方法**：每個情況一個 800×600、沒有邊框的 iframe，`documentElement.innerHTML` 設成 `<style>html, body { margin: 0 } div { height: 100px }</style>` 加上下表的頁面（kun 沒有 `document.write`），然後照順序做那些步驟，同一個 task 裡。「讀」之前先讀 `body.offsetHeight` 強迫 layout，讀 `scrollY` 或容器的 `scrollTop`。anchor 本身讀不到，用行為判斷：在預期的 anchor 前後改東西，看捲動位置跟不跟。

**結果**：54 個情況，kun 和 Chrome 152 每一個讀到的值都一樣（下表；`full_rebuild` 第一次是 190，改了之後一樣，見 `progress.md` 的「做過的檢查」）。另外在 Chrome 上用 `Emulation.setDeviceMetricsOverride` 設 2 倍跑了一次，結果和 1 倍完全一樣：Chrome 模擬的 device scale factor 不經過 zoom-for-dsf，捲動位置是整數 CSS px（`scrollTo(0, 150.5)` 讀回 151），anchoring 的調整也在 CSS px 上取整（插入 10.3px 時捲 10、10.6px 時捲 11）。kun 的 script 捲動照 Blink 的 `SnapScrollOffsetToPhysicalPixels` 取整到裝置像素，anchoring 也在裝置像素上取整（Blink 的 `ComputeAdjustment` 在 layout 單位上取整，zoom-for-dsf 時是裝置像素），2 倍時插入 10.3px 捲 10.5（`frame_view` 的 `the_scroll_is_in_device_pixels`）；1 倍時兩者一樣。

另外一頁（`shell` 的 `scroll_anchoring` 測試的頁面）直接在 Chrome 開：`150 / 180 / -50 / 180 / b / b / 290 / c`，和 kun 一樣。

| 情況 | 頁面 | 步驟 | 讀到的值 |
| --- | --- | --- | --- |
| `content_above` | `<div id=a></div><div id=b></div><div id=c></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；在 #a 前插入 30px 高的 div；讀 scrollY；#a 的 style 設成 `height: 150px`；讀 scrollY；#a 的 style 設成 `height: 0`；讀 scrollY；#filler 的 style 設成 `height: 3000px`；讀 scrollY | 180, 230, 80, 80 |
| `top` | `<div id=a></div><div id=b></div><div id=c></div><div id=filler style='height: 2000px'></div>` | 在 #a 前插入 30px 高的 div；讀 scrollY；捲 viewport 到 150；layout；捲 viewport 到 0；在 #a 前插入 30px 高的 div；讀 scrollY | 0, 0 |
| `scroll_again` | `<div id=a></div><div id=b></div><div id=c></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；在 #a 前插入 30px 高的 div；讀 scrollY；捲 viewport 到 260；#filler 的 style 設成 `height: 3000px`；讀 scrollY；#filler 的 style 設成 `height: 500px`；讀 scrollY；#filler 的 style 設成 `height: 3000px`；讀 scrollY | 180, 260, 230, 260 |
| `container` | `<div id=s style='height: 200px; overflow: auto'><div id=s1></div><div id=s2></div><div id=s3></div><div id=s4></div></div><div style='height: 2000px'></div>` | 捲 s 到 150；捲 viewport 到 10；在 #s1 前插入 30px 高的 div；讀 #s 的 scrollTop；讀 scrollY | 180, 40 |
| `suppress_margin` | `<div id=a></div><div id=b></div><div id=c></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#b 的 style 設成 `margin-top: 40px`；讀 scrollY；在 #a 前插入 30px 高的 div；讀 scrollY | 150, 180 |
| `suppress_wrap` | `<div id=a></div><div id=w style='height: auto'><div id=b></div></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#w 的 style 設成 `height: auto; padding-top: 20px`；讀 scrollY；捲 viewport 到 160；#w 的 style 設成 `height: auto; padding-top: 20px; border-top: 20px solid`；讀 scrollY | 150, 180 |
| `leave_flow` | `<div id=a></div><div id=b></div><div id=c></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#a 的 style 設成 `position: absolute; width: 10px`；讀 scrollY | 150 |
| `leave_flow_container` | `<div id=s style='height: 200px; overflow: auto'><div id=s1></div><div id=s2></div><div id=s3></div><div id=s4></div><div id=s5></div></div><div id=a></div><div style='height: 2000px'></div>` | 捲 s 到 150；捲 viewport 到 250；#s1 的 style 設成 `position: absolute; width: 10px`；讀 #s 的 scrollTop；#s 的 style 設成 `height: 300px; overflow: auto`；讀 scrollY | 150, 350 |
| `removed` | `<div id=a></div><div id=b></div><div id=c></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；移除 #b；讀 scrollY | 50 |
| `display_none` | `<div id=a></div><div id=b></div><div id=c></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#b 的 style 設成 `display: none`；讀 scrollY | 50 |
| `overflow_anchor_none` | `<div id=a></div><div id=b></div><div id=c></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#b 的 style 設成 `overflow-anchor: none; height: 130px`；讀 scrollY | 180 |
| `dpr2` | `<div id=a></div><div id=b></div><div id=c></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；在 #a 前插入 10.3px 高的 div；讀 scrollY | 160 |
| `partly_a1_at100` | `<div id=a style='height: 300px'><div id=a1 style='height: 120px'></div><div id=a2 style='height: 50px'></div></div><div id=b></div><div style='height: 2000px'></div>` | 捲 viewport 到 100；在 #a2 前插入 30px 高的 div；讀 scrollY | 100 |
| `partly_a2_at130` | `<div id=a style='height: 300px'><div id=a1 style='height: 120px'></div><div id=a2 style='height: 50px'></div></div><div id=b></div><div style='height: 2000px'></div>` | 捲 viewport 到 130；在 #a2 前插入 30px 高的 div；讀 scrollY | 160 |
| `abs_anchor` | `<div id=a style='position: absolute; top: 300px; width: 10px; height: 10px'></div><div id=b style='height: 250px'></div><div id=f style='height: 2000px'></div>` | 捲 viewport 到 260；在 #f 前插入 30px 高的 div；讀 scrollY | 260 |
| `overflow_extends` | `<div id=a style='height: 50px'><div id=a1 style='height: 200px'></div></div><div style='height: 2000px'></div>` | 捲 viewport 到 100；在 #a1 前插入 30px 高的 div；讀 scrollY | 130 |
| `overflow_hidden` | `<div id=a style='height: 50px; overflow: hidden'><div id=a1 style='height: 200px'></div></div><div id=b></div><div style='height: 2000px'></div>` | 捲 viewport 到 100；在 #a1 前插入 30px 高的 div；讀 scrollY | 100 |
| `nested_s1` | `<div id=pad style='height: 50px'></div><div id=s style='height: 200px; overflow: auto'><div id=s1></div><div id=s2></div><div id=s3></div></div><div style='height: 2000px'></div>` | 捲 viewport 到 100；在 #s1 前插入 30px 高的 div；讀 scrollY；讀 #s 的 scrollTop | 130, 0 |
| `none_subtree` | `<div id=a style='overflow-anchor: none; height: 200px'><div id=a1 style='overflow-anchor: auto'></div></div><div id=c style='height: 2000px'></div>` | 捲 viewport 到 10；在 #a1 前插入 30px 高的 div；讀 scrollY | 10 |
| `zero_height` | `<div id=a style='height: 0'><div id=a1 style='height: 0'></div></div><div id=b></div><div id=c style='height: 2000px'></div>` | 捲 viewport 到 10；在 #b 前插入 30px 高的 div；讀 scrollY | 40 |
| `container_inner` | `<div id=s style='height: 200px; overflow: auto'><div id=s1></div><div id=s2></div><div id=s3></div><div id=s4></div></div><div style='height: 2000px'></div>` | 捲 s 到 150；在 #s1 前插入 30px 高的 div；讀 #s 的 scrollTop；讀 scrollY | 180, 0 |
| `oof_fallback` | `<div id=r style='position: relative; height: 1000px'><div id=p style='height: 0'><div id=x style='position: absolute; top: 300px; width: 10px; height: 10px'></div></div></div><div style='height: 2000px'></div>` | 捲 viewport 到 100；#r 的 style 設成 `position: relative; height: 1000px; border-top: 30px solid`；讀 scrollY | 130 |
| `oa_existing` | `<div id=a></div><div id=b></div><div id=c></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；在 #a 前插入 30px 高的 div；讀 scrollY；#b 的 style 設成 `overflow-anchor: none; height: 130px`；讀 scrollY | 180, 210 |
| `remove_existing` | `<div id=a></div><div id=b></div><div id=c></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；在 #a 前插入 30px 高的 div；讀 scrollY；移除 #b；讀 scrollY | 180, 80 |
| `zero_adjust_kept` | `<div id=a></div><div id=b></div><div id=c></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；在 #a 前插入 30px 高的 div；讀 scrollY；#b 的 style 設成 `height: 0`；讀 scrollY；在 #c 前插入 30px 高的 div；讀 scrollY | 180, 180, 180 |
| `clamped` | `<div id=a></div><div id=b style='height: 1000px'></div><div id=c style='height: 1000px'></div><div id=d></div>` | 捲 viewport 到 1600；讀 scrollY；#a 的 style 設成 `height: 130px`；#d 的 style 設成 `height: 70px`；讀 scrollY | 1600, 1600 |
| `w_padding` | `<div id=a></div><div id=w style='height: auto'><div id=b></div></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#w 的 style 設成 `height: auto; padding-top: 20px`；讀 scrollY | 150 |
| `w_margin` | `<div id=a></div><div id=w style='height: auto'><div id=b></div></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#w 的 style 設成 `height: auto; margin-top: 20px`；讀 scrollY | 150 |
| `w_border` | `<div id=a></div><div id=w style='height: auto'><div id=b></div></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#w 的 style 設成 `height: auto; border-top: 20px solid`；讀 scrollY | 170 |
| `w_inset` | `<div id=a></div><div id=w style='height: auto; position: relative'><div id=b></div></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#w 的 style 設成 `height: auto; position: relative; top: 20px`；讀 scrollY | 150 |
| `w_transform` | `<div id=a></div><div id=w style='height: auto'><div id=b></div></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#w 的 style 設成 `height: auto; transform: translateY(20px)`；讀 scrollY | 150 |
| `w_height` | `<div id=a></div><div id=w style='height: 100px; display: flex; flex-direction: column; justify-content: flex-end'><div id=b style='flex: none'></div></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#w 的 style 設成 `height: 120px; display: flex; flex-direction: column; justify-content: flex-end`；讀 scrollY | 150 |
| `w_min_height` | `<div id=a></div><div id=w style='height: 100px; display: flex; flex-direction: column; justify-content: flex-end'><div id=b style='flex: none'></div></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#w 的 style 設成 `height: 100px; min-height: 120px; display: flex; flex-direction: column; justify-content: flex-end`；讀 scrollY | 150 |
| `w_max_height` | `<div id=a></div><div id=w style='height: 140px; max-height: 100px; display: flex; flex-direction: column; justify-content: flex-end'><div id=b style='flex: none'></div></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#w 的 style 設成 `height: 140px; max-height: 120px; display: flex; flex-direction: column; justify-content: flex-end`；讀 scrollY | 150 |
| `w_width` | `<div id=a></div><div id=w style='height: auto; width: 800px'><span style='display: inline-block; width: 300px; height: 20px'></span><span style='display: inline-block; width: 300px; height: 20px'></span><div id=b></div></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#w 的 style 設成 `height: auto; width: 400px`；讀 scrollY | 150 |
| `w_position` | `<div id=a></div><div id=w style='height: auto'><div id=b></div></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#w 的 style 設成 `height: auto; position: relative`；#a 的 style 設成 `height: 130px`；讀 scrollY | 150 |
| `outside_container` | `<div id=o style='height: auto'><div id=s style='height: 200px; overflow: auto'><div id=s1></div><div id=s2></div><div id=s3></div><div id=s4></div></div></div><div style='height: 2000px'></div>` | 捲 s 到 150；#o 的 style 設成 `height: auto; padding-top: 20px`；在 #s1 前插入 30px 高的 div；讀 #s 的 scrollTop | 180 |
| `rebuilt_display` | `<div id=a></div><div id=b></div><div id=c></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；在 #a 前插入 30px 高的 div；讀 scrollY；#b 的 style 設成 `display: flex`；在 #c 前插入 30px 高的 div；讀 scrollY | 180, 210 |
| `rebuilt_parent` | `<div id=a></div><div id=w style='height: auto'><div id=b></div></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；在 #a 前插入 30px 高的 div；讀 scrollY；#w 的 style 設成 `height: auto; display: flex; flex-direction: column`；在 #filler 前插入 30px 高的 div；讀 scrollY | 180, 210 |
| `kept_paint` | `<div id=a></div><div id=b></div><div id=c></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；在 #a 前插入 30px 高的 div；讀 scrollY；#b 的 style 設成 `background: red`；在 #c 前插入 30px 高的 div；讀 scrollY | 180, 180 |
| `w_min_width` | `<div id=a></div><div id=w style='height: auto; width: 400px'><span style='display: inline-block; width: 300px; height: 20px'></span><span style='display: inline-block; width: 300px; height: 20px'></span><div id=b></div></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#w 的 style 設成 `height: auto; width: 400px; min-width: 800px`；讀 scrollY | 150 |
| `w_max_width` | `<div id=a></div><div id=w style='height: auto; width: 800px'><span style='display: inline-block; width: 300px; height: 20px'></span><span style='display: inline-block; width: 300px; height: 20px'></span><div id=b></div></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#w 的 style 設成 `height: auto; width: 800px; max-width: 400px`；讀 scrollY | 150 |
| `w_border_wide` | `<div id=a></div><div id=w style='height: auto; width: 800px'><span style='display: inline-block; width: 300px; height: 20px'></span><span style='display: inline-block; width: 300px; height: 20px'></span><div id=b></div></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 150；#w 的 style 設成 `height: auto; width: 800px; border-top: 20px solid`；讀 scrollY | 170 |
| `inline_relpos` | `<div id=a></div><div id=t style='height: auto'><span id=s>Text</span></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 105；#s 的 style 設成 `position: relative; top: 20px`；讀 scrollY | 105 |
| `inline_control` | `<div id=a></div><div id=t style='height: auto'><span id=s>Text</span></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 105；在 #a 前插入 20px 高的 div；讀 scrollY | 125 |
| `container_explicit` | `<div id=s style='height: 200px; overflow: auto'><div id=s1></div><div id=s2></div><div id=s3></div><div id=s4 style='height: 300px'></div></div><div style='height: 2000px'></div>` | 捲 s 到 150；在 #s1 前插入 30px 高的 div；讀 #s 的 scrollTop；捲 s 到 260；#s4 的 style 設成 `height: 400px`；讀 #s 的 scrollTop | 180, 260 |
| `container_clamp` | `<div id=s style='height: 200px; overflow: auto'><div id=s1></div><div id=s2></div><div id=s3></div><div id=s4 style='height: 300px'></div></div><div style='height: 2000px'></div>` | 捲 s 到 150；在 #s1 前插入 30px 高的 div；讀 #s 的 scrollTop；#s4 的 style 設成 `height: 0`；讀 #s 的 scrollTop；#s4 的 style 設成 `height: 300px`；讀 #s 的 scrollTop | 180, 130, 180 |
| `oof_scroller` | `<div id=a></div><div id=s style='overflow: auto'></div><div id=b></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 210；#s 的 style 設成 `position: absolute; overflow: auto; width: 10px`；讀 scrollY | 210 |
| `full_rebuild` | `<body id=body><div id=a></div><div id=b></div><div id=c></div><div id=filler style='height: 2000px'></div></body>` | 捲 viewport 到 150；#b 的 style 設成 `margin-top: 40px`；#body 的 style 設成 `overflow-y: scroll`；讀 scrollY | 150 |
| `full_rebuild_control` | 同上 | 捲 viewport 到 150；在 #a 前插入 30px 高的 div；#body 的 style 設成 `overflow-y: scroll`；讀 scrollY | 180 |
| `text_own` | `<div id=a></div><div id=t style='height: auto; line-height: 20px'><span id=s>Text</span><br><span id=u>Text</span></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 105；#s 的 style 設成 `overflow-anchor: none`；#u 的 style 設成 `line-height: 60px`；讀 scrollY | 125 |
| `text_control` | 同上 | 捲 viewport 到 105；#u 的 style 設成 `line-height: 60px`；讀 scrollY | 105 |
| `root_display_inline` | `<div id=a></div><div id=t style='height: auto'><span id=s>Text</span></div><div id=filler style='height: 2000px'></div>` | 捲 viewport 到 105；#s 的 style 設成 `position: relative; top: 20px`；`<html>` 的 style 設成 `display: flow-root`；讀 scrollY | 105（kun 一開始是 125） |
| `root_display_control` | 同上 | 捲 viewport 到 105；在 #a 前插入 20px 高的 div；`<html>` 的 style 設成 `display: flow-root`；讀 scrollY | 105（kun 一開始是 125） |

另外一組不當測試：同樣的 `text_own` 頁面，改成在 `#u` 之前插入一個 20px 的 div（`#s` 設 `overflow-anchor: none`），Chrome 是 105、kun 是 125。Blink 在 inline 之間插入 block 時把兩邊包進新的匿名 block，它們沒有 layout 過（`EverHadLayout`），選 anchor 時連同裡面整段跳過，anchor 變成 `#t` 本身；kun 的 box tree 不把匿名 block 算進重建。只在 inline 和 block 混在一起、又剛好在 anchor 那裡插入時不同，記在 `docs/limitations.md`。

## 第 2 段：priority candidate

同樣的方法，頁面是 `<style>html, body { margin: 0 } body { height: 4000px } .s { height: 100px }</style>`、六個 100px 的 `.s`（第五個是 `#growing`）、下表的元素、再一個 `.s`；捲到 150，`#focused.focus()`，讀 `scrollY`，`#growing` 改成 3000px 高，再讀。focus 的元素是 priority candidate 時，捲動跟著它（3050），否則跟著一般選到的 anchor（`#growing` 之後的元素在可見範圍下面，所以不動，150）。

| 情況 | 元素 | `focus()` 之後 | Chrome 152 | kun |
| --- | --- | --- | --- | --- |
| contenteditable | `<div id=focused contenteditable style='height: 10px'>` | 150 | 3050 | 3050 |
| 在 editing host 裡 | `<div contenteditable><p id=focused style='margin: 0; height: 10px'>x</p></div>` | 150 | 3050 | 3050 |
| 部分在畫面裡 | `<div id=focused contenteditable style='height: 300px'>` | Chrome 150、kun 300 | 3050 | 3200 |
| 在畫面外 | `<div id=focused contenteditable style='height: 10px; position: relative; top: 2000px'>` | Chrome 2309、kun 2010 | 4910 | 4910 |
| inline 的 editing host | `<div id=box><span id=focused contenteditable>x</span></div>` | 150 | 3050 | 3050 |
| `<textarea>` | `<textarea id=focused style='height: 10px; display: block'>` | 150 | 150 | 150 |
| `<input>` | `<input id=focused style='height: 10px; display: block'>` | 150 | 150 | 150 |
| tabindex | `<div id=focused tabindex=0 style='height: 10px'>` | 150 | 150 | 150 |
| 在 `overflow-anchor: none` 裡 | `<div style='overflow-anchor: none'><div id=focused contenteditable …>` | 150 | 150 | 150 |
| 在 sticky 裡 | `<div style='position: sticky; top: 0'><div id=focused contenteditable …>` | 150 | 150 | 150 |

另外一種：`#focused` 是 `<div contenteditable>`，裡面一個 50px 的 `#growing2`、一個 10px 的 `<p>`、一個 200px 的 div；先 focus、再捲到 650（`<p>` 在最上面），`#growing2` 改成 100px：Chrome 和 kun 都是 700（在 priority candidate 裡面往下找，anchor 是 `<p>`）。

「部分在畫面裡」和「在畫面外」的差別在 `focus()` 自己的捲動（kun 捲到最近的邊、部分在畫面裡的也捲，`docs/limitations.md`），之後兩邊都跟著 focus 的元素移動 2900。

## 第 2 段：WPT 的 `css/css-scroll-anchoring/` 在 Chrome 上

用一個靜態的 HTTP server（`python3 -m http.server`，沒有 wptserve 的 substitution 和 testdriver）提供 `../third_party/wpt`，Chrome 152 開每個 testharness.js 測試，攔下 `add_completion_callback` 收結果。Chrome 沒通過的：`ignore-adjustments-while-scrolling-to-anchor`、`scroll-anchoring-editing-content-change`（3 個）、`contenteditable-insert-line-at-top`、`history-restore-anchors` 要 testdriver 或 wptserve，在這個環境下不算數；`anchor-inside-textarea`（310 對 300）、`dirty-contents-reselect-anchor`（1207 對 0）、`multicol-fragmented-anchor`（20 對 10）是 Chrome 自己沒過。kun 剩下的失敗各自的原因寫在 `.ini` 的註解。
