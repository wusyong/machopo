# RFC 0007 的進度：用 vello_cpu 取代 Skia

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-10，Windows 11。在 `0007` 上做。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0 圖片的解碼和讀尺寸、JPEG 的編碼換成純 Rust 的 crate（Skia 照舊畫） | 進行中 | |
| 1a 原型：常用的 op；量文字多的 tile、存截圖（第一個檢查點） | 還沒開始 | |
| 1b 原型：全部的 op 和組出來的功能；整套 reftest（第二個檢查點） | 還沒開始 | |
| 2 playback 換成 vello_cpu、ink bounds 改用 skrifa、拿掉 skia-safe、加上列出缺的功能的文件 | 還沒開始 | |
| 3 改進（文字的 gamma 和對比、漸層的 dithering、fork 裡的 `ColorMatrix`；可能不需要） | 還沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。
