# RFC 0122 的路線圖：要開哪些 RFC、誰先誰後

這份放「site isolation 要切成哪些 RFC、順序、相依和規模」。為什麼這樣切、路線圖層級的選項在 `README.md`；現況的盤點和 Chromium 的做法在 `details.md`，這裡不重複。

**狀態**：2026-11-27 接受（`README.md` 的已決定的事項第 1 到 6 條）：M1、M2 一定做，照 A、C、B、N、F1 的順序（第 6 條改的，原本是 A、N、F1、C、B）；M3 照需要；M4 等 M2 做完、原型量過再決定。

**怎麼更新**：一份 RFC 建立時，把表裡它的代號換成編號和連結；做完時寫上日期。里程碑達成時在「里程碑」寫上日期，量到的（process 的數量、記憶體、Speedometer）記進那份 RFC 的 `progress.md`。

## 要開的 RFC

代號只在這份裡用，編號建立時再取。規模以 renderer 拆 process 那份（RFC 0004）為 1（`details.md` 的「規模的估法」）；S 是 0.25 以下，M 約 0.5，L 約 1，XL 是 1 到 1.5。

| 代號 | RFC | 範圍 | 規模 | 依賴 | 主要動到 | 里程碑 |
| --- | --- | --- | --- | --- | --- | --- |
| A：[RFC 0123](../0123-browser-survives-renderer/README.md)（2026-11-27 完成） | Browser 不被 renderer 弄倒 | IO thread 解開 renderer 送給 Viz 的訊息就檢查，說不通的是它的壞訊息；wgpu 的錯誤用 error scope 和 handler 接住；GPU 的記憶體有預算和上限；`browser`、`viz`、`net` 會 panic 的地方寫明不變式（lint）；不加 fuzzer（原本打算加、進 CI） | S–M | — | `viz`、`browser`、`compositor`、`net` | M1 |
| N：[RFC 0128](../0128-orb/README.md)（2026-11-28 完成） | 跨源的回應不交給不該拿的 renderer | ORB（Fetch 的 opaque response blocking：`no-cors` 的跨源回應照 MIME type 和內容的開頭擋掉 HTML、JSON、XML）；`Cross-Origin-Resource-Policy`；都在 network service 這一側（renderer 拿到的是擋過的） | M | — | `net`、`protocol` | M1 |
| F1：[RFC 0129](../0129-frame-ancestors/README.md)（2026-11-28 完成） | 不讓別人嵌：`X-Frame-Options` 和 CSP 的 `frame-ancestors` | Browser 在子 frame 的導覽 commit 之前檢查（Chromium 的 `AncestorThrottle`，兩個都在 Browser；原本記成「`frame-ancestors` Chromium 在 renderer 檢查」，那是「Compromised renderers」的文件的說法，2026-11-28 讀 `ancestor_throttle.cc` 確認 Chromium 現在在 Browser 的 `EvaluateFrameAncestors`）；不准的顯示錯誤頁。完整的 CSP 不在這份 | S–M | — | `browser` 的導覽、`net` 的 header | M1 |
| C：[RFC 0124](../0124-cross-site-process-swap/README.md)（2026-11-28 完成） | 跨網站的導覽換 process | Browser 的 `SiteInstance`、`BrowsingInstance`（先是一個分頁一個 group）；決定一次導覽要不要換 process（site 不同、錯誤頁、`about:blank` 和 `data:` 跟著發起者、`file:`）；在新的 process commit、舊的收掉（`pagehide`、`unload` 的時機）；session history、DevTools 的 session、WebDriver 的 session、automation 跨過換 process；renderer 啟動的時間（要不要先備一個） | L | — | `browser`（`web_contents`、`frame_tree`、`navigation_controller`、`render_process_host`）、`shell` 的 automation、`scripts/bench` | M2 |
| B：[RFC 0127](../0127-process-lock/README.md)（2026-11-28 完成） | process lock 和 Browser 的檢查 | process 在載入內容之前鎖到一個最上層的網站（`README.md` 的已決定的事項第 4 條）；每一種「要資料」的訊息照鎖檢查（commit 的 origin、loader 和 storage key 的最上層網站、cookie、blob URL 的 origin、導覽的發起者），不對的是壞訊息；測試用的「被攻破的 renderer」。請求一定帶發起者移到 N、`Sec-Fetch-*` 移到 FM（RFC 0127 的已決定的事項第 4、5 條） | S–M | C | `browser`、`net`、`protocol` | M2 |
| FM | Fetch Metadata：`Sec-Fetch-Site`、`Sec-Fetch-Mode`、`Sec-Fetch-Dest`、`Sec-Fetch-User` | Browser 這一側照請求的發起者、`mode`、`destination` 和使用者的啟動算（Chromium 的 `SetFetchMetadataHeaders`），導覽和子資源都送；renderer 送的 `sec-` header 已經擋了。WPT 的 `fetch/metadata/`。伺服器自願用來擋 CSRF 的訊號，不是 Browser 這一側的隔離，所以不在任何里程碑裡 | S–M | N（每個請求都帶發起者） | `net`、`browser` 的導覽 | — |
| F2 | iframe 的 `sandbox` | sandbox 的旗標（opaque 的 origin、不跑 script、不開彈出視窗、不送表單、不導覽最上層……）、旗標跟著 frame 存在 Browser 的 frame 樹；CSP 的 `sandbox` 不在這份。要不要把 sandbox 的 frame 另外隔離等 OOPIF | M | — | `dom_api`、`renderer`、`browser` 的 frame 樹 | M3 |
| W | `window.open` 和 `opener` | `window.open`、`opener`、具名的 browsing context、`noopener`；同一個 site 的彈出視窗和開它的頁面在同一個 process（同一個 `BrowsingInstance`）；跨網站的在別的 process、沒有 `opener`（要遠端的 window，等 O4）；`MessagePort` 的 pipe 跨 renderer；COOP（換 `BrowsingInstance`） | L | C（B 之後更好） | `dom_api`、`renderer`、`browser`、`shell` 的分頁 | M3 |
| G | `Origin-Agent-Cluster` 和 `document.domain` | 照 Chrome 現在的做法（`README.md` 的已決定的事項第 3 條）：讀 `Origin-Agent-Cluster`，文件的 agent cluster 照 origin（預設）或照 site（`?0`），同一個 browsing context group 裡同一個 origin 的 keying 一致；`window.originAgentCluster`；`document.domain` 的 getter 和 setter（只在照 site 的 agent cluster 裡生效）；跨 window 的檢查照 same origin-domain（設過的文件不能再用照 origin 的 security token）。WPT 的 `html/browsers/origin/origin-keyed-agent-clusters/` | S–M | C（process 照 site 分了之後） | `dom_api`、`dom_js`、`bindings`、`renderer` | M3 |
| X | cross-origin isolated | 頁面送 COOP 加 `Cross-Origin-Embedder-Policy`（`require-corp`、`credentialless`）時：沒有 CORP、CORS 同意的跨源資源擋掉（CORP 的 embedder policy 從 `unsafe-none` 換成文件的，iframe 的導覽也查）、report-only 和回報、跟著文件和 worker 走；`crossOriginIsolated`、`SharedArrayBuffer`、高精度的計時器；是不是 cross-origin isolated 進 `SiteInfo` 和鎖（Chromium 的 `WebExposedIsolationInfo`：isolated 的和沒 isolated 的不同 process，同一個網站也是）。網頁宣告之後安全地多給它能力，不是擋被攻破的 renderer，所以不是 M1、M2 的一部分（2026-11-28 開 N 時加的，RFC 0128 的第 4 題） | M | W（COOP 換 browsing context group）、N（CORP） | `browser` 的 `site_info`、`net`、`dom_api`、`bindings` | M3 |
| P | OOPIF 的原型（不合併） | 第二個 renderer 畫一個跨網站的 iframe、把它的 surface 嵌進父頁面，不管 input 和 script；量 O2 的難度、修正下面幾份的規模 | S | C | 原型 | M4 之前 |
| O1 | 跨 process 的 frame 樹 | Browser 的 frame 樹裡每個 frame 可以在不同的 renderer；子 frame 導覽到別的 site 時在另一個 process commit；其他 process 裡的代理 frame（Chromium 的 `RenderFrameProxy`、`RemoteFrame`），同步名字、origin、sandbox 的旗標；子 frame 的 process 不在了時那一格是 sad frame；鎖從最上層的網站改成 frame 的 site | L | C、B、F2、P | `browser`、`renderer`、`protocol` | M4 |
| O2 | 子 frame 的畫面 | 跨 process 的子 frame 是 local root：自己的 widget、compositor、frame sink；父頁面的 frame 裡在 iframe 的位置放嵌 surface 的 quad（子 frame 現在是畫進父 frame 的）；大小、device scale、可見、viewport 的交集跨 process 同步；`LocalSurfaceId` 的比對；看不見的跨網站 frame throttle | XL | O1 | `renderer`、`compositor`、`viz`、`paint` | M4 |
| O3 | input 和 hit test | 事件落在哪個 process 的 frame（Viz 或 Browser 的 hit test、座標換算）、分派；focus 和鍵盤跟著 focused frame；子 frame 捲不完的傳回父頁面；游標、滑鼠的 capture | XL | O2 | `browser`、`shell`、`compositor`、`viz` | M4 |
| O4 | 跨 process 的 script | 遠端的 window（`parent`、`top`、`frames[i]`、`length`、具名的查找）、`postMessage` 經過 Browser、對遠端的 frame 設 `location`；父頁面的 `load` 等別的 process 的子 frame；iframe 的 `onload`、lazy load；跨網站的彈出視窗的 `opener`（補上 W 留的） | L | O1 | `dom_api`、`dom_js`、`renderer`、`browser` | M4 |
| O5 | 工具和其餘 | DevTools 和 CDP 的 target（每個 OOPIF 一個，auto-attach）、WebDriver 切 frame、automation、截圖、history；量測工具照 process 找 renderer；WPT 的預期結果 | M–L | O1 到 O4 | `shell`、`driver`、`scripts`、`tests/wpt` | M4 |

