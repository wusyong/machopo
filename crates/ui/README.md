# ui

machopo 自己的 UI 框架：瀏覽器的分頁列、工具列、網址列是用它做的，但它不知道瀏覽器是什麼，不依賴瀏覽器的 crate（只依賴 `base`、`gfx`、`compositor_paint`、`parley`、`taffy`，`python3 scripts/sanity.py deps` 檢查；renderer 那一側不能依賴它，`sanity.py boundary` 檢查）。位置相當於 Chromium 的 `//ui/views`，設計照 machopo 自己的目標比較（彈性、效能、成本、當成 GUI API 好不好用、好不好測、不靠 `Rc` / `RefCell`），Views 只是參考之一。瀏覽器用它的部分在 `shell` 的 `chrome.rs`。

## 怎麼運作

- **樹**：`ViewTree<A>` 把 view 存在一個 arena（`Vec`）裡，用帶世代的 id 指：`ViewId<T>` 拿到的就是 `T`（插入時就定了型別），`AnyViewId` 是不管型別的。拿掉的 view 的 id 查不到東西，那一格給新的 view 重用時也查不到。父、子、焦點、收住指標的 view 都只是 id。
- **App**：`A: App` 帶兩個型別：view 送出的訊息（`Message`）和畫的時候用的主題（`Theme`）。
- **排版**：每個節點有一個 taffy 的 `Style`。樹實作 taffy 的低階 trait（和 `layout` crate 的 `BoxTree` 同樣的做法）：有子節點的是 flex 或 grid 容器，沒有子節點的是葉子，由 view 的 `measure` 量；結果在整棵樹上快取。bounds 是 f32 的 CSS px，相對於父 view。
- **畫**：每個 view 錄自己的 display list（自己的座標，不含子 view），留到它要求重畫或大小變了為止。整棵樹組成一個 layer（`Frame`），附上變了的範圍：重畫的 view 的舊位置和新位置、搬動的 view 的兩個位置、拿掉和隱藏的 view 原本的位置，夾在 layer 裡。使用的一方交給 compositor，只重 raster 碰到的 tile。
- **事件**：指標給收住它的 view，或 hit test 找到的最深的 view，不處理（`Handled::No`）就往父 view 找；處理了按下的 view 收住指標，直到那個按鈕放開。hover 的進出比較前後兩次指標下面的路徑（離開的從深到淺，進來的從淺到深）。按鍵和輸入法給有焦點的 view，同樣往上找。
- **宣告式的一層**：app 從它的狀態描述 UI，是一棵 `Element` 的樹（每個節點是一種 view 的 Props、它的 `Style`，可以帶 key 和名字），交給 `ViewTree::describe`。比對前一次的描述：同一種 view、同一個 key（沒有 key 的照兄弟之間的位置）的留著，Props 不相等才呼叫它的 `set_props`，換了位置的搬過去；其他的建新的，或拿掉。名字在描述裡唯一，`find::<T>(名字)` 拿到帶型別的 id，給命令式的操作（讓網址列取得焦點）和測試用。用描述管理的子 view 不要再用 `append_child` 手動加。
- **訊息**：view 處理事件時 `ctx.submit(message)`，使用的一方在分派完之後 `take_messages()`，照順序處理。元件內部的狀態（編輯中的文字、hover 的樣子）留在 view 自己。
- **主題**：app 用 `tokens!` 宣告 token 的 enum，`TokenTable` 是每份主題的值（用 `match` 建，少一個 token 編不過）。
- **文字**：樹擁有一份 `TextContexts`（parley 的字型和排版暫存），view 在量測、畫、處理事件時借；樹外面要畫字的也可以借（`ViewTree::text`）。建一份 `FontContext` 就重新掃一次系統字型，所以不要每個 view 各建一份。

## 用法

