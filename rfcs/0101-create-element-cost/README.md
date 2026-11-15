# RFC 0101：`createElement` 的成本往 Chrome 靠

- 開始日期：2026-11-15
- 狀態：✅ 已完成：2026-11-15
- 細節：`measurements.md`（開始前的量測和原型）；結果在 `progress.md`

## 摘要

建一個元素，machopo 在 main thread 上比 Chrome for Testing 多約 750 條指令（約 1990 對 1234），blink_perf 的 `bindings/create-element` 慢 1.42 倍，三個 `gc-*` 頁（也是 `createElement` 的迴圈）慢 1.05 到 1.37 倍。GC 不是原因：machopo 的 GC 比 Chrome 少。這份分四段把一次呼叫的成本往下拿：拿掉多餘的檢查、元素的名稱用 V8 字串的身分查、建節點少做事、屬性的名稱也用身分查。

做完時（2026-11-15）：`createElement` 每次 2048 → 1292 條指令、448 → 272 cycles，callback 本身 1658 → 945 條；`create-element` 對 Chrome for Testing 1.42 → 0.86 到 0.88 倍，`gc-mini-tree`、`gc-tree` 比 Chrome 快，`gc-forest` 1.04。`getAttribute` 每次 560 → 378 條、`setAttribute` 1071 → 864 條。數字和檢查在 `progress.md`。

## 動機

- **差距在建節點和建 wrapper**（`measurements.md`）：四頁每一圈都在 `createElement`；頁面自己計時，`createElement('div')` 一次 machopo 80.2 ns、Chrome for Testing 53.2 ns（1.51 倍），`createTextNode` 1.26 倍。
- **之前把 Chrome 量慢了**：會配置的呼叫之前是對這台機器上 Fedora 的 Chromium 量的，它建節點比 Chrome for Testing 慢三到四成（getter 一樣）。對它量，machopo 看起來打平。
- **一次呼叫做了什麼是數得出來的**：gdb 單步，callback 本身 1658 條，照順序是讀名稱和猜 interface 約 260、看 realm 約 80、開 scope 和預先建 wrapper 約 340、建節點約 490、把 wrapper 接上節點約 255、節點指回 wrapper 約 92、頭尾約 140。
- 名稱那一段的做法對每個拿名稱的成員都有用（`getAttribute`、`setAttribute`、`hasAttribute`……），不只這四頁。

不在這份的範圍：GC；`createElement` 以外的建物件（event、range……）；序列化和解析（`innerHTML`、`cssText`）；最熱的 getter 剩下的差。

## 設計概要

對網頁沒有看得到的變化。四段各自獨立：

| 段 | 做什麼 | 開始時估省（條） | `createElement` 每次實際（條） |
| --- | --- | ---: | ---: |
| 1 | 拿掉多餘的檢查，預先建 wrapper 不開 scope | 165 到 245 | 2048 → 1843 |
| 2 | 元素的名稱用 V8 字串的身分查 | 220 到 260 | 1843 → 1423 |
| 3 | 建節點少做事：建立時要多做事的名稱查一張表、`QualifiedName::new` 直接組、elise 的 `__init` inline | 約 200 | 1423 → 1292 |
| 4 | 屬性的名稱用 V8 字串的身分查（開始時不在範圍裡，決定 18 加的） | | `getAttribute` 560 → 378、`setAttribute` 1071 → 864 |

## 詳細設計

每一段的設計在那一段開始時定（「已決定的事項」）；第 1 段的表是開始時量到的。

### 第 1 段：多餘的檢查

| 現在的條數 | 是什麼 | 為什麼多餘 |
| ---: | --- | --- |
| 約 60 | 把預先建好的 wrapper 接上節點（`bindings::unified_heap::wrap`）時，`IsApiWrapper` 兩次、`IsWrapping` 一次 | wrapper 是同一次呼叫裡從樣板複製出來的：一定是 API wrapper，一定還沒接上東西 |
| 約 55 | `new_wrapper` 拿樣板物件時的 `assert_access_allowed`、`Local::New`、比 context | 每次呼叫重做同樣的事 |
| 約 80 | `this` 的 creation context、目前的 context、比較 | Blink 也問一次 creation context；有沒有更便宜的問法（例如只在 `this` 不是目前 realm 的 document 時才問）開始時查 |
| 約 50 | 為了預先建 wrapper 開的 scope（`StateInfo::with_scope`） | 樣板物件已經在的時候（除了每個 realm 每個 interface 的第一次），要做的只有問目前的 context、把樣板的 handle 拿出來、複製它：`bindings::InfoHandles`（不開 scope 就能配置的那一組）已經有 `current_context` 和 `clone_object`，缺的只有問 `this` 的 creation context。第一次建樣板時才開 scope |