合計：M1 約 1（A、N、F1），M2 約 1.5（C、B），M3 約 2.5（F2、W、G、X），M4 約 4 到 6（O1 到 O5）。

### 每一份開始時要定的（先記下來）

- **A**：wgpu 的錯誤怎麼對回是哪個 renderer 的 frame；Browser 的 panic 是逐一改掉，還是在訊息的入口接住（`catch_unwind`）當成壞訊息。
- **N**：ORB 照規範的哪一版（Chromium 現在的 ORB 和規範的差別）；要 sniff 多少。另外一定要做（RFC 0127 的已決定的事項第 4 條）：renderer 的請求不帶發起者是壞訊息（正常的 renderer 每個子資源都帶，`no-cors` 的也是；RFC 0127 寫成「`no-cors` 的不帶」是誤讀，2026-11-28 開 N 的草稿時更正），CORS 照 `mode` 決定、不再看有沒有發起者；否則被攻破的 renderer 拿掉發起者就躲過 ORB。
- **F1**：~~`frame-ancestors` 放 Browser 還是照 Chromium 放 renderer~~：不用選了，Chromium 現在也在 Browser（見上面 F1 那一列）。
- **C**：換不換 process 的規則（哪些導覽留在原來的 process）；同一個 site 的分頁共不共用 process；舊的文件的 `unload` 在哪裡跑、等不等；`beforeunload` 要不要在這份加；新的 renderer 要不要預先啟動；`--single-process` 和測試的 process 裡的分頁怎麼辦（不換）。
- **B**：`about:blank`、`data:`、`blob:`、`srcdoc`、錯誤頁、`file:` 各自怎麼鎖；jail 之外要不要 citadel 的檢查。
- **W**：跨網站的彈出視窗在 O4 之前沒有 `opener`，還是等 O4 再開放跨網站的 `window.open`；新視窗還是新分頁。新分頁的初始文件被開它的頁面碰過之後，網址列不再顯示還沒 commit 的網址（RFC 0125 的已決定的事項第 1 條，Chromium 的 `IsUnmodifiedBlankTab`）。**renderer 要能裝好幾個分頁**（同一個 site 的彈出視窗和開它的頁面同步存取對方，一定在同一個 process）：這是第一個非做不可的地方，做的時候把 RFC 0124 留到這時的兩件事重新考慮一次：(1) 同一個 site 的一般分頁要不要也共用 process、要不要設 process 數的上限（RFC 0124 的已決定的事項第 10 條現在是不共用）；(2) renderer 是 Browser 的 process 裡的 thread 時（`--single-process`、測試）跨網站的導覽怎麼換（RFC 0124 的已決定的事項第 11 條現在是換一組新的 thread）：renderer 能裝好幾個分頁之後，可以照 Chromium 的 `--single-process` 改成同一個 renderer 裡換一個新的 frame，Browser 這一側不變。另外補上 RFC 0124 留到這時的資料模型（已決定的事項第 17 條：現在只有 `RenderProcessHost` 上的 `SiteInfo`）：`BrowsingInstance` 和包著 `SiteInfo` 的 `SiteInstance`。(1) 參考 Chrome：不相干的分頁只有預設的搜尋引擎共用 process，一個 process 最多 2 個（`../0124-cross-site-process-swap/details.md` 的「預備的 renderer 和重用」）。
- **G**：設過 `document.domain` 的文件的 security token 怎麼辦（Blink 的做法要對原始碼）；OOPIF 之後照 site 的 agent cluster 和 origin 的鎖怎麼並存。
- **X**：`SharedArrayBuffer` 要不要照 Chrome 只給 cross-origin isolated 的頁面（machopo 現在完全沒有）；高精度的計時器現在的精度（RFC 0122 的 `details.md` 的「沒有查到的」）；`credentialless` 要不要一起做；鎖多一個維度時 `SiteInfo::of_document`、`admits`、`allows` 怎麼改（RFC 0127）。
- **O1 到 O5**：原型之後。**O2**：同一個 renderer 裡 main frame 的 paint holding 要不要改成 Browser 和 Viz 的 fallback、Browser 的計時器（RFC 0124 的已決定的事項第 19 條的 H3；現在是 Browser 判斷、renderer 暫時不交 frame，換 process 時才用 Viz 的 fallback，第 18 條）：子 frame 跨 process 時只能用 fallback（Chromium 對子 frame 只在同一個 origin 時留），不統一的話有三種情況。

