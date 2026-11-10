// TodoMVC as a wasm guest (the world `script`), on the page
// `tests/scenarios/pages/todomvc-wasm.html`, whose markup and style sheets
// are the JS app's (`tests/sites/todomvc`): that app's model, view and
// controller, one for one (what each function here is of is named in its
// comment), so that the two do the same to the same input. Not kept: the
// todos (the JS app's store is in memory too).
//
// Where the JS app would throw (an element it looks for is gone), a
// listener here returns: a trap would end the guest.
#include "script.h"

#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef machopo_dom_dom_node_t node_t;
typedef machopo_dom_dom_dom_exception_t exception_t;
typedef machopo_dom_dom_option_element_t option_element_t;

// A call that must succeed: the guest has nowhere to report to but a trap.
#define OK(call)                         \
    do {                                 \
        exception_t err_;                \
        if (!(call)) __builtin_trap();   \
    } while (0)

// A string argument, borrowed.
static script_string_t s(const char *text) {
    script_string_t string;
    script_string_set(&string, text);
    return string;
}

// A string result, copied into `out` (cut to its size) and freed.
static void take(script_string_t *string, char *out, size_t size) {
    size_t length = string->len < size - 1 ? string->len : size - 1;
    memcpy(out, string->ptr, length);
    out[length] = 0;
    script_string_free(string);
}

// The elements the view keeps (the JS view's `$todoList`...), for as long
// as the guest runs.
static node_t document, window, todo_list, item_counter, clear_completed, main_block, footer, toggle_all_input,
    toggle_all, new_todo;

// The handles to let go when the export that runs returns (`drop`). An
// object has one handle in a store, however many times the guest was
// given it: a function that released the one it got would take it from
// its caller, which may hold the same object (a listener's target, found
// again by a query).
enum { MAX_DROPPED = 2048 };
static node_t dropped[MAX_DROPPED];
static size_t dropped_count;

// `handle` isn't used after the export that runs: one of the view's own
// is kept.
static void drop(node_t handle) {
    const node_t kept[] = {document,   window, todo_list,        item_counter, clear_completed,
                           main_block, footer, toggle_all_input, toggle_all,   new_todo};
    for (size_t i = 0; i < sizeof kept / sizeof kept[0]; i++) {
        if (kept[i] == handle) return;
    }
    for (size_t i = 0; i < dropped_count; i++) {
        if (dropped[i] == handle) return;
    }
    // Full: the handle stays the store's (until the page goes).
    if (dropped_count < MAX_DROPPED) dropped[dropped_count++] = handle;
}

static void release_dropped(void) {
    for (size_t i = 0; i < dropped_count; i++) machopo_dom_dom_release(dropped[i]);
    dropped_count = 0;
}

// helpers.js's `qs`: the first element matching `selector` in `scope`
// (the document, if 0); false if there is none.
static bool qs(const char *selector, node_t scope, node_t *found) {
    script_string_t selectors = s(selector);
    option_element_t element;
    if (scope) {
        OK(machopo_dom_dom_element_query_selector(scope, &selectors, &element, &err_));
    } else {
        OK(machopo_dom_dom_document_query_selector(document, &selectors, &element, &err_));
    }
    *found = element.val;
    return element.is_some;
}

// One the page always has.
static node_t must(const char *selector) {
    node_t found;
    if (!qs(selector, 0, &found)) __builtin_trap();
    return found;
}

static void set_attribute(node_t element, const char *attribute, const char *value) {
    script_string_t name = s(attribute), text = s(value);
    OK(machopo_dom_dom_element_set_attribute(element, &name, &text, &err_));
}

static void set_class(node_t element, const char *value) {
    script_string_t text = s(value);
    OK(machopo_dom_dom_element_set_class_name(element, &text, &err_));
}

static void set_text(node_t node, const char *value) {
    script_string_t text = s(value);
    OK(machopo_dom_dom_node_set_text_content(node, &text, &err_));
}

