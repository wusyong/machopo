// dom_wasm's test guest: each case calls the DOM through
// machopo:dom and says what went wrong ("" if nothing did); the cases
// that trap are the tests' to expect.
#include "test.h"

#include <stdio.h>
#include <string.h>
#include <time.h>

typedef machopo_dom_dom_node_t node_t;
typedef machopo_dom_dom_dom_exception_t exception_t;

static char failure[256];

#define FAIL(...)                                          \
    do {                                                   \
        snprintf(failure, sizeof failure, __VA_ARGS__);    \
        return;                                            \
    } while (0)

#define CHECK(cond)                                                    \
    do {                                                               \
        if (!(cond)) FAIL("line %d: %s", __LINE__, #cond);             \
    } while (0)

// A call that must succeed: `err` is the case's exception.
#define OK(call)                                                               \
    do {                                                                       \
        if (!(call)) {                                                         \
            unsigned code_ = err.code;                                         \
            machopo_dom_dom_dom_exception_free(&err);                         \
            FAIL("line %d: %s failed with code %u", __LINE__, #call, code_);   \
        }                                                                      \
    } while (0)

// A call that must fail with `code`.
#define FAILS(call, want)                                                      \
    do {                                                                       \
        if (call) FAIL("line %d: %s didn't fail", __LINE__, #call);            \
        unsigned code_ = err.code;                                             \
        machopo_dom_dom_dom_exception_free(&err);                             \
        if (code_ != (want)) FAIL("line %d: %s failed with code %u, not %u",   \
                                  __LINE__, #call, code_, (unsigned)(want));   \
    } while (0)

// A string argument, borrowed.
static test_string_t s(const char *text) {
    test_string_t string;
    test_string_set(&string, text);
    return string;
}

// Whether `got` (the guest's, freed here) is `want`.
static bool is(test_string_t *got, const char *want) {
    bool same = got->len == strlen(want) && memcmp(got->ptr, want, got->len) == 0;
    test_string_free(got);
    return same;
}

// The node `option` holds, or 0 for none.
static node_t some(machopo_dom_dom_option_node_t option) {
    return option.is_some ? option.val : 0;
}

static node_t element(node_t document, const char *name) {
    test_string_t local_name = s(name);
    node_t made = 0;
    exception_t err;
    if (!machopo_dom_dom_document_create_element(document, &local_name, &made, &err)) {
        machopo_dom_dom_dom_exception_free(&err);
    }
    return made;
}

// The tree: making, inserting, walking and removing nodes, a node the
// same number each time.
static void tree(void) {
    exception_t err;
    node_t document = machopo_dom_dom_create_document();
    node_t div = element(document, "div");
    CHECK(document != 0 && div != 0 && div != document);
    test_string_t data = s("h\xc3\xa9llo");
    node_t text;
    OK(machopo_dom_dom_document_create_text_node(document, &data, &text, &err));
    node_t appended;
    OK(machopo_dom_dom_node_append_child(div, text, &appended, &err));
    CHECK(appended == text);
    machopo_dom_dom_option_node_t child;
    OK(machopo_dom_dom_node_first_child(div, &child, &err));
    CHECK(some(child) == text);
    OK(machopo_dom_dom_node_last_child(div, &child, &err));
    CHECK(some(child) == text);
    OK(machopo_dom_dom_node_parent_node(text, &child, &err));
    CHECK(some(child) == div);
    test_string_t name;
    OK(machopo_dom_dom_node_node_name(div, &name, &err));
    CHECK(is(&name, "DIV"));
    uint32_t type;
    OK(machopo_dom_dom_node_node_type(text, &type, &err));
    CHECK(type == 3);
    OK(machopo_dom_dom_node_node_type(document, &type, &err));
    CHECK(type == 9);
    test_option_string_t content;
    OK(machopo_dom_dom_node_text_content(div, &content, &err));
    CHECK(content.is_some && is(&content.val, "h\xc3\xa9llo"));
    OK(machopo_dom_dom_node_text_content(document, &content, &err));
    CHECK(!content.is_some);
    bool yes;
    OK(machopo_dom_dom_node_has_child_nodes(div, &yes, &err));
    CHECK(yes);
    OK(machopo_dom_dom_node_contains(div, &text, &yes, &err));
    CHECK(yes);
    OK(machopo_dom_dom_node_contains(div, NULL, &yes, &err));
    CHECK(!yes);
    machopo_dom_dom_option_document_t owner;
    OK(machopo_dom_dom_node_owner_document(div, &owner, &err));
    CHECK(owner.is_some && owner.val == document);
    OK(machopo_dom_dom_node_is_connected(text, &yes, &err));
    CHECK(!yes);
    OK(machopo_dom_dom_node_append_child(document, div, &appended, &err));
    machopo_dom_dom_option_element_t root;
    OK(machopo_dom_dom_document_document_element(document, &root, &err));
    CHECK(root.is_some && root.val == div);
    OK(machopo_dom_dom_node_is_connected(text, &yes, &err));
    CHECK(yes);
    OK(machopo_dom_dom_node_parent_element(text, &root, &err));
    CHECK(root.is_some && root.val == div);
    node_t removed;
    OK(machopo_dom_dom_node_remove_child(div, text, &removed, &err));
    CHECK(removed == text);
    OK(machopo_dom_dom_node_first_child(div, &child, &err));
    CHECK(!child.is_some);
    node_t span = element(document, "span");
    OK(machopo_dom_dom_node_append_child(div, span, &appended, &err));
    OK(machopo_dom_dom_node_insert_before(div, text, &span, &appended, &err));
    OK(machopo_dom_dom_node_first_child(div, &child, &err));
    CHECK(some(child) == text);
    OK(machopo_dom_dom_node_next_sibling(text, &child, &err));
    CHECK(some(child) == span);
    OK(machopo_dom_dom_node_previous_sibling(span, &child, &err));
    CHECK(some(child) == text);
    uint32_t count;
    OK(machopo_dom_dom_element_child_element_count(div, &count, &err));
    CHECK(count == 1);
    OK(machopo_dom_dom_element_first_element_child(div, &root, &err));
    CHECK(root.is_some && root.val == span);
    OK(machopo_dom_dom_document_first_element_child(document, &root, &err));
    CHECK(root.is_some && root.val == div);
    // An element's handle is its node's: the node's members take it.
    uint32_t length;
    OK(machopo_dom_dom_character_data_length(text, &length, &err));
    CHECK(length == 5);
    machopo_dom_dom_release(document);
    machopo_dom_dom_release(div);
    machopo_dom_dom_release(text);
    machopo_dom_dom_release(span);
}

// Strings both ways: attributes, a text's data, names.
static void strings(void) {
    exception_t err;
    node_t document = machopo_dom_dom_create_document();
    node_t div = element(document, "DIV");
    test_string_t name = s("data-x"), value = s("h\xc3\xa9llo \xe6\x97\xa5\xe6\x9c\xac");
    OK(machopo_dom_dom_element_set_attribute(div, &name, &value, &err));
    test_option_string_t got;
    OK(machopo_dom_dom_element_get_attribute(div, &name, &got, &err));
    CHECK(got.is_some && is(&got.val, "h\xc3\xa9llo \xe6\x97\xa5\xe6\x9c\xac"));
    test_string_t missing = s("missing");
    OK(machopo_dom_dom_element_get_attribute(div, &missing, &got, &err));
    CHECK(!got.is_some);
    bool yes;
    OK(machopo_dom_dom_element_has_attribute(div, &name, &yes, &err));
    CHECK(yes);
    OK(machopo_dom_dom_element_has_attributes(div, &yes, &err));
    CHECK(yes);
    OK(machopo_dom_dom_element_remove_attribute(div, &name, &err));
    OK(machopo_dom_dom_element_has_attribute(div, &name, &yes, &err));
    CHECK(!yes);
    test_string_t id = s("main"), class_name = s("a b");
    OK(machopo_dom_dom_element_set_id(div, &id, &err));
    OK(machopo_dom_dom_element_set_class_name(div, &class_name, &err));
    test_string_t read;
    OK(machopo_dom_dom_element_id(div, &read, &err));
    CHECK(is(&read, "main"));
    OK(machopo_dom_dom_element_class_name(div, &read, &err));
    CHECK(is(&read, "a b"));
    OK(machopo_dom_dom_element_local_name(div, &read, &err));
    CHECK(is(&read, "div"));
    OK(machopo_dom_dom_element_tag_name(div, &read, &err));
    CHECK(is(&read, "DIV"));
    OK(machopo_dom_dom_element_namespace_uri(div, &got, &err));
    CHECK(got.is_some && is(&got.val, "http://www.w3.org/1999/xhtml"));
    OK(machopo_dom_dom_element_prefix(div, &got, &err));
    CHECK(!got.is_some);
    test_string_t svg = s("http://www.w3.org/2000/svg"), rect = s("svg:rect");
    node_t made;
    OK(machopo_dom_dom_document_create_element_ns(document, &svg, &rect, &made, &err));
    OK(machopo_dom_dom_element_prefix(made, &got, &err));
    CHECK(got.is_some && is(&got.val, "svg"));
    OK(machopo_dom_dom_element_tag_name(made, &read, &err));
    CHECK(is(&read, "svg:rect"));
    test_string_t local = s("rect");
    test_string_t fill = s("fill"), red = s("red");
    OK(machopo_dom_dom_element_set_attribute_ns(made, NULL, &fill, &red, &err));
    OK(machopo_dom_dom_element_get_attribute_ns(made, NULL, &fill, &got, &err));
    CHECK(got.is_some && is(&got.val, "red"));
    OK(machopo_dom_dom_element_has_attribute_ns(made, NULL, &fill, &yes, &err));
    CHECK(yes);
    OK(machopo_dom_dom_element_remove_attribute_ns(made, NULL, &fill, &err));
    OK(machopo_dom_dom_element_has_attribute_ns(made, &svg, &local, &yes, &err));
    CHECK(!yes);
    test_string_t data = s("\xf0\x9f\x98\x80");
    node_t text;
    OK(machopo_dom_dom_document_create_text_node(document, &data, &text, &err));
    uint32_t length;
    OK(machopo_dom_dom_character_data_length(text, &length, &err));
    CHECK(length == 2);
    // `[LegacyNullToEmptyString]`: none is the empty string.
    OK(machopo_dom_dom_character_data_set_data(text, NULL, &err));
    OK(machopo_dom_dom_character_data_data(text, &read, &err));
    CHECK(is(&read, ""));
    test_string_t abc = s("abc");
    OK(machopo_dom_dom_node_set_text_content(div, &abc, &err));
    test_option_string_t content;
    OK(machopo_dom_dom_node_text_content(div, &content, &err));
    CHECK(content.is_some && is(&content.val, "abc"));
    OK(machopo_dom_dom_node_set_text_content(div, NULL, &err));
    OK(machopo_dom_dom_node_has_child_nodes(div, &yes, &err));
    CHECK(!yes);
}

// The DOM's errors, by their codes.
static void errors(void) {
    exception_t err;
    node_t document = machopo_dom_dom_create_document();
    node_t div = element(document, "div");
    node_t span = element(document, "span");
    node_t appended;
    FAILS(machopo_dom_dom_node_append_child(div, document, &appended, &err), 2);
    test_string_t bad = s("1x");
    node_t made;
    FAILS(machopo_dom_dom_document_create_element(document, &bad, &made, &err), 4);
    node_t removed;
    FAILS(machopo_dom_dom_node_remove_child(div, span, &removed, &err), 3);
    test_string_t prefixed = s("a:b");
    FAILS(machopo_dom_dom_document_create_element_ns(document, NULL, &prefixed, &made, &err), 9);
}

// A node of another interface for `this` is a TypeError, code 1.
static void type_errors(void) {
    exception_t err;
    node_t document = machopo_dom_dom_create_document();
    node_t div = element(document, "div");
    test_string_t data = s("t"), name = s("id");
    node_t text;
    OK(machopo_dom_dom_document_create_text_node(document, &data, &text, &err));
    test_option_string_t got;
    FAILS(machopo_dom_dom_element_get_attribute(text, &name, &got, &err), 1);
    test_string_t read;
    FAILS(machopo_dom_dom_character_data_data(div, &read, &err), 1);
    node_t made;
    FAILS(machopo_dom_dom_document_create_element(div, &name, &made, &err), 1);
}

// Released, a node's slot gives its next node another number.
static void release(void) {
    exception_t err;
    node_t document = machopo_dom_dom_create_document();
    node_t div = element(document, "div");
    node_t appended;
    OK(machopo_dom_dom_node_append_child(document, div, &appended, &err));
    machopo_dom_dom_release(div);
    machopo_dom_dom_option_node_t child;
    OK(machopo_dom_dom_node_first_child(document, &child, &err));
    CHECK(child.is_some && child.val != div);
}

// Handles kept past the call (`release-kept` lets them go).
static node_t kept[3];

static void keep(void) {
    node_t document = machopo_dom_dom_create_document();
    kept[0] = document;
    kept[1] = element(document, "p");
    kept[2] = element(document, "p");
    CHECK(kept[1] != kept[2]);
}

static void release_kept(void) {
    for (int i = 0; i < 3; i++) {
        machopo_dom_dom_release(kept[i]);
    }
}

// A kept node, read after a GC: its name.
static void read_kept(void) {
    exception_t err;
    test_string_t name;
    OK(machopo_dom_dom_node_node_name(kept[1], &name, &err));
    CHECK(is(&name, "P"));
}

// These trap: a released handle, a number never given, 0, a string that
// isn't UTF-8.
static void stale(void) {
    node_t document = machopo_dom_dom_create_document();
    machopo_dom_dom_release(document);
    uint32_t type;
    exception_t err;
    machopo_dom_dom_node_node_type(document, &type, &err);
    FAIL("a released handle didn't trap");
}

static void never_given(void) {
    uint32_t type;
    exception_t err;
    machopo_dom_dom_node_node_type(0x00abcdef, &type, &err);
    FAIL("a number never given didn't trap");
}

static void zero(void) {
    node_t document = machopo_dom_dom_create_document();
    node_t appended;
    exception_t err;
    machopo_dom_dom_node_append_child(document, 0, &appended, &err);
    FAIL("0 as an argument didn't trap");
}

static void release_twice(void) {
    node_t document = machopo_dom_dom_create_document();
    machopo_dom_dom_release(document);
    machopo_dom_dom_release(document);
    FAIL("releasing a handle twice didn't trap");
}

static void release_never_given(void) {
    machopo_dom_dom_release(0x00abcdef);
    FAIL("releasing a number never given didn't trap");
}

static void bad_utf8(void) {
    node_t document = machopo_dom_dom_create_document();
    test_string_t bad = {(uint8_t *)"\xff\xfe", 2};
    node_t made;
    exception_t err;
    machopo_dom_dom_document_create_element(document, &bad, &made, &err);
    FAIL("a string that isn't UTF-8 didn't trap");
}

// Whether `sleep` has come back from its wait.
static bool woke;

// libc's sleep is the clock's `wait-for`, which blocks this task: the
// host runs the store again when the time has come.
test_callback_code_t exports_test_sleep(uint64_t nanos) {
    struct timespec time = {(time_t)(nanos / 1000000000), (long)(nanos % 1000000000)};
    nanosleep(&time, NULL);
    woke = true;
    exports_test_sleep_return();
    return TEST_CALLBACK_CODE_EXIT;
}

// Never called: `sleep` exits from its first call.
test_callback_code_t exports_test_sleep_callback(test_event_t *event) {
    (void)event;
    return TEST_CALLBACK_CODE_EXIT;
}

static void is_awake(void) { CHECK(woke); }

static void is_asleep(void) { CHECK(!woke); }

// A line to each output, and one with no newline at its end.
static void print(void) {
    printf("to stdout\n");
    fflush(stdout);
    fprintf(stderr, "to stderr\nno newline");
    fflush(stderr);
}

// The clock reads to 100 microseconds, and says so.
static void coarse_clock(void) {
    struct timespec time, resolution;
    CHECK(clock_gettime(CLOCK_MONOTONIC, &time) == 0);
    CHECK(time.tv_nsec % 100000 == 0);
    CHECK(time.tv_sec != 0 || time.tv_nsec != 0);
    CHECK(clock_getres(CLOCK_MONOTONIC, &resolution) == 0);
    CHECK(resolution.tv_sec == 0 && resolution.tv_nsec == 100000);
}

static const struct {
    const char *name;
    void (*run)(void);
} CASES[] = {
    {"tree", tree},
    {"strings", strings},
    {"errors", errors},
    {"type-errors", type_errors},
    {"release", release},
    {"keep", keep},
    {"release-kept", release_kept},
    {"read-kept", read_kept},
    {"stale", stale},
    {"never-given", never_given},
    {"zero", zero},
    {"release-twice", release_twice},
    {"release-never-given", release_never_given},
    {"bad-utf8", bad_utf8},
    {"is-awake", is_awake},
    {"is-asleep", is_asleep},
    {"print", print},
    {"coarse-clock", coarse_clock},
};

void exports_test_run(test_string_t *name, test_string_t *ret) {
    failure[0] = 0;
    bool found = false;
    for (size_t i = 0; i < sizeof CASES / sizeof CASES[0]; i++) {
        if (name->len == strlen(CASES[i].name) && memcmp(name->ptr, CASES[i].name, name->len) == 0) {
            found = true;
            CASES[i].run();
        }
    }
    if (!found) snprintf(failure, sizeof failure, "no case %.*s", (int)name->len, (const char *)name->ptr);
    test_string_free(name);
    test_string_dup(ret, failure);
}