第一項要一個不檢查的 `wrap`：`unsafe` 在 `bindings`，論證是「物件剛從樣板複製、還沒交給任何人」，寫進 `crates/bindings/soundness.md`。

### 第 2 段：元素的名稱用 V8 字串的身分查

之前：確認字串的文字借得到（`text_lends` 40 條）、讀出文字（`chars` 28）、看有沒有大寫、用 perfect hash 查固定名稱表再比文字（`foresee_element_words` 75、`find_fixed` 84）；建節點時再照文字檢查合法（38）、找 `-`。

現在：JS 的字面值是 internalized 的字串，同樣內容的在一個 isolate 裡只有一個物件，header 裡有 V8 算好的 hash。

- **表**：isolate 的狀態有一張 1024 格的表（`dom_js` 的 `Caches::name_slots`），照 V8 的 hash 直接對應，每一格記一個名稱的編號；撞到就蓋掉。名稱的 V8 字串在原有的名稱快取裡（`Caches::names`，編號對到它的 internalized 字串）。
- **查**（`front::name_by_identity`）：讀參數的 hash、看那一格的名稱在快取裡的字串和參數是不是同一個物件。是就是那個名稱，不讀文字；操作只拿到編號（`dom_api` 的 `ElementName::Known`），HTML 文件裡直接照編號建，不檢查合法、不找 `-`，其他文件照那個名稱的文字走原本的路。
- **登記**（`front::remember_name`）：認不出來的照文字查；查到的是固定名稱表裡合法、沒有大寫、沒有 `-` 的名稱（`dom_api::typed::is_plain_element_name`，第一次問時算的一張 bitset）就登記，參數那個字串留作名稱快取裡那個名稱的字串（之後 `localName` 回傳的也是它）。
- **GC**：表記的是編號，字串由名稱快取的 `Global` 留著，GC 搬了字串比的還是同一個物件。
- **fork**（只改 Rust）：`v8::machopo::internalized_hash`（照 V8 的佈局讀「是不是 internalized」和 hash 欄位，debug build 和 `Name::GetIdentityHash` 比對）、`Local::is_same_object`（inline 地比 handle 和 `Global`）、`Local::new_in_callback_unchecked`（拿樣板物件的 handle 時不檢查 `Global` 還活著、是不是這個 isolate 的）。
- **原型**（`prototypes/name-identity-*.patch`，`measurements.md`）只接在猜 interface 的那一步，表是 thread-local、記位址。
- **代價**：兩個落在同一格的名稱輪流建時互相蓋掉，兩個都走照文字的路。

### 第 3 段：建節點

| 開始時的條數 | 之前 | 現在 |
| ---: | --- | --- |
| 164 | `Cx::node` 加 `Node::__init`：節點 13 個欄位，先在 stack 上組一個 init 的 struct，再呼叫沒有 inline 的 `__init` 逐欄讀回來寫進 heap，每個 `Option<Gc>` 欄位一次分支 | elise 的 derive 產生的 `__init` 在配置的地方 inline：常數欄位折成幾次整塊寫 0，初始化約 25 條；合計 105 條 |
| 57 | `make_element` 照名稱一個一個比（template、表單的幾種、SVG 的 `use`、`iframe`、`base`） | `dom` 的 build script 多一張表（`MADE_WITH_MORE`）：HTML 元素建立時除了節點還要做事的名稱。查一次，要做事的才進不 inline 的 `html_element_made` |
| 32 | `QualifiedName::new` | 放得進 bits 的名稱 inline 直接組，要進表的在不 inline 的 `new_tabled` |
| 131 | callback 裡 inline 的（參數、進操作、結果） | 沒有細看（「之後的可能」） |

開始時打算的「配置一塊清成 0 的物件」沒有做（決定 16）：bump 出來的記憶體本來就不是 0，清成 0 也要寫；慢的是 `__init` 沒有 inline。剩下的約 80 條是泛用的配置路徑。

### 第 4 段：屬性的名稱用 V8 字串的身分查

`getAttribute`、`setAttribute`、`hasAttribute`、`removeAttribute`、`toggleAttribute` 的 qualified name（`dom_api` 的 `AttributeQualifiedName`）。開始前 `getAttribute` 的 callback 549 條裡和名稱有關的約 180 到 200 條。

