// TodoMVC as a wasm guest (the world `script`), on the page
// `tests/scenarios/pages/todomvc-wasm.html`, whose markup and style sheets
// are the JS app's (`tests/sites/todomvc`). So far it only shows its
// todos: `start` builds each one's item as the JS app's template does and
// fills in the count; nothing can be done to them yet (a guest has no
// events).
#include "script.h"

#include <stdbool.h>
#include <stdio.h>

typedef machopo_dom_dom_node_t node_t;
typedef machopo_dom_dom_dom_exception_t exception_t;

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

static node_t document;

// The first element matching `selector`.
static node_t query(const char *selector) {
    script_string_t selectors = s(selector);
    machopo_dom_dom_option_element_t found;
    OK(machopo_dom_dom_document_query_selector(document, &selectors, &found, &err_));
    if (!found.is_some) __builtin_trap();
    return found.val;
}

// A new `<tag>`, the last child of `parent`.
static node_t append(node_t parent, const char *tag) {
    script_string_t name = s(tag);
    node_t element, appended;
    OK(machopo_dom_dom_document_create_element(document, &name, &element, &err_));
    OK(machopo_dom_dom_node_append_child(parent, element, &appended, &err_));
    return element;
}

static void set(node_t element, const char *attribute, const char *value) {
    script_string_t name = s(attribute), text = s(value);
    OK(machopo_dom_dom_element_set_attribute(element, &name, &text, &err_));
}

typedef struct {
    int id;
    const char *title;
    bool completed;
    int priority;
} todo_t;

static const todo_t TODOS[] = {
    {1, "Write the guest in C", true, 4},
    {2, "Give it events", false, 3},
    {3, "Take the todos away again", false, 2},
};

// The item of `todo`, as the JS app's template makes it:
//   <li data-id class data-priority>
//     <div class="view">
//       <input class="toggle" type="checkbox" [checked]>
//       <label>title</label>
//       <button class="destroy"></button>
static void show(node_t list, const todo_t *todo) {
    char number[16];
    node_t item = append(list, "li");
    snprintf(number, sizeof number, "%d", todo->id);
    set(item, "data-id", number);
    set(item, "class", todo->completed ? "completed" : "");
    snprintf(number, sizeof number, "%d", todo->priority);
    set(item, "data-priority", number);

    node_t view = append(item, "div");
    set(view, "class", "view");

    node_t toggle = append(view, "input");
    set(toggle, "class", "toggle");
    set(toggle, "type", "checkbox");
    if (todo->completed) set(toggle, "checked", "");

    node_t label = append(view, "label");
    script_string_t title = s(todo->title);
    OK(machopo_dom_dom_node_set_text_content(label, &title, &err_));

    node_t destroy = append(view, "button");
    set(destroy, "class", "destroy");

    // The guest keeps none of them: the tree does.
    machopo_dom_dom_release(destroy);
    machopo_dom_dom_release(label);
    machopo_dom_dom_release(toggle);
    machopo_dom_dom_release(view);
    machopo_dom_dom_release(item);
}

script_callback_code_t exports_script_start(node_t page) {
    document = page;
    node_t list = query(".todo-list");
    int left = 0;
    for (size_t i = 0; i < sizeof TODOS / sizeof TODOS[0]; i++) {
        show(list, &TODOS[i]);
        if (!TODOS[i].completed) left++;
    }
    machopo_dom_dom_release(list);

    // "<strong>2</strong> items left", as the JS app's counter.
    char count[64];
    snprintf(count, sizeof count, "<strong>%d</strong> item%s left", left, left == 1 ? "" : "s");
    node_t counter = query(".todo-count");
    script_string_t html = s(count);
    OK(machopo_dom_dom_element_set_inner_html(counter, &html, &err_));
    machopo_dom_dom_release(counter);

    exports_script_start_return();
    return SCRIPT_CALLBACK_CODE_EXIT;
}

// Never called: `start` exits from its first call.
script_callback_code_t exports_script_start_callback(script_event_t *event) {
    (void)event;
    return SCRIPT_CALLBACK_CODE_EXIT;
}
