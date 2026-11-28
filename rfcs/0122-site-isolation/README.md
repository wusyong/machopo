# RFC 0122：site isolation 的路線圖

- 開始日期：2026-11-27
- 狀態：👍 已接受：2026-11-27（路線圖層級的五條定了；下一步是開 A 那一份）
- 路線圖：`roadmap.md`
- 細節：`details.md`

## 摘要

renderer 已經是自己的 process、關在作業系統的 sandbox 裡，但一個分頁從頭到尾用同一個 renderer：使用者在同一個分頁走過的每個網站、頁面裡每個跨網站的 iframe，都在同一個 process 裡，Browser 也不知道「這個 process 是哪個網站的」。這份是路線圖，不直接實作：把「一個 process 只有一個網站的東西，而且 Browser 強制」拆成幾份 RFC、排順序、估規模，每一份開始時再各自決定設計。分四個里程碑：(1) renderer 弄不倒 Browser、讀不到別的網站的回應；(2) 一個 process 一個最上層的網站，Browser 照它鎖；(3) frame 的政策、彈出視窗和 `document.domain`；(4) 跨網站的 iframe 在自己的 process（OOPIF），完整的 site isolation。規模以 renderer 拆 process 那份為 1：前兩個里程碑合計約 2.5，第三個約 2，OOPIF 另外 4 到 6（`roadmap.md`）。

## 動機

現況（2026-11-27 的 `main`，`ecac4005`；`details.md` 的「machopo 的現況（盤點）」）：

- 一個分頁一個 renderer process，只在它不在了之後才換。同一個分頁從 A 導覽到 B，B 和 A 留下的東西在同一個 process。
- 跨網站的 iframe 和嵌它的頁面在同一個 renderer，只靠 V8 的 security token 隔開。
- Browser 對 renderer 的檢查是「照文件」的（每份文件的 loader 帶著它的 origin）：擋得住 renderer 用 A 文件的 loader 要 B 的 cookie，擋不住它拿著 B 文件的 loader（B 是它 process 裡的 iframe，或上一個網站）。
- `no-cors` 的跨源回應（`<img src>` 指到別的網站的 JSON）整個交給 renderer：沒有 ORB。
- renderer 送的 frame 能讓 wgpu 的驗證錯誤結束整個程式；Browser 處理訊息的程式有會 panic 的地方。

這些的後果：renderer 被攻破時（或用 Spectre 這類旁路讀自己 process 的記憶體時），拿得到的不只是它正在顯示的那個網站的資料；而且不用逃出 sandbox，讓 Browser 結束就好。這幾份 RFC 收緊的 sandbox 擋的是 renderer 碰作業系統，擋不了這兩件。

**目標**（整條路線圖的）：

1. renderer 送什麼都不能讓 Browser 結束：壞的訊息、壞的 frame 只結束那個 renderer。
2. renderer 拿不到它不該有的跨源回應的內容（ORB、`Cross-Origin-Resource-Policy`）。
3. 一個 renderer process 只屬於一個網站，Browser 在每一則要資料的訊息上照它檢查（Chromium 的 process lock 和 jail 的檢查）。
4. 跨網站的 iframe 在自己的 process 裡：嵌的和被嵌的互相讀不到（Chromium 桌面版的 site-per-process）。
5. 每個里程碑做完時，行為和 Chrome 對得上的部分用 WPT 守著，量得到的（process 的數量、記憶體、Speedometer）記下來。

**不在範圍內**：

- GPU process、network service 拆成自己的 process：另外的路線。
- V8 的 sandbox。
- Android 那種「只隔離一部分網站」的模式、process 數量的上限和重用的策略的調校（先做最簡單的，量到問題再說）。
- extension、WebUI 的隔離（machopo 沒有）。

要解決的 `docs/limitations.md` 裡的限制（各份 RFC 做完時刪）：「隔離」的「renderer 還有兩條路能讓 Browser 結束」、「名稱表和屬性值的表在同一個 renderer 裡共用」；「iframe」的「跨網站的 iframe 和頁面在同一個 renderer」；`MessagePort` 不能跨 renderer、沒有 `window.open`、iframe 沒有 `sandbox` 的那幾條。

## 設計概要

給 machopo 的使用者：