// `element.style.display = value`.
static void set_display(node_t element, const char *value) {
    script_string_t property = s("display"), text = s(value);
    node_t style;
    OK(machopo_dom_dom_html_element_style(element, &style, &err_));
    OK(machopo_dom_dom_css_style_declaration_set_property(style, &property, &text, NULL, &err_));
    drop(style);
}

// A new `<tag>`, the last child of `parent`.
static node_t append(node_t parent, const char *tag) {
    script_string_t name = s(tag);
    node_t element, appended;
    OK(machopo_dom_dom_document_create_element(document, &name, &element, &err_));
    OK(machopo_dom_dom_node_append_child(parent, element, &appended, &err_));
    return element;
}

// --- model.js and store.js: the todos, in the order they were made.

enum { MAX_TODOS = 64, MAX_TITLE = 256 };

typedef struct {
    int id;
    char title[MAX_TITLE];
    bool completed;
} todo_t;

static todo_t todos[MAX_TODOS];
static size_t todo_count;
static int next_id = 1;

static todo_t *find(int id) {
    for (size_t i = 0; i < todo_count; i++) {
        if (todos[i].id == id) return &todos[i];
    }
    return NULL;
}

// `Model.create`: `title` is trimmed already.
static void model_create(const char *title, bool completed) {
    if (todo_count == MAX_TODOS) return;
    todo_t *todo = &todos[todo_count++];
    todo->id = next_id++;
    snprintf(todo->title, sizeof todo->title, "%s", title);
    todo->completed = completed;
}

static void model_remove(int id) {
    todo_t *todo = find(id);
    if (!todo) return;
    size_t at = (size_t)(todo - todos);
    memmove(todo, todo + 1, (todo_count - at - 1) * sizeof *todo);
    todo_count--;
}

// `text`, without the white space around it (`String.prototype.trim`, of
// ASCII's).
static void trim(const char *text, char *out, size_t size) {
    while (*text == ' ' || *text == '\t' || *text == '\n' || *text == '\r') text++;
    size_t length = strlen(text);
    while (length && (text[length - 1] == ' ' || text[length - 1] == '\t' || text[length - 1] == '\n'
                      || text[length - 1] == '\r')) {
        length--;
    }
    if (length > size - 1) length = size - 1;
    memcpy(out, text, length);
    out[length] = 0;
}

// --- view.js: `View.render`'s commands.

// The item of the todo `id`, if the list shows it.
static bool item_of(int id, node_t *item) {
    char selector[40];
    snprintf(selector, sizeof selector, "[data-id=\"%d\"]", id);
    return qs(selector, 0, item);
}

// `showEntries`: the list's items replaced by one for each of the todos
// that are `completed` (all of them, if `all`), as template.js's `show`
// makes them:
//   <li data-id class data-priority>
//     <div class="view">
//       <input class="toggle" type="checkbox" [checked]>
//       <label>title</label>
//       <button class="destroy"></button>
static void show_entries(bool all, bool completed) {
    for (;;) {
        machopo_dom_dom_option_node_t child;
        node_t removed;
        OK(machopo_dom_dom_node_first_child(todo_list, &child, &err_));
        if (!child.is_some) break;
        OK(machopo_dom_dom_node_remove_child(todo_list, child.val, &removed, &err_));
        drop(child.val);
    }
    int shown = 0;
    for (size_t i = 0; i < todo_count; i++) {
        const todo_t *todo = &todos[i];
        if (!all && todo->completed != completed) continue;
        char number[16];
        node_t item = append(todo_list, "li");
        snprintf(number, sizeof number, "%d", todo->id);
        set_attribute(item, "data-id", number);
        set_attribute(item, "class", todo->completed ? "completed" : "");
        snprintf(number, sizeof number, "%d", 4 - shown % 5);
        set_attribute(item, "data-priority", number);
        shown++;

        node_t view = append(item, "div");
        set_attribute(view, "class", "view");
        node_t toggle = append(view, "input");
        set_attribute(toggle, "class", "toggle");
        set_attribute(toggle, "type", "checkbox");
        if (todo->completed) set_attribute(toggle, "checked", "");
        node_t label = append(view, "label");
        set_text(label, todo->title);
        node_t destroy = append(view, "button");
        set_attribute(destroy, "class", "destroy");

        drop(destroy);
        drop(label);
        drop(toggle);
        drop(view);
        drop(item);
    }
}

