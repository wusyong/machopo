# RFC 0125 的細節：網址列只顯示已經 commit 的網址（頁面發起的導覽）

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

## 盤點

2026-11-27 讀的（`main`，`4c98f19d`）。

### 分頁的網址

- `browser` 的 `WebContents` 有一個 `url`（「The URL to show: the latest navigation's while it loads, then the committed one」）。設它的地方：
  - `begin_navigation`：main frame 的每一次導覽一開始（Browser 發起的、頁面發起的、上一頁和下一頁、重新整理都一樣）；
  - `DidCommitNavigation`、`DidCommitSameDocumentNavigation`：main frame commit 時。
- 停下（`stop_loading`）、被另一次導覽取代、被丟掉時都不設回去。載入失敗會 commit 錯誤頁（網址是要去的網址），所以失敗時顯示的是要去的網址。
- frame 樹的 root 另外記著 commit 的網址（`FrameTree::url`）。

### 用到 `WebContents::url` 的地方

| 地方 | 做什麼 |
| --- | --- |
| `shell` 的 `browser_view.rs` 的 `ui_state` | 視窗的網址列 |
| `shell` 的 `browser_view.rs` 的 `tab_title` | 頁面沒有標題時，分頁上顯示網址 |
| `shell` 的 `automation_session.rs` 的 `target_info` | DevTools 的 `Target.getTargets`、`getTargetInfo` 的 `url`；machopodriver 的 Get Current URL 讀它（`crates/driver` 的 `session.rs`） |
| `shell` 的 `automation_session.rs`（`Page.frameStartedNavigating`） | 剛開始的那一次導覽要去的網址 |
| `shell` 的 `automation_cookies.rs` | `Network.getCookies` 沒給網址時的最上層網址 |
| `shell` 的 `headless.rs` | reftest 的結果的「顯示的網址」（「the one committed, or the one a link to a fragment changed it to」） |

### 確認

2026-11-27，臨時加在 `crates/browser/tests/it/navigation.rs` 的測試（試完還原）：頁面 `/` 每 50 ms 執行 `location = '/victim'`，`/victim` 的回應延遲 5 秒。3 秒內每 100 ms 取一次：`WebContents::url()` 一直是 `/victim`，frame 樹的 root 的網址一直是 `/`、標題一直是頁面 `/` 的；沒有 commit。同一個 origin 的網址，但做法和跨網站的一樣。

## Chromium

2026-11-27 讀的原始碼（`main`）。

- **顯示的 entry**（`content/browser/renderer_host/navigation_controller_impl.cc` 的 `GetVisibleEntry`）：

  > The pending entry is safe to return for new (non-history), browser-initiated navigations. Most renderer-initiated navigations should not show the pending entry, to prevent URL spoof attacks.
  >
  > We make an exception for renderer-initiated navigations in new tabs, as long as no other page has tried to access the initial empty document in the new tab. If another page modifies this blank page, a URL spoof is possible, so we must stop showing the pending entry.

  條件：有 pending entry、它是新的（`pending_entry_index_ == -1`，不是上一頁、下一頁、重新整理），而且是 Browser 發起的、或是「沒被動過的空白分頁」（`IsUnmodifiedBlankTab`：第一次導覽、最後 commit 的是初始的 entry、沒有別的頁面碰過初始的文件）。另外，新分頁裡 Browser 發起的上一頁、下一頁（例如 Ctrl+Back）也顯示：「no existing page is visible and no one can script the new tab before it commits」。其餘的顯示最後 commit 的。
- **顯示的網址**（`content/browser/web_contents/web_contents_impl.cc` 的 `GetVisibleURL`）：`GetVisibleEntry` 的網址。
- **分頁的標題**（同一個檔案的 `GetNavigationEntryForTitle`）：用最後 commit 的 entry：「when the user types in a URL, we want to keep the old page's title until the new load has committed」；只有第一次導覽、pending 的有標題或是上一頁、下一頁時用顯示的那一個。
- **DevTools 的 target 的網址**（`content/browser/devtools/render_frame_devtools_agent_host.cc` 的 `GetURL`）：main frame 的是 `web_contents->GetVisibleURL()`；子 frame 的是那個 frame 最後 commit 的。
- **`Page.frameStartedNavigating` 的 `url`**（`content/browser/devtools/protocol/page_handler.cc`）：那一次導覽的網址（`navigation_request->common_params().url`，註解叫它 `pending_url`），不是顯示的、也不是 commit 的。
- **`Network.getCookies`**（`content/browser/devtools/protocol/network_handler.cc` 的 `GetCookies`、`ComputeCookieURLs`）：沒給網址時，從 session 的 frame 往下，每個 frame 的 `GetLastCommittedURL()`；分區和 site for cookies 不管有沒有給網址，都用那個 frame（commit 了的文件的）的 `GetNetworkIsolationKey()`、`ComputeSiteForCookies()`。
- **chromedriver 的 Get Current URL**（`chrome/test/chromedriver/window_commands.cc` 的 `ExecuteGetCurrentUrl`）：在最上層的 frame 跑 `function() { return document.URL; }`；拿到的是錯誤頁的內部網址時，改用 `Page.getNavigationHistory` 的目前那一筆的網址（`chrome/test/chromedriver/chrome/web_view_impl.cc` 的 `WebViewImpl::GetUrl`）。都是 commit 了的文件的網址，不是 DevTools 的 target 的（顯示的）。
- 沒有查：Stop 時 pending entry 怎麼丟（`WebContentsImpl::Stop` 停下每個 frame 樹的載入，`DiscardNonCommittedEntries` 在 `navigation_controller_impl.cc` 裡有好幾處，沒有逐一讀）。

## 參考資料

- Chromium 的原始碼（`main`，2026-11-27）：`content/browser/renderer_host/navigation_controller_impl.cc`、`content/browser/web_contents/web_contents_impl.cc`、`content/browser/devtools/render_frame_devtools_agent_host.cc`。