- 里程碑 2 之後，在同一個分頁從一個網站走到另一個網站時，背後換了一個 renderer process；一個網站的 renderer 當掉或被攻破，碰不到別的網站的 cookie 和 storage。
- 里程碑 4 之後，頁面裡跨網站的 iframe（廣告、嵌入的影片、登入的元件）在自己的 process，它當掉只有那一格變成 sad frame。
- process 會變多，記憶體跟著變多。

給改 machopo 的人：

- Browser 多一層「這個 process 是誰的」（Chromium 的 `SiteInstance`、`BrowsingInstance`、`ProcessLock`）：新的「renderer 向 Browser 要資料」的訊息，都要說它照什麼檢查。
- 分頁和 renderer process 不再是一對一：先是「一個分頁先後有好幾個」，之後是「一個分頁同時有好幾個」「好幾個分頁共用一個」。照 pid 找 renderer 的工具（`bench.py`、trace、census）、DevTools 的 target、WebDriver 的 session 都要跟著。

## 詳細設計

這份只定路線圖層級的事：隔離的單位、OOPIF 之前鎖什麼、順序。每一份 RFC 的設計在它自己開始時定。每一節最後是選了哪個（理由在「已決定的事項」）。

### 隔離的單位

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| U1 | **照 site**（scheme 加 registrable domain；machopo 已經有 `site` crate） | 和 Chromium 一樣；同一個 site 的不同子網域在同一個 process，process 少 | 同一個 site 底下的子網域（`a.example.com` 和 `b.example.com`）互相不隔離 |
| U2 | 照 origin | 最緊 | process 多；Chromium 只當成選項（`--isolate-origins`） |

Chromium 用 site 的理由是 `document.domain`：同一個 site 的不同 origin 改了它之後可以同步地互相存取，所以一定要在同一個 process。machopo 現在沒有 `document.domain`：

| | 做法 | 說明 |
| --- | --- | --- |
| D1 | **不支援 `document.domain` 的 setter**（設了沒有效果，照 Chrome 現在的預設；不做 `Origin-Agent-Cluster: ?0` 的退出） | agent cluster 一律照 origin；之後要把鎖收到 origin 也沒有障礙；靠它的舊網站不能用 |
| D2 | 照 Chrome 現在的：預設沒有效果，頁面送 `Origin-Agent-Cluster: ?0` 時生效 | 相容性和 Chrome 一樣；process 的單位就只能是 site，而且要做 site-keyed 的 agent cluster |

定了 **U1**（已決定的事項第 2 條）和 **D2**（第 3 條：照 Chrome，獨立的一份，排在 M3）。

### OOPIF 之前鎖什麼

Chromium 的 process lock 是在 process 載入任何內容之前定的，之後不變。跨網站的 iframe 還在嵌它的頁面的 process 裡時（里程碑 4 之前），一個 process 裡不只一個 site，要選：

| | 做法 | 好處 | 代價 |
| --- | --- | --- | --- |
| L1 | **照最上層的網站鎖**：這個 process 只拿得到「最上層是 X」的資料：X 自己的第一方資料，和嵌在 X 底下的其他網站的第三方資料（分區的、沒有 cookie 的） | 被攻破的 renderer 拿不到別的網站的第一方資料（它的 cookie、它的 `localStorage`）：machopo 本來就不送第三方的 cookie，storage 和 HTTP cache 都照最上層的網站分區（`details.md` 的「資料照什麼分」），所以把別的網站嵌進 iframe 只拿得到沒登入的內容。跨網站的 iframe 照常能用 | 嵌的和被嵌的在同一個 process：被嵌的（廣告）被攻破時讀得到嵌它的頁面；不靠 cookie 的私密內容（內網照 IP 放行的頁面）被嵌進來時讀得到。這兩件要等 OOPIF |
| L2 | 有跨網站 iframe 的 process 不鎖（Chromium 部分隔離時的「unlocked process」） | 和 Chromium 的過渡期一樣 | 被攻破的 renderer 加一個 iframe 就讓自己變成不鎖的，鎖等於沒有 |
| L3 | 照 frame 的 site 鎖，跨網站的 iframe 不載入（或沒有資料） | 鎖最單純 | 跨網站的 iframe 全壞，直到 OOPIF |