// `removeItem`.
static void view_remove_item(int id) {
    node_t item, removed;
    if (!item_of(id, &item)) return;
    exception_t err;
    // Not the list's child: the JS app throws here.
    machopo_dom_dom_node_remove_child(todo_list, item, &removed, &err);
    drop(item);
}

// `setFilter`: the filter's link for `page` ("", "active", "completed")
// is the selected one.
static void view_set_filter(const char *page) {
    char selector[64];
    node_t link;
    if (!qs(".filters .selected", 0, &link)) return;
    set_class(link, "");
    drop(link);
    snprintf(selector, sizeof selector, ".filters [href=\"#/%s\"]", page);
    if (!qs(selector, 0, &link)) return;
    set_class(link, "selected");
    drop(link);
}

// `elementComplete`.
static void view_element_complete(int id, bool completed) {
    node_t item, input;
    if (!item_of(id, &item)) return;
    set_class(item, completed ? "completed" : "");
    // In case it was toggled from an event and not by clicking the checkbox.
    if (qs("input", item, &input)) {
        OK(machopo_dom_dom_html_input_element_set_checked(input, completed, &err_));
        drop(input);
    }
    drop(item);
}

// `editItem`: the item is being edited, in a new field that has the focus
// and the title.
static void view_edit_item(int id, const char *title) {
    node_t item;
    if (!item_of(id, &item)) return;
    script_string_t name, text = s(title);
    char classes[64], edited[80];
    OK(machopo_dom_dom_element_class_name(item, &name, &err_));
    take(&name, classes, sizeof classes);
    snprintf(edited, sizeof edited, "%s editing", classes);
    set_class(item, edited);

    node_t input = append(item, "input");
    set_class(input, "edit");
    OK(machopo_dom_dom_html_element_focus(input, &err_));
    OK(machopo_dom_dom_html_input_element_set_value(input, &text, &err_));
    drop(input);
    drop(item);
}

// `editItemDone`: the field gone, the item's label `title`.
static void view_edit_item_done(int id, const char *title) {
    node_t item, input, removed, label;
    if (!item_of(id, &item)) return;
    if (!qs("input.edit", item, &input)) {
        // Gone already (the JS app throws here).
        drop(item);
        return;
    }
    OK(machopo_dom_dom_node_remove_child(item, input, &removed, &err_));
    drop(input);

    // `className.replace("editing", "")`: the first one.
    script_string_t name;
    char classes[64];
    OK(machopo_dom_dom_element_class_name(item, &name, &err_));
    take(&name, classes, sizeof classes);
    char *editing = strstr(classes, "editing");
    if (editing) memmove(editing, editing + 7, strlen(editing + 7) + 1);
    set_class(item, classes);

    if (qs("label", item, &label)) {
        set_text(label, title);
        drop(label);
    }
    drop(item);
}

// --- controller.js

// The route shown, and the one before (`_activeRoute`, `_lastActiveRoute`).
typedef enum { ROUTE_NONE, ROUTE_ALL, ROUTE_ACTIVE, ROUTE_COMPLETED, ROUTE_UNKNOWN } route_t;
static route_t active_route = ROUTE_NONE, last_active_route = ROUTE_NONE;

