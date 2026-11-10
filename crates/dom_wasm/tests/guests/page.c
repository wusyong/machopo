// dom_wasm's test guest for a page's script (the world `script`): `start`
// does what the document element's `data-case` says, and leaves what it
// did in the document for the test to read.
#include "script.h"

#include <stdio.h>
#include <string.h>
#include <time.h>

typedef machopo_dom_dom_node_t node_t;
typedef machopo_dom_dom_dom_exception_t exception_t;

// A string argument, borrowed.
static script_string_t s(const char *text) {
    script_string_t string;
    script_string_set(&string, text);
    return string;
}

// A `<p>` saying `text`, appended to `parent`.
static void say(node_t document, node_t parent, const char *text) {
    exception_t err;
    script_string_t tag = s("p"), content = s(text);
    node_t p, appended;
    if (!machopo_dom_dom_document_create_element(document, &tag, &p, &err)) return;
    if (!machopo_dom_dom_node_set_text_content(p, &content, &err)) return;
    machopo_dom_dom_node_append_child(parent, p, &appended, &err);
}

// What `start` left for its callback (the `yield` case).
static node_t yielded_document, yielded_root;
static int yields;

script_callback_code_t exports_script_start(node_t document) {
    exception_t err;
    machopo_dom_dom_option_element_t root;
    if (!machopo_dom_dom_document_document_element(document, &root, &err) || !root.is_some) {
        __builtin_trap();
    }
    script_string_t name = s("data-case");
    script_option_string_t given;
    if (!machopo_dom_dom_element_get_attribute(root.val, &name, &given, &err) || !given.is_some) {
        __builtin_trap();
    }
    char which[32] = {0};
    memcpy(which, given.val.ptr, given.val.len < sizeof which - 1 ? given.val.len : sizeof which - 1);
    script_string_free(&given.val);

    if (strcmp(which, "hello") == 0) {
        say(document, root.val, "hello from wasm");
    } else if (strcmp(which, "print") == 0) {
        printf("printed by the guest\n");
        fflush(stdout);
    } else if (strcmp(which, "trap") == 0) {
        say(document, root.val, "before the trap");
        __builtin_trap();
    } else if (strcmp(which, "sleep") == 0) {
        say(document, root.val, "before the wait");
        // 5 ms: the page goes on meanwhile.
        struct timespec time = {0, 5000000};
        nanosleep(&time, NULL);
        say(document, root.val, "after the wait");
    } else if (strcmp(which, "window") == 0) {
        // The document's window, the same each time, and back; its
        // location, read and its fragment set.
        machopo_dom_dom_option_window_t view, again;
        node_t back, location;
        if (!machopo_dom_dom_document_default_view(document, &view, &err) || !view.is_some
            || !machopo_dom_dom_document_default_view(document, &again, &err)
            || !machopo_dom_dom_window_document(view.val, &back, &err)
            || !machopo_dom_dom_window_location(view.val, &location, &err)) {
            __builtin_trap();
        }
        say(document, root.val, view.val == again.val && view.val != document ? "one window" : "windows");
        say(document, root.val, back == document ? "its document" : "another document");
        script_string_t text, fragment = s("#/active");
        if (!machopo_dom_dom_location_href(location, &text, &err)) __builtin_trap();
        char href[128] = {0};
        memcpy(href, text.ptr, text.len < sizeof href - 1 ? text.len : sizeof href - 1);
        script_string_free(&text);
        say(document, root.val, href);
        if (!machopo_dom_dom_location_set_hash(location, &fragment, &err)
            || !machopo_dom_dom_location_hash(location, &text, &err)) {
            __builtin_trap();
        }
        memset(href, 0, sizeof href);
        memcpy(href, text.ptr, text.len < sizeof href - 1 ? text.len : sizeof href - 1);
        script_string_free(&text);
        say(document, root.val, href);
        // A window is no node: a node's member on it is a TypeError.
        script_string_t name;
        bool is_node = machopo_dom_dom_node_node_name(view.val, &name, &err);
        say(document, root.val, !is_node && err.code == 1 ? "a window is no node" : "a window as a node");
    } else if (strcmp(which, "focus") == 0 || strcmp(which, "blur") == 0 || strcmp(which, "click") == 0) {
        // An input in the document: focused (and blurred again), or, a
        // checkbox, clicked.
        script_string_t tag = s("input"), type = s("type"), checkbox = s("checkbox");
        node_t field, appended;
        if (!machopo_dom_dom_document_create_element(document, &tag, &field, &err)
            || !machopo_dom_dom_node_append_child(root.val, field, &appended, &err)) {
            __builtin_trap();
        }
        if (strcmp(which, "click") == 0) {
            bool checked = false;
            if (!machopo_dom_dom_element_set_attribute(field, &type, &checkbox, &err)
                || !machopo_dom_dom_html_element_click(field, &err)
                || !machopo_dom_dom_html_input_element_checked(field, &checked, &err)) {
                __builtin_trap();
            }
            say(document, root.val, checked ? "checked" : "not checked");
        } else {
            if (!machopo_dom_dom_html_element_focus(field, &err)) __builtin_trap();
            if (strcmp(which, "blur") == 0 && !machopo_dom_dom_html_element_blur(field, &err)) {
                __builtin_trap();
            }
            say(document, root.val, "done");
        }
    } else if (strcmp(which, "yield") == 0) {
        // Gives way twice before it is done: the host calls back each time.
        say(document, root.val, "before the yields");
        yielded_document = document;
        yielded_root = root.val;
        return SCRIPT_CALLBACK_CODE_YIELD;
    }
    exports_script_start_return();
    return SCRIPT_CALLBACK_CODE_EXIT;
}

// Called again after each yield (the other cases exit from `start`).
script_callback_code_t exports_script_start_callback(script_event_t *event) {
    (void)event;
    if (++yields < 2) {
        say(yielded_document, yielded_root, "after one yield");
        return SCRIPT_CALLBACK_CODE_YIELD;
    }
    say(yielded_document, yielded_root, "after two yields");
    exports_script_start_return();
    return SCRIPT_CALLBACK_CODE_EXIT;
}
