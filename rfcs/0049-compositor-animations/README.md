# RFC 0049：compositor 的動畫

- 開始日期：2026-10-16
- 狀態：📝 草稿
- 細節：`details.md`

## 摘要

照 Chromium，把 `transform` 和 `opacity` 的動畫交給 compositor thread 跑：Blink 判斷能不能交（`CompositorAnimations::CheckCanStartAnimationOnCompositor`），cc 的 `AnimationHost`、`KeyframeModel` 在 impl thread 上用 frame 的時間算值、改 property tree，main thread 只在開始時同步，照樣自己 tick、發事件。範圍（要不要做 render surface，`will-change` 要不要一起做）還沒決定，見「待決定」。

## 動機

RFC 0048（transition、CSS animation 和 Web Animations）的動畫全部在 main thread 上跑：每個 frame 重算動畫的元素的 style、重新 paint、re-raster 它所在的 layer；JS 忙的時候動畫會停（Chrome 的 `transform`、`opacity` 動畫不會）。RFC 0048 原本把這件事排成它的第 4 段，2026-10-16 決定移到這份（RFC 0048 的第 23 條）：它要改的是 paint 的 layerize、compositor 和 Viz 的架構，和 0048 的動畫模型分得開。

要解決的 `docs/limitations.md` 的限制：

- 「Transition 和 CSS animation」那一條的「全部在 main thread 上：每個 frame 重算動畫的元素的 style、重畫和重新 raster 它所在的 layer，JS 忙的時候不動」。
- 「Compositor、layer 和 input」的「transform 和 opacity 本身也不成為 layer（等有 `will-change`、動畫這類合成理由）」的動畫的部分。
- 「Viz、GPU 和記憶體」的「`DrawQuad` 只有平移」（做 affine 的 quad 時）。

**不在範圍內**：`filter`、`background-color` 等 Chrome 也能交給 compositor 的其他 property；3D transform（machopo 沒有）；scroll-driven animation。

## 設計概要

做完之後，`transform` 或 `opacity` 的 transition、CSS animation 和 `element.animate()`，在能交給 compositor 時（見「待決定」的範圍），JS 忙的時候照樣在動；每個 frame 不再重算 style、re-raster，只畫已經 raster 好的 layer。`getComputedStyle`、`currentTime`、事件和 promise 的行為不變（main thread 照樣 tick）。

## 詳細設計

草稿：現況和要改的地方在 `details.md` 的「現況（2026-10-16）」，選項在「選項的比較」。

### 測試

（草稿）`compositor` 和 `viz` 的單元測試（有矩陣、opacity 的 quad 和 property tree）、`browser` 的端對端測試（照 `scrolling_goes_on_while_script_keeps_the_main_thread_busy` 的做法：script 讓 main thread 忙的時候，動畫照樣出 frame）、reftest（有 transform、opacity 的 layer 畫得和沒有 layer 時一樣）。

### 分階段

（草稿）大致是：paint 的 compositing reason 和 compositor 的 property tree（矩陣、effect tree）；Viz 的 affine quad 和 opacity（兩個 renderer）；impl thread 的動畫和 main thread 的同步；（看範圍）render surface。

## 缺點

compositor、Viz 和兩個 renderer 都要改；軟體 renderer 要做逆矩陣取樣和邊緣的 AA；main thread 和 impl thread 各算一份動畫的值，要保持一致（開始時間、暫停、seek 時重新交出）。

## 理由和其他做法

見 `details.md` 的「選項的比較」。不做的話，動畫照舊在 main thread、每個 frame re-raster，JS 忙時會停。

## 先例

- **Chromium**：Blink 的 `CompositorAnimations` 判斷、`Animation::StartAnimationOnCompositor` 把 keyframe 和 timing 轉成 cc 的 `KeyframeModel`；cc 的 `AnimationHost` 在 impl thread 上 tick，改 property tree 的 transform node 和 effect node，`AnimationEvents` 把開始的時間送回 main thread（`NotifyAnimationStarted`）；有動畫的 transform、opacity 是 compositing reason（`CompositingReason::kActiveTransformAnimation`、`kActiveOpacityAnimation`）；群組 opacity 用 render surface。
- **Gecko**：WebRender 的 `AnimationInfo`，`transform`、`opacity` 等在 compositor 上跑。

## 已決定的事項

無。

## 待決定

- **範圍**（`details.md` 的「選項的比較」）：
  - C1：照 Chromium 全做，包括 render surface（離屏畫群組的 opacity），兩個 renderer 都支援。
  - C2：不做 render surface。opacity 只在動畫的元素整個是一個 layer、裡面沒有別的 layer 時交給 compositor，那時 per-quad 的 opacity 就是群組的 opacity；其他的照舊在 main thread。
  - 2026-10-16 的看法：C2，render surface 之後再做。
- **`will-change`** 要不要一起做成 compositing reason（要先解析 `will-change`）。
- **軟體 renderer**（headless、reftest）要不要和 wgpu 一樣畫 affine 的 quad，還是 headless 照舊不交給 compositor。

## 之後的可能

- render surface（若範圍選 C2）、`filter` 的動畫、scroll-driven animation 在 compositor 上跑。