// `_updateCount`: the count of what is left, the button that clears what is
// done, and the list's and the footer's blocks (shown if there is a todo).
static void update_count(void) {
    int completed = 0;
    for (size_t i = 0; i < todo_count; i++) completed += todos[i].completed;
    int active = (int)todo_count - completed;

    // "<strong>2</strong> items left", template.js's `itemCounter`.
    char count[64];
    snprintf(count, sizeof count, "<strong>%d</strong> item%s left", active, active == 1 ? "" : "s");
    script_string_t html = s(count);
    OK(machopo_dom_dom_element_set_inner_html(item_counter, &html, &err_));

    set_text(clear_completed, completed > 0 ? "Clear completed" : "");
    set_display(clear_completed, completed > 0 ? "block" : "none");
    set_display(main_block, todo_count > 0 ? "block" : "none");
    set_display(footer, todo_count > 0 ? "block" : "none");
}

// `_filter`: the count, and the list made again unless it shows all and
// did before.
static void filter(bool force) {
    update_count();
    // The JS app calls `this["show" + route]`, and throws at a route it
    // has none for.
    if (active_route == ROUTE_UNKNOWN) return;
    if (force || last_active_route != ROUTE_ALL || last_active_route != active_route) {
        show_entries(active_route == ROUTE_ALL, active_route == ROUTE_COMPLETED);
    }
    last_active_route = active_route;
}

// `setView`: the route in the location's fragment ("#/active": the part
// after its first slash).
static void set_view(void) {
    node_t location;
    script_string_t text;
    char hash[64], page[64] = "";
    OK(machopo_dom_dom_window_location(window, &location, &err_));
    OK(machopo_dom_dom_location_hash(location, &text, &err_));
    take(&text, hash, sizeof hash);
    drop(location);
    const char *slash = strchr(hash, '/');
    if (slash) {
        snprintf(page, sizeof page, "%s", slash + 1);
        char *next = strchr(page, '/');
        if (next) *next = 0;
    }
    // `_updateFilterState`.
    if (page[0] == 0) {
        active_route = ROUTE_ALL;
    } else if (strcmp(page, "active") == 0) {
        active_route = ROUTE_ACTIVE;
    } else if (strcmp(page, "completed") == 0) {
        active_route = ROUTE_COMPLETED;
    } else {
        active_route = ROUTE_UNKNOWN;
    }
    filter(false);
    if (active_route != ROUTE_UNKNOWN) view_set_filter(page);
}

static void add_item(const char *title) {
    char trimmed[MAX_TITLE];
    trim(title, trimmed, sizeof trimmed);
    if (trimmed[0] == 0) return;
    model_create(trimmed, false);
    // `clearNewTodo`.
    script_string_t empty = s("");
    OK(machopo_dom_dom_html_input_element_set_value(new_todo, &empty, &err_));
    filter(true);
}

static void remove_item(int id) {
    model_remove(id);
    view_remove_item(id);
    filter(false);
}

static void edit_item(int id) {
    const todo_t *todo = find(id);
    if (todo) view_edit_item(id, todo->title);
}

static void edit_item_save(int id, const char *title) {
    char trimmed[MAX_TITLE];
    trim(title, trimmed, sizeof trimmed);
    todo_t *todo = find(id);
    if (trimmed[0] == 0) {
        remove_item(id);
    } else if (todo) {
        snprintf(todo->title, sizeof todo->title, "%s", trimmed);
        view_edit_item_done(id, trimmed);
    }
}

static void edit_item_cancel(int id) {
    const todo_t *todo = find(id);
    if (todo) view_edit_item_done(id, todo->title);
}

static void toggle_complete(int id, bool completed, bool silent) {
    todo_t *todo = find(id);
    if (todo) {
        todo->completed = completed;
        view_element_complete(id, completed);
    }
    if (!silent) filter(false);
}

static void toggle_all_to(bool completed) {
    for (size_t i = 0; i < todo_count; i++) {
        if (todos[i].completed != completed) toggle_complete(todos[i].id, completed, true);
    }
    filter(false);
}