## 里程碑

| 里程碑 | 做完之後 | 要的 RFC | 達成 |
| --- | --- | --- | --- |
| M1 | renderer 送什麼都不能讓 Browser 結束；`no-cors` 的跨源回應裡的 HTML、JSON 到不了 renderer；網站可以拒絕被嵌 | A、N、F1 | 2026-11-28（RFC 0123、0128、0129） |
| M2 | 同一個分頁導覽到別的網站時換 renderer；每個 renderer process 鎖到一個最上層的網站，要別的網站的第一方資料是壞訊息 | C、B | 2026-11-28（RFC 0124、0127） |
| M3 | iframe 的 `sandbox`；`window.open` 和同網站的 `opener`；`document.domain`（送 `Origin-Agent-Cluster: ?0` 的頁面）；cross-origin isolated 和 `SharedArrayBuffer` | F2、W、G、X | |
| M4 | 跨網站的 iframe 在自己的 process；鎖照 frame 的 site | P、O1 到 O5 | |

## 相依和可以同時做的

```
A ─┐
N ─┼─ M1      N ──► FM
F1 ┘
C ──► B ── M2
C ──► W ─┐
C ──► G ─┤
W、N ──► X ┤
F2 ──────┼─ M3
C、B、F2 ──► P ──► O1 ──► O2 ──► O3
                    └──► O4          ──► O5 ── M4
```