定了 **L1**（已決定的事項第 4 條）。它靠的前提是 machopo 已經有的（第三方 cookie 一律擋、storage 分區）；這也表示之後如果要加「允許第三方 cookie」的開關，要等 OOPIF。Chromium 沒有這個中間形態（它的資料當時沒有分區）。

### 順序

重新評估（2026-11-27）之後和先前口頭的建議不同的地方：

- **換 process 要在鎖之前**：鎖是 process 一輩子不變的，一個分頁先後裝好幾個網站的 process 鎖不了。所以先「跨網站的導覽換 process」，才有東西可以鎖。
- **ORB 是缺的一塊**，而且和 process 模型無關、現在就能做：沒有它，鎖住的 process 照樣用 `<img>` 把別的網站的 JSON 要進自己的記憶體。
- **`X-Frame-Options`（和 `frame-ancestors`）排前面**：在 L1 底下，「不讓別的網站把我嵌進它的 process」正是它管的事。
- **子 frame 現在沒有自己的 frame sink**（畫進父 frame）：OOPIF 的畫面那一份要從 renderer 送出嵌 surface 的 quad 開始，不是只接上現成的。
- **`window.open` 的跨網站 `opener` 要 OOPIF 的代理**（遠端的 window）：在那之前，跨網站的彈出視窗只能沒有 `opener`。

| 里程碑 | 做完之後 | 內容（代號見 `roadmap.md`） |
| --- | --- | --- |
| M1 | renderer 弄不倒 Browser，讀不到別的網站的回應 | A（Browser 的防禦）、N（ORB 和 CORP）、F1（`X-Frame-Options`、`frame-ancestors`）：三份互不相依；A 做完之後先做 M2 的 C、B，再做 N、F1（第 6 條） |
| M2 | 一個 process 一個最上層的網站，Browser 照它檢查 | C（`SiteInstance`、`BrowsingInstance`，跨網站的導覽換 process）→ B（process lock 和 jail 的檢查） |
| M3 | frame 的政策、彈出視窗、`document.domain` | F2（iframe 的 `sandbox`）、W（`window.open`、`opener`、跨 renderer 的 `MessagePort`、COOP）、G（`Origin-Agent-Cluster` 和 `document.domain`） |
| M4 | 完整的 site isolation | P（原型）→ O1 到 O5（OOPIF） |

M1 和 M2 一定做，M4 做不做、什麼時候做，在 M2 做完、原型量過之後再決定（已決定的事項第 1 條）；M3 的兩份和隔離沒有先後的硬性關係（W 的資料模型在 C 就要留位置）。

### 每個里程碑怎麼驗

- **M1**：fuzzer 餵 Browser 壞的訊息和壞的 frame，Browser 不結束（放進每週的 CI）；WPT 的 `fetch/orb/`、`fetch/cross-origin-resource-policy/`、`x-frame-options/`、`content-security-policy/frame-ancestors/`。
- **M2**：從外面看 process：同一個分頁導覽到別的網站之後 renderer 的 pid 變了、舊的結束了；一個「被攻破的 renderer」（測試用的假 renderer，`RendererLauncher::Detached` 或能送任意訊息的 child）要別的網站的 cookie、storage、帶別的 origin 的 commit，都是壞訊息；session history、DevTools 的 session、WebDriver 的 session 跨過換 process 還在。
- **M4**：WPT 裡靠跨源 iframe 的測試（`html/browsers/`、`html/semantics/embedded-content/the-iframe-element/`、`cross-origin-*`）；情境和 reftest 的截圖合成好幾個 process 的 surface。
- 每份 RFC：mutation test、量測工具的測試（process 的對應變了）。

### 分階段

路線圖本身：

0. 草稿和盤點（這一版）。
1. 定路線圖層級的事（「待決定」）。
2. 照 `roadmap.md` 開各份 RFC；每開一份，把表裡的代號換成編號和連結，做完寫上日期。里程碑達成時寫上日期。
3. M2 達成時重新評估 M4（原型、規模）。

## 缺點

