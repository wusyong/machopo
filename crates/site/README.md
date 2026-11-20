# site

對應 Chromium 的 `//net/base` 裡 browser 和 renderer 兩邊都連結的那一部分：一份資料是「哪個網站的」。Browser、network service 和 renderer 都能用它；public suffix list（`psl` crate）只在這裡。

## `registrable_domain`

host 的 registrable domain（public suffix 加一個 label，eTLD+1），照 Chromium 的 `GetDomainAndRegistry` 加 `INCLUDE_PRIVATE_REGISTRIES`：

```rust
use url::Host;
site::registrable_domain(&Host::Domain("a.b.example.co.uk")); // Some("example.co.uk")
site::registrable_domain(&Host::Domain("x.alice.github.io")); // Some("alice.github.io")
site::registrable_domain(&Host::Domain("a.b.test"));          // Some("b.test")
site::registrable_domain(&Host::Domain("co.uk"));             // None
```

- 清單的 private 那一段也算（`github.io` 底下每個使用者各一個）；清單不認得的頂層網域照樣當後綴。
- 沒有的：IP 位址、public suffix 本身、只有一個 label 的名稱（`localhost`）、有空的 label 的名稱。名稱最後的點留著。
- host 要是網址裡的樣子：小寫的 ASCII，國際化的名稱是 punycode。`psl` 不轉小寫，也會把 IP 位址的數字當成 label，所以收 `url::Host`。
- 更新清單：`cargo update -p psl`（清單一變它就發版）。

## `Site`

HTML 的「obtain a site」，Chromium 的 `net::SchemefulSite`：「same site」比的東西。

```rust
let site = site::Site::of(&url.origin()); // 或 Site::of_url(&url)
site.serialize();                         // "https://example.com"，opaque 的是 "null"
```

- tuple origin 的網站是 scheme 加 registrable domain；沒有 registrable domain 的（IP 位址、`localhost`、public suffix 本身）是 scheme 加 host。不看 port。
- opaque 的 origin 的網站就是那個 origin：只和自己相等。`serialize()` 分不出兩個 opaque 的網站，不能拿來比。
- `url` crate 只給 http、https、ftp、ws、wss 的網址 tuple origin，正好是 Chromium 會查 registrable domain 的那幾種 scheme；`file:` 的 origin 是 opaque 的。WebSocket 的 scheme 沒有換成 http、https（Chromium 在算網站之前先換）：machopo 還沒有 WebSocket。

## 測試

`cargo test -p site`：registrable domain（萬用字元和例外、punycode、private 的後綴、不認得的頂層網域、不是名稱的），網站（同一個 registrable domain 的 host、scheme、port、沒有 registrable domain 的 host、opaque 的 origin）。