static void remove_completed_items(void) {
    // Those that are done now: removing one moves the rest.
    int ids[MAX_TODOS];
    size_t count = 0;
    for (size_t i = 0; i < todo_count; i++) {
        if (todos[i].completed) ids[count++] = todos[i].id;
    }
    for (size_t i = 0; i < count; i++) remove_item(ids[i]);
    filter(false);
}

// --- `View.bind`: the listeners, each by its number.

enum {
    ON_LOAD = 1,
    ON_HASH_CHANGE,
    ON_NEW_TODO,
    ON_REMOVE_COMPLETED,
    ON_TOGGLE_ALL,
    ON_ITEM_EDIT,
    ON_ITEM_EDIT_BLUR,
    ON_ITEM_EDIT_KEY_PRESS,
    ON_ITEM_EDIT_KEY_UP,
    ON_ITEM_REMOVE,
    ON_ITEM_TOGGLE,
};

// helpers.js's `$on`.
static void on(node_t target, const char *type, uint32_t callback, bool capture) {
    script_string_t name = s(type);
    OK(machopo_dom_dom_event_target_add_event_listener(target, &name, callback, capture, &err_));
}

// helpers.js's `$delegate`, its test: whether `target` is one of the list's
// elements that match `selector`.
static bool delegated(node_t target, const char *selector) {
    script_string_t selectors = s(selector);
    exception_t err;
    bool matches = false;
    // Not an element: no match.
    return machopo_dom_dom_element_matches(target, &selectors, &matches, &err) && matches;
}

// `_itemId`: the id of the item `element` is in; -1 if it is in none.
static int item_id(node_t element) {
    script_string_t selectors = s("li"), name = s("data-id");
    option_element_t item;
    script_option_string_t id;
    char number[16] = "";
    OK(machopo_dom_dom_element_closest(element, &selectors, &item, &err_));
    if (!item.is_some) return -1;
    OK(machopo_dom_dom_element_get_attribute(item.val, &name, &id, &err_));
    if (id.is_some) take(&id.val, number, sizeof number);
    drop(item.val);
    return number[0] ? atoi(number) : -1;
}

static bool has_attribute(node_t element, const char *attribute) {
    script_string_t name = s(attribute);
    script_option_string_t value;
    OK(machopo_dom_dom_element_get_attribute(element, &name, &value, &err_));
    if (value.is_some) script_string_free(&value.val);
    return value.is_some;
}

// An input's value, copied.
static void value_of(node_t input, char *out, size_t size) {
    script_string_t value;
    OK(machopo_dom_dom_html_input_element_value(input, &value, &err_));
    take(&value, out, size);
}

// Whether the keyboard event's key is `key` (the JS app compares
// `keyCode`, which the guest has no member for).
static bool key_is(script_object_t event, const char *key) {
    script_string_t text;
    char name[16];
    OK(machopo_dom_dom_keyboard_event_key(event, &text, &err_));
    take(&text, name, sizeof name);
    return strcmp(name, key) == 0;
}

// How many listeners are running, one within another.
static int listeners_running;

