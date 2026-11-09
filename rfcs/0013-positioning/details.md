# RFC 0013 的細節：`position`

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 現在的程式

2026-10-07 的程式（RFC 0003 完成時）：

- **style**：屬性的白名單（`crates/style/src/properties.rs`）沒有 `position` 和 `top` / `right` / `bottom` / `left`。`z-index` 存算出來的 `z_index` 和生效的 `effective_z_index`，只有 flex 和 grid item 的生效（RFC 0003 第 9 條）。
- **layout**：`taffy_style` 不設 `position`（taffy 的預設是 `Relative`，沒有位移）和 `inset`。box tree 維持 DOM 結構；flex 和 grid container 照 `order` 排出另一份 child 的清單。atomic inline 是 IFC 的 child（RFC 0003 第 21 條）。
- **paint**：stacking context 是 root，或 `opacity < 1`、有 `transform`、item 的 `z-index` 生效的 box；`stacking_children` 收集子 stacking context，以 `z-index` 穩定排序。item 和 atomic inline 整個畫完。hit test 是同樣順序的反向，主執行緒捲動區域的 `covered_scrollers` 走同一個 walker。
- **compositor**：viewport 由 compositor 捲動；捲動容器可以成為 layer，把頁面切成「之前、容器、之後」（P4 第 46 條）。`docs/limitations.md` 記著「範圍的不變式在加上 `position: fixed` 時要重新檢討」。

## taffy 0.14 的 `position`

- `Position::Absolute`：block、flex、grid 都把它從 flow 拿掉，對**父 box** 的 padding box 定位（`compute/block.rs` 的 `perform_absolute_layout_on_absolute_children`）；`inset` 是 `auto` 時用 static position（block 在排版時記下 `static_position`）。taffy 的模型裡父 box 就是包含塊，沒有「最近的有 `position` 的祖先」。
- `Position::Relative`：排版後套上 `inset` 的位移，不影響其他 box。
- 沒有 `fixed` 和 `sticky`。

## 參考資料

- CSS 2 § 9.3（定位的方式）、§ 9.9（`z-index`）、§ 10.1（包含塊）、§ 10.3.7 和 § 10.6.4（絕對定位的大小和位置）、附錄 E（paint 的順序）：https://www.w3.org/TR/CSS2/
- CSS Positioned Layout Module Level 3：https://www.w3.org/TR/css-position-3/
- Chromium：`third_party/blink/renderer/core/layout/out_of_flow_layout_part.cc`、`paint/paint_layer.h`、`paint/paint_property_tree_builder.cc`
