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