- **和元素不同的地方**：沒有大寫、沒有 `:` 的名稱在任何文件裡都是它自己，也只對得到沒有 prefix、local name 相同的屬性，所以不分文件種類；可以有 `-`。
- **表**：屬性自己一張（`Caches::attribute_slots`），查法和登記同第 2 段（`front::NameUse` 選哪一張）。只認固定名稱表裡合法、沒有大寫、沒有 `:` 的（`is_plain_attribute_name`）。
- **操作拿到的**（`AttributeName`）：`Known`（照身分認的：照編號找屬性，不檢查合法）；`Lowercase`（front 讀了文字，沒有大寫和 `:`，帶 front 查到的編號：操作不再照文字查，沒有編號的不是任何屬性的名稱，合法與否操作自己檢查）；`Text`（照原本的路）。
- **`dom`**：照編號找、設、移除屬性（`attribute_value_numbered`、`set_attribute_numbered`、`remove_attribute_numbered`）。

### 量法

- **對 Chrome 一律用 Chrome for Testing**（blink_perf 和 Speedometer 用的那個），不用發行版的 Chromium。
- 每段做完：
  - `bench.py calls --baseline`：`createElement` 的指令、cycles、store forwarding 的衝突；其他成員不增加。
  - gdb 單步 callback（RFC 0097 的 `prototypes/step-callback.py`）：改到的那幾段的條數。
  - `bench.py blink-perf --pages` 四頁：`bindings/create-element`、`gc-forest`、`gc-mini-tree`、`gc-tree`，各跑三次（單次之間有一成的起伏）。
  - 頁面自己計時的三方比較（`prototypes/time-three-browsers.py`）。
- 指令數少了不代表時間少了：cycles 和衝突數一起看；衝突變多的在同一段查清楚。
- Speedometer：全部做完時量一次。

### 測試

- 行為：`dom_js` 和 `dom` 的測試、相關的 WPT 目錄（`dom/nodes`、`custom-elements`、`html/dom`）。
- 第 2 段的新測試：同樣內容但不是同一個物件的字串（串接出來的）、大寫的名稱、有 `-` 的名稱、不合法的名稱、GC 搬動字串之後、兩個 realm、表滿了之後，結果都和照文字查的一樣；名稱由身分認出來時不讀文字（計數）。
- 第 1 段：不檢查的 `wrap` 只在預先建的 wrapper 上用，debug build 仍然檢查。
- 第 3 段動到 elise：它自己的測試、模型、fuzz；merge 之後手動開始 `gc.yml`。
- 每段的 mutation test。

### 分階段

三段，照「設計概要」的表。順序見「待決定」的第 2 條。

## 缺點

- 第 2 段依賴 V8 字串的佈局（instance type 的一個 bit、hash 欄位的位置和格式）：V8 升級時要跟著檢查。debug build 對 V8 自己的 API 比對，升級時壞了看得到。
- 第 2 段比 Blink 做得多：Blink 對短字串照文字查 `AtomicString` 的表。多一張要維護的表。
- 第 1 段多一處不檢查的 `unsafe`。
- 第 3 段要動 elise 配置物件的介面。
- 估計可能不準：三段裡只有第 2 段的一部分量過。全部做到估計還差 Chrome 約 100 到 250 條。

## 理由和其他做法

- **名稱**：之前量過不做的是最近查過的名稱的小快取（每個名稱省約 40 條；會淘汰、Blink 沒有）和 external string（短的名稱要 V8 的 sandbox）。身分的表不淘汰、行為確定，原型省 157 條。
- **不做、留著**：V8 複製樣板物件（約 170，Blink 也付）、登記 wrapper 的指標表（Blink 也付）。

## 先例

- Blink 的 `V8DOMWrapper::CreateWrapper`、`DOMDataStore::SetWrapper`：wrapper 從每個 context 的樣板複製、接上物件，沒有再檢查一次它是不是 wrapper。
- Blink 的 `HTMLElementFactory::Create`：用 `AtomicString`（指標）查一張 hash 表拿到建構的函式；名稱本身是進來時轉成 `AtomicString` 的（短字串照文字查 `AtomicString` 的表）。
- Blink 的 `MakeGarbageCollected<HTMLDivElement>`：物件在 Oilpan 配置的位置上直接建構。

## 已決定的事項

2026-11-15（使用者：都照建議；選項在「當時的選項」）：