- A、N、F1 互不相依，主要動的 crate 不同（`viz` 和 `browser` 的訊息、`net`、`browser` 的導覽），可以在不同的機器上同時做；定了一份一份做（`README.md` 的第 5 條）。
- C 是關鍵路徑：B、W、OOPIF 都等它。C 和 A 都動 `browser` 的 `render_process_host.rs`、`web_contents.rs`，不要同時做。
- F2 和 C 可以同時做（F2 主要在 renderer 和 `dom_api`）。
- O2、O3 一定照順序；O4 在 O1 之後就能開始，可以和 O2、O3 同時做。

## 每個里程碑之後還擋不住的

寫給之後更新 `docs/limitations.md` 的人：

| 之後 | 還擋不住的 |
| --- | --- |
| M1 | process 裡還是裝著這個分頁走過的每個網站、每個跨網站的 iframe |
| M2 | 嵌的和被嵌的在同一個 process：被嵌的 iframe 被攻破時讀得到嵌它的頁面，反過來也是；不靠 cookie 的私密內容（照 IP 放行的內網頁面）被嵌進別的網站的 process 時讀得到（除非它用 `X-Frame-Options` 拒絕）；ORB 不管的型別（圖片、`application/octet-stream`） |
| M4 | 同一個 site 的不同 origin 在同一個 process；Chromium 自己列的缺口（`details.md` 的「鎖和檢查（P、C）」） |