- process 變多：M2 之後數量和現在差不多（一個分頁同時還是一個），M4 之後每個跨網站的 iframe 多一個；記憶體跟著多。
- 換 process 的導覽比同一個 process 的慢（要啟動 renderer；V8、字型的清單都重來）。
- 「分頁對 renderer 一對一」的假設散在 Browser、工具和測試裡，拆掉它是這條路線圖最大的隱性成本。
- L1 是 Chromium 沒有的中間形態：它的保證要自己論證、自己測，而且綁著「不送第三方 cookie」。
- OOPIF 很大，做到一半（O1、O2 做完，O3 還沒）的狀態不能預設打開，要有開關。

## 理由和其他做法

- **不做的話**：sandbox 再緊，被攻破的 renderer 還是拿得到這個分頁走過的每個網站的東西，還能讓 Browser 結束。
- **直接做 OOPIF**：Chromium 的終點，但前面的東西（process 屬於誰、鎖、換 process）它都要；先做 M1、M2 就已經擋住「拿別的網站的第一方資料」。
- **只做 M1**：最小，renderer 弄不倒 Browser、讀不到跨源的回應；但 process 還是裝著好幾個網站。
- 各節的選項比較在上面。

## 先例

`details.md` 的「Chromium」「其他瀏覽器」：

- **Chromium**：桌面版完整的 site isolation（每個 process 鎖一個 site，OOPIF），Android 只隔離一部分，WebView 和 iOS 沒有；`SiteInstance`、`BrowsingInstance`、process lock、jail 和 citadel 的檢查、ORB。
- **Firefox**：Fission，桌面版預設開啟（照記憶）。
- **Safari**：還在做。

## 已決定的事項