1. **三段都排進這份**，一段一段做，每段做完看數字再決定下一段。
2. **順序是第 1、2、3 段**：第 1 段最小、最確定，先走完一次量法；第 2 段有原型；第 3 段動 elise，放最後。
3. **第 2 段先做 `createElement`**，量完再決定屬性名稱要不要跟著做（在這份的第 2 段裡）。
4. **原型留在 `prototypes/`**。
5. **預先建 wrapper 的 scope 放進第 1 段**（使用者問能不能優化）：樣板物件已經在時不開 scope，用 `InfoHandles`。

第 1 段，2026-11-15（使用者：都照建議；選項在「待決定」原本的列表）：

6. **接上預先建的 wrapper 用型別保證不檢查**：`bindings` 的 `NewWrapper`（只有複製樣板做得出來、不能複製、接上時消耗掉）。不選 `unsafe fn` 加註解（`dom_js` 不能寫 `unsafe`，包成安全的函式就是靠註解）；不選留著檢查。
7. **realm 只問 `this` 的 creation context**，拿它找樣板；樣板對不上（第一次、另一個 realm）才走原本的路。
8. **這一段不動 fork**：handle 的比較留到第 2 段。
9. **這一段在 fork 加一個不用 scope 問 creation context 的入口**（使用者，同一天稍後）：決定 5、7、8 對不上（不開 scope 問 creation context 在 fork 沒有入口），三個裡改的是決定 8：加那一個入口，只改 Rust；handle 的比較仍然留到第 2 段。不選把 scope 和 realm 延到第 2 段（會把和名稱無關的改動混進去）；不選改用目前的 context 判斷（跨 realm 的呼叫要另外確認，比較複雜）。

第 2 段，2026-11-15（使用者：都照建議；選項在「待決定」原本的列表）：

10. **表只記「V8 的 hash → 名稱的編號」**，比身分用現有的名稱快取（`Caches::names`）；第一次照文字認到一個名稱時，參數那個字串就是快取裡那個名稱的字串（之後 `localName` 回傳的也是它，不另建）。不選每一格自己留 handle（同一個字串留兩份）。
11. **固定 1024 格、照 hash 直接對應、撞到就蓋掉**。不選會長大的 hash 表（每次查多幾條，多一塊要管的記憶體）。代價：兩個落在同一格的名稱輪流建時互相蓋掉，兩個都走照文字的路，每次還多一次登記；一般頁面用到的元素名稱一兩百個以內，1024 格裡撞到的機會小。
12. **只認固定名稱表裡的**「合法的元素名稱、沒有 `-`、沒有大寫」（`dom_api::typed::is_plain_element_name`）。和當時寫的不同：旗標不是編譯時算的，是第一次問時算一張 bitset（名稱表是 `dom` 的，合法與否的規則在 `dom_api`；編譯時算要把規則搬進 `dom` 的 build script）。不選連執行時才出現的名稱也認。
13. **fork 加三個函式**（只改 Rust）：`v8::machopo::internalized_hash`、`Local::is_same_object`、`Local::new_in_callback_unchecked`。第三個和當時寫的不同：不是「把 `Global` 當 `Local` 借來用、不另建 handle」，而是照樣建一個 handle、只拿掉 `Global` 還活著和是不是這個 isolate 的兩個檢查。借來用的 `Local` 指向 `Global` 自己的那一格，`Global` 被丟掉或換掉之後它就懸空，而 `Local` 的 lifetime 是整個 callback 的，型別擋不住；建 handle 只多 18 條。

第 2 段做完之後，2026-11-15（使用者：都 A，沒做的列在「之後的可能」）：

14. **屬性名稱這份不做**（決定 3 留下的）：`getAttribute` 一次 560 條，名稱那一段佔多少還沒量。不選在第 2 段接著做 `getAttribute`、`setAttribute`、`hasAttribute`、`removeAttribute`。
15. **第 3 段只做不動 elise 的兩項**：`make_element` 查名稱表的旗標、`QualifiedName::new` 的名稱放得進 bits 時直接組。不選連 elise 配置清成 0 的物件一起做（要新增入口，並在編譯時檢查「全 0 是這個型別的合法值」，跑它的 models、fuzz、miri）：`create-element` 這頁已經和 Chrome for Testing 打平，那一項的收穫是所有建節點的路徑的，另外開一份做比較合適。不選第 3 段整個不做。

第 3 段做完之後，2026-11-15（使用者想研究配置清成 0 的物件；先做了原型，選項在 `progress.md` 的「第 3 段的第三項」）：

