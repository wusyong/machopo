# RFC 0122 的進度：site isolation 的路線圖

開始日期：2026-11-27。完成：2026-11-28（M1、M2 達成）。這份是路線圖，不直接實作：每一份的實作、驗收和做過的檢查在它自己的 `progress.md`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點（`details.md`） | 做完 | 2026-11-27 |
| 1. 路線圖層級的事（已決定的事項第 1 到 6 條） | 做完 | 2026-11-27 |
| 2. M1、M2 的五份 RFC（順序照第 6 條：A、C、B、N、F1） | 做完 | 2026-11-28 |
| 3. 收尾：M3、M4 各自開 RFC（第 7 條），M4 等原型 | 做完 | 2026-11-28 |

## 驗收

| 里程碑 | 項目 | 在哪裡驗證 |
| --- | --- | --- |
| M1 | renderer 送什麼都不能讓 Browser 結束：壞的訊息、壞的 frame 只結束那個 renderer；Browser 處理 renderer 輸入的模組沒有沒寫明理由的 panic | RFC 0123（A）的 `progress.md`。原本打算的「fuzzer 進每週的 CI」由它的已決定的事項第 14 條改成 lint 和測試 |
| M1 | `no-cors` 的跨源回應裡的 HTML、JSON、XML 到不了 renderer（ORB），`Cross-Origin-Resource-Policy`；renderer 的請求一定帶發起者 | RFC 0128（N）的 `progress.md`；WPT 的 `fetch/orb/`、`fetch/corb/`、`fetch/cross-origin-resource-policy/` |
| M1 | 網站可以拒絕被嵌（`X-Frame-Options`、`frame-ancestors`） | RFC 0129（F1）的 `progress.md`；WPT 的 `x-frame-options/`、`content-security-policy/frame-ancestors/` |
| M2 | 同一個分頁導覽到別的網站時換 renderer，舊的結束；session history、DevTools、WebDriver 的 session 跨過換 renderer 還在 | RFC 0124（C）的 `progress.md` |
| M2 | 每個 renderer process 鎖到一個最上層的網站，要別的網站的第一方資料（cookie、storage、blob URL、commit 的 origin）是壞訊息；測試用的被攻破的 renderer | RFC 0127（B）的 `progress.md` |

## 做過的檢查

- 第 6 條的順序（A 之後先 C、B，再 N、F1）是做完 A、開始 N 的盤點時改的：machopo 不送第三方的 cookie，ORB、CORP、`X-Frame-Options` 擋的大多已經擋了，C 擋的洞反而一直開著。
- 做的途中更正過路線圖的兩處：N 的「`no-cors` 的請求不帶發起者」是誤讀（正常的 renderer 每個子資源都帶，2026-11-28 開 N 時更正）；F1 的「Chromium 在 renderer 檢查 `frame-ancestors`」不對（讀 `ancestor_throttle.cc` 確認在 Browser，2026-11-28 開 F1 時更正）。
- 途中加進路線圖的：X（cross-origin isolated，開 N 時）、FM（Fetch Metadata，開 B 時）。
- M2 達成時照第 1 條應該重新評估 M4：要等原型（P）量過，原型還沒做，移到「之後的可能」（第 7 條）。
- 收尾時照 `docs/README.md` 的「一份 RFC 做完時」第 4 步看了 `docs/test-gaps.md`：「Browser 照 frame 打開 `localStorage` 時，frame 不在樹上也回答」那一列要等「有壞掉的 renderer 的測試工具」，RFC 0127 做了（`renderer_process.rs` 的 `rogue`）。補了 `browser` 的 `renderer_process::a_storage_area_of_no_frame_is_answered`：被攻破的 renderer 對一個樹上沒有的 frame 要 storage area，收到 `area: None` 的回答就以結束碼 7 結束，沒有回答的話等到逾時（看它怎麼結束，不靠時間差）；拿掉了那一列。mutation test（`mutants.toml`，1 個）：拿掉 Browser 回答 `None` 的那一支，測試失敗。
