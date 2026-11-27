# RFC 0125：網址列只顯示已經 commit 的網址（頁面發起的導覽）

- 開始日期：2026-11-27
- 狀態：✅ 已完成：2026-11-27（Linux：Fedora 45）
- 進度和驗收：`progress.md`
- 細節：`details.md`

## 摘要

頁面自己發起的導覽（連結、`location`、表單），在新的文件 commit 之前，網址列照舊顯示現在的文件的網址；只有 Browser 發起的新導覽（網址列輸入、書籤）和新分頁的第一次導覽，載入期間就顯示要去的網址。照 Chromium 的 `NavigationControllerImpl::GetVisibleEntry`。

## 動機

- **網址列的偽造，現在就做得到**（`docs/limitations.md` 的「隔離」）：machopo 在每次 main frame 的導覽一開始就把分頁的網址換成要去的網址（`browser` 的 `WebContents::begin_navigation`），不管是誰發起的。頁面一直發起、取代導覽：

  ```js
  setInterval(() => { location = "https://bank.example/login"; }, 50);
  ```

  導覽永遠不 commit，網址列一直是 `https://bank.example/login`，畫面是這個頁面自己畫的（例如假的登入頁）。沒有時間上限，不用使用者做任何事，不用攻破 renderer。2026-11-27 用臨時的測試確認過（`details.md` 的「確認」）。
- Chromium 正是為了這個這樣規定（`GetVisibleEntry` 的註解：「Most renderer-initiated navigations should not show the pending entry, to prevent URL spoof attacks.」）。
- 路線圖（RFC 0122 的 `roadmap.md`）沒有一份管這件事，所以排在 RFC 0124 的第 1 段之前先做（2026-11-27 使用者決定）。

**目標**

- 頁面發起的導覽在 commit 之前不改變網址列和其他「顯示的網址」。
- 分頁的網址分成兩種、每個用的地方各自用對的那一種（「詳細設計」）。

**不在範圍內**

- 新文件畫出來之前留著舊的畫面（paint holding）的偽造：RFC 0124 的已決定的事項第 7 條和換 process 一起做。
- 網址列怎麼顯示網址（省略 scheme、標出 registrable domain、punycode）：另一件事。

要解決的 `docs/limitations.md` 的限制（做完時刪掉）：

- 「隔離」的「頁面自己發起的導覽，還沒 commit 時網址列就顯示它要去的網址」。

## 設計概要

- 在 `a.example` 點了到 `b.example` 的連結：載入期間網址列照舊是 `a.example`，B 的文件 commit 的那一刻換成 `b.example`。
- 網址列輸入 `b.example`、按書籤：載入期間就顯示 `b.example`（使用者自己要去的，不是偽造）。
- 新分頁（連結的 `target=_blank`、新開的分頁）的第一次導覽：載入期間就顯示要去的網址。
- 導覽停下、被取代、失敗而沒有 commit 東西時：顯示的網址回到現在的文件的。

## 詳細設計

`WebContents` 原本只有一個 `url`，一開始就換、commit 時再設一次。它換成：

- **最後 commit 的網址**（Chromium 的 `GetLastCommittedURL`）：main frame commit 時設（`DidCommitNavigation`、`DidCommitSameDocumentNavigation`），就是現在 commit 時設的那一個。
- **顯示的網址**（`visible_url`，`GetVisibleURL`）：不存，照第 1、3 條從 main frame 的導覽算。`WebContents` 只記最新的那一次 main frame 的導覽是不是「可以顯示的那種」（`shown_navigation`，它的編號）：Browser 發起的新導覽，或什麼都還沒顯示、或只顯示著初始的 entry 的分頁裡的新導覽；renderer 發起的（帶著 `begin_id`）、上一頁和下一頁、重新整理都不是。那一次導覽還在 frame 樹上（在路上，或送出 commit、renderer 還沒回報：`FrameTree::uncommitted_url`）時顯示它的網址，否則顯示最後 commit 的。
- **最新的導覽要去的網址**（`latest_navigation_url`）：`Page.frameStartedNavigating` 用。
- 分頁的第一次導覽之前（什麼都還沒 commit）：顯示的是那一次導覽的網址，最後 commit 的沒有。

用到的地方照第 2 條改（`shell` 的 `browser_view.rs`、`automation_session.rs`、`automation_cookies.rs`、`headless.rs`；machopodriver 的 `session.rs` 的 `current_url`）。原本的 `WebContents::url` 拿掉了，每個用到的地方都要選一種。

### 測試