16. **elise 的 derive 產生的 `__init` 在配置的地方 inline，併進這份當第 3 段的第三項**。讀了程式之後，建節點慢的不是「沒有清成 0」（bump 出來的記憶體本來就不是 0，清成 0 也要寫），而是 `__init` 沒被 inline：呼叫的人知道 11 個欄位是常數，卻先寫進 stack 上的 init struct、再一欄一欄讀回來。inline 之後編譯器把常數欄位折成幾次整塊寫 0，初始化只剩約 25 條，「清成 0 的配置入口」能再省的只剩個位數，不做。不選另開一份 RFC 連泛用的配置路徑一起研究；不選不做。
17. **所有型別都 inline**（原型的做法，binary 沒有變大）。不選加一個屬性只標指定的型別。

第 4 段，2026-11-15（使用者：把屬性名稱照字串的身分認做完；選項在 `progress.md` 的「第 4 段」，都照建議）：

18. **決定 14 改掉：屬性名稱在這份做，當第 4 段**。量了之後 `getAttribute` 的 549 條裡和名稱有關的約 180 到 200 條（`text_lends` 51、掃大寫 59、`find_fixed` 68、有 prefix 的分支約 20），表和 fork 的函式都是這份做的。不選另開一份。
19. **範圍是 `getAttribute`、`setAttribute`、`hasAttribute`、`removeAttribute`、`toggleAttribute`**（同一種參數）。不選加上 `getElementsByTagName`（回傳快取的集合，熱迴圈裡少見，名稱的規則是元素的那一套）。
20. **屬性自己一張 1024 格的表**（`Caches::attribute_slots`）：熱路上不多檢查，兩種名稱不互相擠掉，每個 isolate 多 4 KB。不選和元素共用（每一側查到之後要再看自己的旗標，`createElement` 每次多幾條）。
21. **只認固定名稱表裡的**「合法的屬性名稱、沒有大寫、沒有 `:`」（`dom_api::typed::is_plain_attribute_name`），和決定 12 一致；可以有 `-`。不選連執行時才出現的名稱（頁面自訂的 `data-*`）也認：名稱快取會跟著留字串，名稱表本身回不回收沒有查。

認不出來的那條路沒有選項：front 就地看文字，沒有大寫、沒有 `:` 的名稱由 front 查編號、交給操作（`AttributeName::Lowercase`），操作不再照文字查；front 不查兩次。

## 當時的選項

1. **範圍：三段都做，還是先做一部分**
   - A（建議）：三段都排進這份，一段一段做，每段做完看數字再決定下一段。
   - B：只做第 1、2 段，第 3 段（動 elise）另外開。
2. **順序**
   - A（建議）：第 1 段、第 2 段、第 3 段。第 1 段最小、最確定，先走完一次量法；第 2 段有原型；第 3 段動 elise，放最後。
   - B：第 2 段先（收穫最大，而且對其他成員也有用）。
3. **第 2 段推到哪些成員**
   - A（建議）：先做 `createElement`，量完再決定屬性名稱要不要跟著做（寫進這份的第 2 段，不另開 RFC）。
   - B：一開始就連屬性名稱一起做。
4. **原型要不要留在 RFC 的資料夾**
   - A（建議）：留（`prototypes/`，和之前的 RFC 一樣）。
   - B：不留，只留數字。

## 待決定

沒有了。下面是各段開始時列的選項。

第 1 段的三條已經定了（決定 6 到 8），當時的選項留在這裡：

1. **接上預先建的 wrapper 時怎麼不檢查**（`bindings::unified_heap::wrap` 的 `assert!(wrapper.is_api_wrapper() && !is_wrapping(..))`，約 60 條）
   - A（建議）：用型別保證。`bindings` 給一個 `NewWrapper<'s>`：只有「從樣板複製」的那個入口做得出來，不能複製，`wrap_new` 收下它（消耗掉）、不檢查。front 預先建的 wrapper（`FrontTypes::PreparedNode`）帶的是它而不是 `Local<Object>`。`unsafe` 只在 `bindings` 裡那一處，論證是「這個值只能是剛複製出來、還沒交給任何人的 wrapper；樣板是 `FunctionTemplate` 的 instance、從不接東西」。debug build 仍然檢查。
   - B：`unsafe fn wrap_unchecked`，呼叫的地方寫 `// SAFETY:`。改動最小；但 `unsafe` 的呼叫點會在 `bindings` 之外（`dom_js` 不能寫 `unsafe`），所以得在 `bindings` 包成安全的函式，等於靠註解保證。
   - C：留著檢查。
