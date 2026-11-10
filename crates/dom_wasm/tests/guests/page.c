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

// The page's document and its root, for the listeners; the element the
// cases listen on, and the one a listener clicks (`CLICKS`).
static node_t page, page_root, subject, other;

// What a listener does beside saying it ran, in its callback's number.
enum {
    PREVENTS = 0x10,
    STOPS = 0x20,
    TRAPS = 0x40,
    CLICKS = 0x80,
    KEEPS = 0x100,
    PRINTS = 0x200,
};

// The handle a listener kept of its event (`KEEPS`), which was its for
// that call only.
static script_object_t kept;

static void listen(node_t target, const char *type, uint32_t callback, bool capture) {
    exception_t err;
    script_string_t name = s(type);
    if (!machopo_dom_dom_event_target_add_event_listener(target, &name, callback, capture, &err)) {
        __builtin_trap();
    }
}

// Says "<callback>:<type>", where the event is at ("at root", "at
// subject"), and what the event's own interface has.
void exports_script_handle_event(uint32_t callback, script_object_t event) {
    exception_t err;
    script_string_t text;
    machopo_dom_dom_option_object_t target, current;
    if (!machopo_dom_dom_event_type(event, &text, &err)
        || !machopo_dom_dom_event_target(event, &target, &err)
        || !machopo_dom_dom_event_current_target(event, &current, &err)) {
        __builtin_trap();
    }
    char type[32] = {0}, line[128];
    memcpy(type, text.ptr, text.len < sizeof type - 1 ? text.len : sizeof type - 1);
    script_string_free(&text);
    int length = snprintf(line, sizeof line, "%u:%s%s%s", (unsigned)callback, type,
                          target.is_some && target.val == subject ? " of subject" : "",
                          !current.is_some ? "" : current.val == page_root ? " at root"
                              : current.val == subject ? " at subject" : " elsewhere");
    if (strcmp(type, "click") == 0) {
        // A mouse's (a click the page fired itself is a plain event).
        int32_t button = -1, detail = -1;
        if (machopo_dom_dom_mouse_event_button(event, &button, &err)
            && machopo_dom_dom_ui_event_detail(event, &detail, &err)) {
            snprintf(line + length, sizeof line - (size_t)length, " button %d detail %d", (int)button,
                     (int)detail);
        }
    } else if (strcmp(type, "keydown") == 0) {
        if (!machopo_dom_dom_keyboard_event_key(event, &text, &err)) __builtin_trap();
        snprintf(line + length, sizeof line - (size_t)length, " key %.*s", (int)text.len, (const char *)text.ptr);
        script_string_free(&text);
    }
    say(page, page_root, line);
    if (callback & PRINTS) {
        printf("printed by a listener\n");
        fflush(stdout);
    }
    if (callback & KEEPS) {
        // The one before was the guest's for that call: a trap, by now.
        if (kept && machopo_dom_dom_event_type(kept, &text, &err)) {
            script_string_free(&text);
            say(page, page_root, "the handle kept lives");
        }
        kept = event;
    }
    if (callback & PREVENTS) {
        bool prevented = false;
        if (!machopo_dom_dom_event_prevent_default(event, &err)
            || !machopo_dom_dom_event_default_prevented(event, &prevented, &err)) {
            __builtin_trap();
        }
        say(page, page_root, prevented ? "prevented" : "not prevented");
    }
    if ((callback & STOPS) && !machopo_dom_dom_event_stop_propagation(event, &err)) __builtin_trap();
    if (callback & TRAPS) __builtin_trap();
    if (callback & CLICKS) {
        // Another dispatch within this one: its listeners first.
        if (!machopo_dom_dom_html_element_click(other, &err)) __builtin_trap();
        say(page, page_root, "after the click within");
    }
}

// An `<input type="checkbox">` with `id`, in the root.
static node_t checkbox(const char *id) {
    exception_t err;
    script_string_t tag = s("input"), type = s("type"), value = s("checkbox"), name = s("id"), given = s(id);
    node_t field, appended;
    if (!machopo_dom_dom_document_create_element(page, &tag, &field, &err)
        || !machopo_dom_dom_element_set_attribute(field, &type, &value, &err)
        || !machopo_dom_dom_element_set_attribute(field, &name, &given, &err)
        || !machopo_dom_dom_node_append_child(page_root, field, &appended, &err)) {
        __builtin_trap();
    }
    return field;
}

// The cases that listen: a checkbox (`subject`) and what listens to it.
// The ones that end in `-click` click it themselves, within `start`.
static bool listening(const char *which) {
    exception_t err;
    bool clicks = false;
    if (strcmp(which, "listen") == 0 || strcmp(which, "listen-click") == 0) {
        // In the root's capture, at the checkbox (twice the same: once),
        // in the root's bubble; and the keys, at the document.
        subject = checkbox("subject");
        listen(page_root, "click", 1, true);
        listen(subject, "click", 2, false);
        listen(subject, "click", 2, false);
        listen(page_root, "click", 3, false);
        listen(page, "keydown", 4, false);
        clicks = which[6] != 0;
    } else if (strcmp(which, "prevent-click") == 0) {
        subject = checkbox("subject");
        listen(subject, "click", 2 | PREVENTS, false);
        clicks = true;
    } else if (strcmp(which, "stop-click") == 0) {
        subject = checkbox("subject");
        listen(page_root, "click", 1 | STOPS, true);
        listen(subject, "click", 2, false);
        clicks = true;
    } else if (strcmp(which, "remove-click") == 0) {
        // Removed as it was added; one that names the other phase stays.
        subject = checkbox("subject");
        script_string_t click = s("click");
        listen(subject, "click", 6, false);
        listen(subject, "click", 2, false);
        listen(page_root, "click", 1, true);
        listen(page_root, "click", 3, false);
        if (!machopo_dom_dom_event_target_remove_event_listener(subject, &click, 2, false, &err)
            || !machopo_dom_dom_event_target_remove_event_listener(page_root, &click, 1, false, &err)
            || !machopo_dom_dom_event_target_remove_event_listener(page_root, &click, 3, false, &err)) {
            __builtin_trap();
        }
        clicks = true;
    } else if (strcmp(which, "listener-trap") == 0 || strcmp(which, "trap-click") == 0) {
        subject = checkbox("subject");
        listen(subject, "click", 2 | TRAPS, false);
        listen(page_root, "click", 3, false);
        clicks = which[0] == 't';
    } else if (strcmp(which, "keep") == 0 || strcmp(which, "keep-click") == 0) {
        subject = checkbox("subject");
        listen(subject, "click", 2 | KEEPS, false);
        clicks = which[4] != 0;
        if (clicks && !machopo_dom_dom_html_element_click(subject, &err)) __builtin_trap();
    } else if (strcmp(which, "nested") == 0) {
        subject = checkbox("subject");
        other = checkbox("other");
        listen(subject, "click", 2 | CLICKS, false);
        listen(other, "click", 5 | PRINTS, false);
    } else {
        return false;
    }
    if (clicks) {
        bool checked = false;
        if (!machopo_dom_dom_html_element_click(subject, &err)
            || !machopo_dom_dom_html_input_element_checked(subject, &checked, &err)) {
            __builtin_trap();
        }
        say(page, page_root, checked ? "clicked: checked" : "clicked: not checked");
    }
    return true;
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
    page = document;
    page_root = root.val;

    if (listening(which)) {
    } else if (strcmp(which, "hello") == 0) {
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
