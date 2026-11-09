# 有 RFC 之前的文件

MVP（P0 到 P4，2026-09-29 到 2026-10-04）的規劃、各階段的設計和進度。那時還沒有 RFC，設計寫在每個階段的 `Pn.md`；2026-10-04 MVP 完成後搬到這裡。

**2026-10-04 起凍結**：只修壞掉的路徑和引用，不再更新內容；找某個設計當初為什麼這樣做時讀這裡。仍然有效、要繼續維護的部分搬到了 `docs/`，原本的位置留下一行指向新的地方：

- 架構、thread 配置、crate 和選型、和 Chromium 刻意不同的地方 → `docs/architecture.md`；
- 各階段的已知限制（合併、只留目前仍然成立的）→ `docs/limitations.md`；
- 測試策略、測試守不住的地方 → `docs/testing.md`。

文件裡寫的「（之後）」「目前」都是 2026-10-04 時的狀態，現在的狀態以 `docs/` 為準。

| 文件 | 內容 |
| --- | --- |
| `MVP.md` | 整體目標、架構、crate 選型、各階段的範圍和驗收標準；原本的 P5、P6 改成 RFC 0003、0004 |
| `P1.md` | P1 靜態渲染：DOM 在 cppgc 上、HTML parser、style、layout、paint、reftest harness |
| `P2.md` | P2 像個瀏覽器：網路和 navigation、圖片、inline box、捲動、input、工具列 |
| `P3.md` | P3 基本 JS：wrapper、與 engine 無關的 DOM 層、script 執行、event loop、事件 |
| `P4.md` | P4 Chromium 式 compositor：compositor thread、layerize、invalidation、記憶體 |
| `PROGRESS.md` | 進度、crate 現況、測試守不住的地方、各階段的驗收、做過的檢查、已知限制 |