2. **看 realm 的那一步**（現在問 `this` 的 creation context、問目前的 context、比較，再用目前的 context 找樣板，約 80 條加樣板那邊再比一次）
   - A（建議）：只問 `this` 的 creation context，直接拿它當新 wrapper 的 realm 去找樣板（樣板本來就是照 context 快取的，複製出來的 wrapper 是樣板那個 realm 的）。不問目前的 context，少一次比較，估省約 35 條；別的 realm 的函式呼叫在這個 realm 的物件上時，也不用退回「操作後再建」的慢路。Blink 也是問 receiver 的 creation context（`ScriptState::ForRelevantRealm`）。
   - B：照現在的，兩個都問。
3. **比兩個 handle 是不是同一個物件**（現在是呼叫 V8 的 `v8__Data__EQ`，一次 14 條，這條路上兩次）
   - A（建議）：這一段先不動；第 2 段本來就要在 fork 加照佈局讀的函式，到時候一起加一個 inline 的比較。
   - B：這一段就動 fork。

預先建 wrapper 不開 scope（決定 5）的做法沒有選項：樣板已經在時用 `InfoHandles`（加一個問 creation context 的方法），第一次才開 scope 建樣板。

第 2 段的四條已經定了（決定 10 到 13），當時的選項留在這裡：

1. **表放哪裡、長什麼樣**
   - A（建議）：表只記「V8 的 hash → 名稱的編號」；比身分用現有的名稱快取（`Caches::names`：名稱的編號對到它的 internalized 的 V8 字串，現在回傳名稱時用）。第一次照文字認到一個名稱時，把參數那個字串登記進快取。不多留 handle。
   - B：表的每一格自己留一個 handle。獨立；同一個字串留兩份。
2. **表的大小和撞到時**
   - A（建議）：固定 1024 格，照 hash 直接對應，撞到就蓋掉（下次重新照文字查，不影響正確）。
   - B：會長大的 hash 表：不漏，每次查多幾條，多一塊要管的記憶體。
3. **認哪些名稱**
   - A（建議）：只認固定名稱表裡的；「合法的元素名稱、沒有 `-`」的旗標編譯時算好。
   - B：連執行時才出現的名稱也認（custom element 的）：要在名稱第一次出現時算旗標，而這類名稱本來就走慢的路。
4. **fork 加的**（只改 Rust）
   - A（建議）：三個一起：讀 internalized 字串的 hash；inline 地比一個 handle 和一個 `Global` 是不是同一個物件（現在呼叫 `v8__Data__EQ`，一次 14 條）；把 `Global` 當 `Local` 借來用、不另建 handle（第 1 段剩下的拿樣板物件的 58 條）。
   - B：只加第一個。

`dom_api` 這一側沒有選項：`ElementLocalName` 的參數多一種「已知的名稱」的形式（只帶編號），`document::create_element` 拿到它時不檢查合法、不找 `-`。store forwarding 的衝突每一步量，變多的當場查。

之後的段落開始時列那一段的。

## 之後的可能

- **配置清成 0 的物件**：不做（決定 16）。`__init` inline 之後節點的初始化只剩約 25 條，再省的只剩個位數。cppgc 的做法是 sweep 時把回收的格子清成 0、配置時不用寫，那動的是 sweep。
- **泛用的配置路徑**（`__init` inline 之後 `Cx::node` 的 105 條裡約 80 條，連函式進出：查型別的資訊、看 nursery、size class 的 cursor、bump、寫 header）：每一次配置都走，沒有看出明顯多做的事。
- **`getElementsByTagName` 的名稱照字串的身分認**（決定 19 沒做的）：規則是元素的那一套，可以用元素的表。
- **執行時才出現的屬性名稱**（決定 21 沒做的，頁面自訂的 `data-*`）：字面值現在每次由 front 照文字查一次編號（操作不再查），不靠身分。要認的話先確認名稱表回不回收。
- **callback 裡 inline 的那一段**（參數、進操作、結果，當時量到 131 條）：沒有細看。
- **其他不照身分認的名稱**：`createElementNS`、大寫的名稱、custom element 的名稱、執行時才出現的名稱。
- **兩個落在同一格的名稱輪流建**（決定 11 的代價）：真的碰到時，一格放兩個名稱。

- `createTextNode`、`createComment`、`cloneNode` 的同一件事（第 1、3 段的改動它們大多直接受惠，沒有另外量）。
- 發行版的 Chromium 量出來的舊數字裡，會配置的項目要對 Chrome for Testing 重量。