1. **範圍：M1、M2 一定做，M3 照需要，M4 在 M2 做完、原型量過之後再決定（R1）**（2026-11-27，使用者決定）：這份 RFC 在 M2 達成時算完成，之後的（M3、M4）另開路線圖或各自的 RFC。理由：M1、M2 的投入和收益比最好（renderer 弄不倒 Browser、讀不到跨源的回應、拿不到別的網站的第一方資料）；OOPIF 是 4 到 6 倍、估計很不準的工作，等原型有數字再承諾。不選一路做到 M4（R2：目標明確，但現在就承諾最大、最不準的那一塊）。不選只做 M1（R3：process 裡還是裝著好幾個網站）。
2. **process 照 site 分（U1）**（2026-11-27，使用者決定）：scheme 加 registrable domain（`site` crate 的 `Site`）。理由：和 Chromium 一樣；同一個 site 的子網域共用 process，process 少。代價：同一個 site 底下不同的 origin 互相不隔離。不選照 origin（U2：process 多很多，Chromium 只當成選項）。`document.domain` 另外定。
3. **`document.domain` 照 Chrome 現在的做法（D2），當成獨立的一份、排在 M3**（2026-11-27，使用者決定）：預設 agent cluster 照 origin、setter 沒有效果，頁面送 `Origin-Agent-Cluster: ?0` 時照 site、setter 生效；`window.originAgentCluster`。路線圖的代號 G，M2 之後做，不擋 M1、M2；做完之前沒有 `document.domain`（現在的樣子）。理由：選了 U1（第 2 條）之後它對 process 模型沒有多的要求（同一個 site 本來就在同一個 process），之後加不用返工；machopo 的 UA 和 Chrome 一樣、行為照 Chrome，靠它的舊網站在 Chrome 上送那個 header 就能用。排在 M3 的理由：它不在關鍵路徑上；它是在執行中放寬同源政策（跨 window 的檢查從「同 origin」變成「same origin-domain」，V8 的 security token 不能再照 origin 固定），是安全敏感的程式，等 process 照 site 分了之後、放寬的範圍被 process 的邊界框住再做。不選不支援（D1：最簡單，和 Chrome 不一致；原本的看法，理由「之後把鎖收到 origin 沒有障礙」不成立：做了 D2，預設的文件還是 origin-keyed）。不選現在就做（在 process 照 site 分之前先放寬）。
4. **OOPIF 之前，process 鎖到最上層的網站（L1）**（2026-11-27，使用者決定）：一個 renderer process 只拿得到「最上層是 X」的資料：X 的第一方資料，和嵌在 X 底下的其他網站的第三方資料（分區的、沒有 cookie 的）。檢查是 Browser 已經算好的東西的比較：每份文件的 loader 的 `IsolationInfo` 和每個 `StorageKey` 的最上層網站，是不是這個 process 鎖的那個。理由：成本小（要比的 Browser 都有了），到 OOPIF 時只換比較的規則（最上層的網站換成 frame 的 site），鎖放在哪、在哪些訊息上檢查、壞訊息、測試用的「被攻破的 renderer」都沿用；M4 做不做還沒定（第 1 條），這個形態可能很久；跨網站的導覽換 process 那一份（C）重寫「這次導覽放進哪個 process」，鎖是它的安全網（放錯 process 時在 commit 擋下來）；補現有照文件的檢查的洞（`RegisterBlobUrl` 的 origin 沒有對照），之後新加的訊息漏了檢查時有一層接著。它靠的前提是 machopo 已經有的：第三方的 cookie 一律不送，storage 和 HTTP cache 照最上層的網站分區；之後要加「允許第三方 cookie」的開關要等 OOPIF。代價：嵌的和被嵌的在同一個 process，互相讀得到；不靠 cookie 的私密內容被嵌進來時讀得到；Chromium 沒有這個形態，保證要自己論證和測。老實說它多的是第二層：C 做完之後，現有照文件的檢查已經擋住大部分（renderer 只拿著自己 process 裡的文件的 loader，假的 commit 會被抓到）。不選不鎖到 OOPIF（L2：最簡單，Chromium 的過渡期也是，但那段時間沒有 process 層級的不變式）。不選照 frame 的 site 鎖、跨網站的 iframe 不載入（L3：iframe 全壞）。
5. **M1 的三份一份一份做：A、N、F1，然後 M2 的 C（S1）**（2026-11-27，使用者決定）：理由：每份單獨審、單獨 merge；A 最小，而且它整理的是 C 要大改的那幾個檔案（`render_process_host.rs`、`web_contents.rs`）；N、F1 和 C 幾乎不重疊，順序之後可以調。不選分到不同的機器同時做（S2：最快，但要協調，A 和 C 改同一批檔案）。不選先做 N（S3：以現在的風險看 ORB 更直接，不用先攻破 renderer 就能把跨源的資料要進它的記憶體；A 小、很快，所以還是 A 先，要對調也可以）。
6. **A 之後先做 C、B，再做 N、F1（S2）**（2026-11-27，使用者決定；改了第 5 條的順序）：做完 A、開始 N 的盤點時找到的：machopo 從不送、也不收第三方的 cookie（`net` 的 `cookie_jar.rs` 的 `accessible`：只有第一方的和 partitioned 的），HTTP cache 照最上層和 frame 的網站分開，所以網站 A 的頁面用 `<img>`、`<script>` 要網站 B 的資源時，B 給的是沒有登入的使用者的東西；ORB 和 CORP 剩下擋的是靠網路的位置認證的資源（內網、`localhost`）和以後放寬第三方 cookie 時；被嵌在 A 裡的 B 也沒有 cookie，`X-Frame-Options` 擋的 clickjacking 大多也做不成。反過來，C 擋的洞現在就在、cookie 的政策也擋不住：一個分頁從頭到尾用同一個 renderer，攻破它的網頁 A 之後，這個分頁導覽到 B（使用者點的，或 A 設 `location`），B 是最上層、cookie 照送，它的資料就在被攻破的 process 裡。B 建在 C 上。代價：C 比 N 大很多，M1（「讀不到別的網站的回應」）晚一點才達成。不選照原本的 A、N、F1、C、B（S1）。

## 待決定

路線圖層級的都定了（已決定的事項第 1 到 6 條；路線圖在這份的 `roadmap.md` 維護、這份到 M2 達成時算完成，是第 1 條的一部分）。

各份 RFC 開始時才定的，列在 `roadmap.md` 的「每一份開始時要定的（先記下來）」。

## 之後的可能

- 鎖收到 origin（`--isolate-origins` 那一類）。
- cross-origin isolated（COOP 加 COEP）和 `SharedArrayBuffer`：2026-11-28 排進 `roadmap.md`，代號 X，放在 M3。
- process 的上限和重用（同一個 site 的 process 跨分頁共用）。
- GPU process、network service 的 process。