void exports_script_handle_event(uint32_t callback, script_object_t event) {
    listeners_running++;
    char title[MAX_TITLE];
    machopo_dom_dom_option_object_t found;
    OK(machopo_dom_dom_event_target(event, &found, &err_));
    // The element the listener is called on (`this`), for the list's.
    node_t target = found.is_some ? found.val : 0;
    int id;

    switch (callback) {
    case ON_LOAD:
    case ON_HASH_CHANGE:
        set_view();
        break;
    case ON_NEW_TODO:
        value_of(new_todo, title, sizeof title);
        add_item(title);
        break;
    case ON_REMOVE_COMPLETED:
        remove_completed_items();
        break;
    case ON_TOGGLE_ALL: {
        bool checked;
        OK(machopo_dom_dom_html_element_click(toggle_all_input, &err_));
        OK(machopo_dom_dom_html_input_element_checked(toggle_all_input, &checked, &err_));
        toggle_all_to(checked);
        break;
    }
    case ON_ITEM_EDIT:
        if (target && delegated(target, "li label") && (id = item_id(target)) >= 0) edit_item(id);
        break;
    case ON_ITEM_EDIT_BLUR:
        if (target && delegated(target, "li .edit") && !has_attribute(target, "data-iscanceled")
            && (id = item_id(target)) >= 0) {
            value_of(target, title, sizeof title);
            edit_item_save(id, title);
        }
        break;
    case ON_ITEM_EDIT_KEY_PRESS:
        // Remove the cursor from the input when you hit enter just like if
        // it were a real form.
        if (target && delegated(target, "li .edit") && key_is(event, "Enter")) {
            OK(machopo_dom_dom_html_element_blur(target, &err_));
        }
        break;
    case ON_ITEM_EDIT_KEY_UP:
        if (target && delegated(target, "li .edit") && key_is(event, "Escape")) {
            set_attribute(target, "data-iscanceled", "true");
            OK(machopo_dom_dom_html_element_blur(target, &err_));
            if ((id = item_id(target)) >= 0) edit_item_cancel(id);
        }
        break;
    case ON_ITEM_REMOVE:
        if (target && delegated(target, ".destroy") && (id = item_id(target)) >= 0) remove_item(id);
        break;
    case ON_ITEM_TOGGLE:
        if (target && delegated(target, ".toggle") && (id = item_id(target)) >= 0) {
            bool checked;
            OK(machopo_dom_dom_html_input_element_checked(target, &checked, &err_));
            toggle_complete(id, checked, false);
        }
        break;
    }
    if (target) drop(target);
    // A listener called within another's call (a blur this guest made)
    // leaves the handles to the outer one, which may still use them.
    if (--listeners_running == 0) release_dropped();
}

// app.js: the view's elements, the controller's listeners (in the order
// the JS app adds them), and the window's.
script_callback_code_t exports_script_start(node_t page) {
    document = page;
    machopo_dom_dom_option_window_t view;
    OK(machopo_dom_dom_document_default_view(document, &view, &err_));
    if (!view.is_some) __builtin_trap();
    window = view.val;

    todo_list = must(".todo-list");
    item_counter = must(".todo-count");
    clear_completed = must(".clear-completed");
    main_block = must(".main");
    footer = must(".footer");
    toggle_all_input = must(".toggle-all");
    toggle_all = must(".toggle-all-label");
    new_todo = must(".new-todo");

    // The tests' page that shows todos without input: the list says so.
    bool seeded = has_attribute(todo_list, "data-seed");
    if (seeded) {
        model_create("Write the guest in C", true);
        model_create("Give it events", false);
        model_create("Take the todos away again", false);
    }

    on(new_todo, "change", ON_NEW_TODO, false);
    on(todo_list, "dblclick", ON_ITEM_EDIT, false);
    on(todo_list, "blur", ON_ITEM_EDIT_BLUR, true);
    on(todo_list, "keypress", ON_ITEM_EDIT_KEY_PRESS, false);
    on(todo_list, "keyup", ON_ITEM_EDIT_KEY_UP, false);
    on(todo_list, "click", ON_ITEM_REMOVE, false);
    on(todo_list, "click", ON_ITEM_TOGGLE, false);
    on(clear_completed, "click", ON_REMOVE_COMPLETED, false);
    on(toggle_all, "click", ON_TOGGLE_ALL, false);
    on(window, "load", ON_LOAD, false);
    on(window, "hashchange", ON_HASH_CHANGE, false);

    // Shown as the script runs, not at the window's `load` as the JS app
    // shows its own: a frame taken as the page loads has them.
    if (seeded) set_view();
    release_dropped();

    exports_script_start_return();
    return SCRIPT_CALLBACK_CODE_EXIT;
}

// Never called: `start` exits from its first call.
script_callback_code_t exports_script_start_callback(script_event_t *event) {
    (void)event;
    return SCRIPT_CALLBACK_CODE_EXIT;
}
