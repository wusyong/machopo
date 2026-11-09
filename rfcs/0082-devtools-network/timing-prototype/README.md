# 時間記到哪個請求：原型

2026-10-23 做的原型，不合併。要回答 `../README.md` 的「已決定的事項」第 5 條留下的問題：DNS 和建立連線是 hyper-util 的連線池叫的，不是請求自己叫的，用 tokio 的 task-local 包住一個請求的 `send`，記到的時間是不是這個請求自己開的連線的。

`src/main.rs` 用和 machopo 同版的 reqwest 0.13.5、hyper-util 0.1.21：
- 自訂的 resolver（`dns_resolver`）量 DNS；
- 一層 `connector_layer` 量建立連線（含 TLS），可以故意讓它變慢；
- 兩者都寫進當時那個 task 的 task-local；
- 只有記到建立連線的請求才算數：只記到 DNS 的是開了一條、最後沒用上的連線。

伺服器（hyper，HTTP/1.1 keep-alive、HTTP/2 cleartext、HTTPS 由 ALPN 選）在回應裡寫明是哪一條連線（`x-conn`）、這條連線的第幾個請求（`x-nth`）。

```sh
CARGO_TARGET_DIR=../../../target/prototypes cargo run
```

HTTPS 用 `../../../../third_party/wpt/tools/certs` 的 `web-platform.test` 憑證，resolver 把 `web-platform.test` 解析成 `localhost`。

## 結果（macOS，Apple M4，連跑 20 次都一樣）

| 情況 | 結果 |
| --- | --- |
| HTTP/1.1 依序三個 | 一條連線；只有第一個算數，後兩個沒有 DNS 和連線（重用） |
| HTTP/1.1 同時五個（伺服器慢 100 ms） | 五條連線；五個都算數，各是自己那條的第一個請求 |
| HTTP/1.1 競爭：建立連線要 300 ms，`a` 開第 1 條、馬上答完；`b` 晚 150 ms 開第 2 條，但第 1 條先空出來 | `b` 走第 1 條（第 2 個請求），只記到 DNS、沒記到連線，不算數；第 2 條在背景開完，沒有記到任何請求上 |
| HTTP/2 cleartext 同時五個，再一個 | 一條連線；剛好一個算數，最後那個不算 |
| HTTPS（ALPN 選 HTTP/2）同時五個，再一個 | 連線池一開始不知道是 HTTP/2，五個各開一條（五條 TLS 連線）；全部走同一條，只有開了它的那個算數，其他四條在背景開完、沒記到請求上；最後那個不算 |

結論：task-local 記得到對的請求，包括 HTTP/2 和競爭的情況；「只記到 DNS 不算」這條規則是必要的（競爭的情況裡 `b` 記到了 DNS）。另外看到的：HTTPS 的 HTTP/2 同時開了五條連線、只用一條，是 machopo 現在就有的行為（Chrome 的 `SpdySessionPool` 會等第一條），和這份無關，記在 RFC 的「之後的可能」。
