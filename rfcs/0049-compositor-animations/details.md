# RFC 0049 的細節：compositor 的動畫

## 現況（2026-10-16）

- **paint 的 layerize**（`crates/paint/src/layerize.rs`）：只有捲動容器（內容超出時）、canvas 和 main frame 的 `position: fixed` 會成為 layer；`can_be_layer` 要求一路到 root 的 transform 都只有平移、effect 的鏈是空的（沒有 opacity < 1）。layer 照 paint 的順序切：前面的內容、容器或 canvas、後面的內容，不做 overlap 的判斷。layer 裡的 transform 和 opacity 都畫進它的 display list（`SetMatrix`、`SaveLayerAlpha`），連續共用一個 effect 的 chunk 共用一個 `SaveLayerAlpha`（群組的 opacity）。
- **paint 的 property tree**：`TransformNode` 是完整的 2D affine 矩陣（`gfx::Transform`），`EffectNode` 只有 `opacity`，而且只在 opacity < 1 時才有（從 1 開始的動畫，tree 的形狀會在動畫中變）。
- **compositor**（`crates/compositor`）：`TransformNode` 只有一個 offset（`cc::TransformNode` 的平移的部分），沒有 effect tree；renderer 把 paint 的 node 轉過來時 `as_translation().expect(...)`。commit 是整份 `CommitState`（layer、property tree、捲動的位置……）；impl thread 每個 frame 只決定要不要 `BeginMainFrame`、有沒有要畫的（`needs_redraw` 只由 commit、捲動、visibility、context lost 設），`BeginFrameArgs.frame_time` 沒有用。沒有任何動畫的機制。frame 的產生（`generate_compositor_frame`）每個 layer 一個 `SharedQuadState`（clip、rounded clip，opacity 一直是 1），只有一個 render pass。property tree 一變就整個 frame 都是 damage。
- **Viz**（`crates/viz`）：`DrawQuad` 沒有 `quad_to_target_transform`（`protocol/src/viz.rs` 的註解寫著還沒有）；`SharedQuadState.opacity` 兩個 renderer 都照它畫（軟體的乘在 coverage 上，wgpu 的乘在顏色上），aggregator 也會往下乘，只是沒有人設成 1 以外的值；只畫 root render pass，沒有 render pass 的 quad、沒有離屏的 target。
  - wgpu：quad 的四個角在 CPU 上轉成 NDC，加上 affine 只是多一步；沒有 MSAA，旋轉的邊會有鋸齒，要在 shader 做邊緣的 AA；tile 用 nearest 的 sampler，有變形時要換成 linear（已經有）。
  - 軟體 renderer：`pixel_bounds` 是軸對齊的，取樣是從 `quad.rect` 線性對應；有矩陣要算變形後的範圍、每個像素的逆矩陣、邊緣的 coverage（bilinear 已經有）。
- **main thread 的動畫**（`crates/renderer/src/render_thread.rs` 的 `update_animations_and_send_events`）：每個 frame 設 timeline 的時間、tick、把在跑的元素標成要重新 style；`transform`、`opacity` 是 `StyleDifference::Paint`，但 box 照樣 `mark_needs_fragment`，layout、pre-paint、paint、layerize 每個 frame 都跑，raster invalidation 比較 chunk 的狀態，改變的範圍 re-raster。
- **自然的接點**：`update_rendering` 裡 `set_layers` 和 `commit` 的地方（`PaintOutput` 帶著動畫）、`CommitState`（帶 keyframe model）、`ProxyImpl::begin_frame` / `draw_or_answer`（用 `frame_time` tick、設 `needs_redraw`、動畫中一直要 frame）、`ToMainThread`（開始和結束的事件）。
- **測試**：`compositor` 約 111 個單元測試、`viz` 的軟體 renderer 有像素測試（沒有 opacity < 1、沒有矩陣的）；`browser` 的 `navigation.rs` 有 main thread 忙時照樣捲動的端對端測試（`DrivenViz`）；reftest 有 `effects/` 的 transform 和 opacity，沒有動畫的。

## 選項的比較

### 範圍

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **C1 照 Chromium 全做** | 有動畫的 transform、opacity 是 compositing reason；compositor 的 transform node 是矩陣、多 effect tree；Viz 的 `quad_to_target_transform`、render surface（群組的 opacity 離屏畫）；兩個 renderer 都支援；impl thread 的 `AnimationHost` 和 `KeyframeModel` | JS 忙時照樣動，每個 frame 不再 re-raster；和 Chromium 同構，之後的 `filter` 等接得上 | 最大：大約分 4 段；軟體 renderer 的逆矩陣取樣和邊緣的 AA、render surface 的離屏繪製；推翻 MVP 時期「`DrawQuad` 只平移」的決定（`rfcs/pre-rfcs/P4.md` 第 45 條） |
| **C2 先做單一 layer（2026-10-16 的看法）** | 同 C1，但不做 render surface：opacity 只在動畫的元素整個是一個 layer、裡面沒有別的 layer 時交給 compositor（這時 per-quad 的 opacity 就是正確的群組 opacity）；其他的照舊在 main thread | 常見的整塊淡入淡出、平移縮放都交出去；成本約 C1 的六成，render surface 留到之後 | 有巢狀 layer 的 opacity 動畫照舊在 main thread；Viz 和軟體 renderer 照樣要做 affine 的 quad |

第三個做法（不做）在 RFC 0048 的第 23 條比較過：那裡選了它，把這件事移到這份 RFC。