`progress.md` 的「驗收」：`browser` 的 `tests/it/visible_url.rs`（renderer 另一端沒有東西，commit 由測試回報）；`shell` 的視窗、自動化的 pipe 和 WebDriver 的測試各一個。`Network.getCookies` 的最上層網址只靠審查（`docs/test-gaps.md`）。

### 分階段

一段。

## 缺點

- 點了連結之後，到新頁面 commit 之前，網址列看不出「正在去哪裡」（Chromium 也是；載入的狀態另外看得到）。

## 理由和其他做法

在「待決定」。

## 先例

- **Chromium**（`details.md` 的「Chromium」）：`GetVisibleEntry`、`GetNavigationEntryForTitle`、DevTools 的 target 的網址用 `GetVisibleURL`。

## 已決定的事項

1. **照 Chromium 的規則決定哪些導覽在 commit 之前顯示要去的網址（U1）**（2026-11-27，使用者決定）：只有 Browser 發起的新導覽（網址列輸入、命令列給的網址、DevTools 和 WebDriver 的 `Page.navigate`）和分頁的第一次導覽；其餘的（頁面發起的、上一頁和下一頁、重新整理）顯示最後 commit 的。Chromium 的第二種另外要「沒有別的頁面碰過新分頁的初始文件」（`IsUnmodifiedBlankTab`）：machopo 沒有 `window.open`，沒有頁面碰得到別的分頁，現在永遠成立；做 `window.open` 時補上。新分頁裡 Browser 發起的上一頁、下一頁（Chromium 的另一個例外，Ctrl+Back）machopo 沒有（不能複製分頁）。理由：Browser 發起的是使用者自己要去的，頁面偽造不了；上一頁、下一頁的網址來自頁面能用 `pushState` 填的 session history。不選一律只顯示最後 commit 的（U2：網址列輸入之後先跳回舊的網址，像是沒生效）。不選照樣顯示、加時間限制（U3：一直重新導覽的頁面每次重新計時）。
2. **用到網址的地方各自照 Chromium 用其中一種**（2026-11-27，使用者決定；`details.md` 的「Chromium」逐項查過）：

   | 地方 | 用哪一種 | Chromium |
   | --- | --- | --- |
   | 視窗的網址列 | 顯示的 | `GetVisibleURL` |
   | 分頁上的文字（頁面沒有標題時的網址） | 最後 commit 的；分頁的第一次導覽用顯示的 | `GetNavigationEntryForTitle` |
   | DevTools 的 target 的 `url`（`Target.getTargets`、`getTargetInfo`） | 顯示的 | `RenderFrameDevToolsAgentHost::GetURL` |
   | WebDriver 的 Get Current URL | 最後 commit 的：machopodriver 改讀 `Page.getNavigationHistory` 的目前那一筆（不用在頁面裡跑 script，頁面卡住時也拿得到），不再讀 target 的 `url` | chromedriver 跑 `document.URL`，錯誤頁時改用 `Page.getNavigationHistory` 的目前那一筆 |
   | `Page.frameStartedNavigating` | 那一次導覽的網址 | `page_handler.cc` 的 `pending_url` |
   | `Network.getCookies` 的最上層網址 | 最後 commit 的 | `ComputeCookieURLs`，分區用 commit 了的文件的 |
   | reftest 的結果的網址 | 最後 commit 的 | （machopo 自己的：它的說明本來就這樣寫） |

   知道的後果：自動化在頁面發起的導覽 commit 之前讀 target 的 `url`，拿到的是舊的網址（和 Chrome 一樣）；WebDriver 的本來就該是 commit 的。
3. **顯示的網址不另外存，每次從 main frame 現在的導覽算出來（R1）**（2026-11-27，使用者決定）：main frame 有導覽在路上（包括 commit 已經送出、renderer 還沒回報的那一段，frame 樹記著正在 commit 的那一筆）、而且它是第 1 條說可以顯示的那種，就是它的網址；否則是最後 commit 的。導覽不管怎麼結束（被取代、停下、renderer 不在了時被清掉），一離開 frame 樹顯示的網址就回去。理由：沒有「漏了在哪裡設回去」的可能（現在停下之後就留著沒去成的網址，正是這樣來的）。Chromium 存一個 pending entry、在停下和失敗等地方丟掉（`DiscardNonCommittedEntries`），結果一樣。不選存一個欄位、在每個導覽結束的地方設回去（R2：改動小，漏一處就顯示錯的網址，多半沒有測試發現）。renderer 在載入期間不在了時那一次導覽會不會照樣 commit，實作時寫測試確認：R1 算出的只看它還在不在 frame 樹上。

## 待決定

無。

## 之後的可能

- 網址列的顯示（省略 scheme、標出 registrable domain、punycode 的同形字）。