```rust
struct MyApp;
impl ui::App for MyApp {
    type Message = Command;
    type Theme = MyTheme;
}

let mut tree = ui::ViewTree::<MyApp>::new(MyTheme::light(), ui::TextContexts::new(false));
tree.set_size(SizeF::new(800.0, 40.0));

// 從狀態描述 UI；狀態變了就再描述一次，只有 Props 變了的 view 被碰到。
fn describe(state: &MyState) -> ui::Element<MyApp> {
    ui::Element::container()
        .style(Style { flex_direction: FlexDirection::Row, ..Style::default() })
        .child(ui::Element::new::<MyButton>(MyButtonProps { enabled: state.can_go_back }).name("back"))
}
tree.describe(describe(&state));
let back: ui::ViewId<MyButton> = tree.find("back").unwrap();

// 命令式地改（描述管不到的、view 自己的狀態）：view 自己判斷有沒有變，有變才要求重畫。
tree.update(back, |button, ctx| button.flash(ctx));

// 事件：座標是樹的 CSS px。
tree.pointer(PointerEvent::Down { position, button: PointerButton::Primary, click_count: 1, modifiers });
for message in tree.take_messages() { /* … */ }

// 畫：沒有變的時候是 None。
if let Some(frame) = tree.paint() { /* frame.layers[0].display_list、invalidation */ }
```

## 寫一個 view 的規則

- **改了畫出來的樣子就 `ctx.request_paint()`，改了影響大小的就 `ctx.request_layout()`**：事件處理和 `ViewTree::update` 拿到的是同一種 `EventCtx`。debug build 的每次 `paint` 都讓每個 view 重錄一次，和留著的比較，不一樣就 panic（`set_check_recordings` 可以關，只給數 paint 次數的測試用），所以漏了會在測試裡被抓到。
- **`paint` 只依賴 view 的狀態和 context**（大小、焦點、主題）：同樣的狀態要錄出同樣的 display list。可以在 paint 裡更新只為了畫的狀態（網址列捲動的位置），但重錄一次要得到一樣的結果。
- **畫的 op 帶它的範圍**（`push_op` 或 `push(op, Some(rect))`）：沒帶範圍的 op 可能畫在任何地方，那個 view 一變就整個 layer 重新 raster。
- **座標都是 view 自己的**：左上角是 (0, 0)，大小是 `ctx.size()`；畫到 bounds 外面（例如焦點的外框）也可以，範圍照實帶上。
- **給描述用的 view 實作 `Component`**：`Props`（`PartialEq`）、`create(props, ctx)`、`set_props(props, ctx)`。`set_props` 只在 Props 不相等時被呼叫，照樣要自己判斷畫出來的樣子有沒有變。
- **葉子實作 `measure`**；有子 view 的節點用 `Style` 決定怎麼排，不用 `measure`。
- 只想收子 view 的事件、自己不被點到的容器，`hit_test` 回傳 `false`（`Container` 就是）。

## 還沒有的

- 排版：block 的容器、`position: absolute`（taffy 的 fork 把 out-of-flow 的子節點交給包含它的區塊排，要實作它的 `LayoutContainingBlock`）、自己寫排法的容器（排版時 view 要能叫子 view 排）。
- 個別 view 自己的 layer（捲動的清單只改位移、動畫），裁切。
- 焦點的走訪（Tab 鍵）、無障礙、動畫、拖放。

## 測試

`cargo test -p ui`：描述（第一次建立、Props 相等不碰、新的 Props、有 key 的搬動、沒有 key 的照位置、換型別、拿掉、名字）、id 和世代、flex 的排版和要求重排、hit test、事件往上找、收住指標（連同那個 view 被拿掉時）、hover 的順序、焦點和按鍵、游標、只重畫變了的 view 和各種變了的範圍、debug build 的錄製檢查、token 表。瀏覽器的 view 的測試在 `shell` 的 `chrome.rs`、`omnibox.rs`、`tab_strip.rs` 和整合測試的 `browser_view`。
