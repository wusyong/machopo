"""machopo as a wptrunner product.

As Ladybird's (`wptrunner/browsers/ladybird.py`): wptrunner starts the
driver, `machopodriver --port 0 --binary MACHOPO` (it picks a free port and
prints it), and the driver starts machopo for each session. machopo reaches wptserve as other browsers do, at WPT's domains
over HTTP and HTTPS: the driver passes it the host mapping rules that send
them to this machine, and WPT's root certificate to trust (`machopo --help`),
as Chrome's product passes `--host-resolver-rules` and the certificate's
SPKI.

wptrunner is imported when wptrunner asks for the product, not with the
package: `python -m wptrunner_machopo.run` puts it on `sys.path` first.
"""

import re

# machopo's rules for WPT's domains: `nonexistent` ones don't resolve, the
# others are this machine, where wptserve listens.
HOST_RESOLVER_RULES = "MAP nonexistent.*.test ^NOTFOUND, MAP *.test 127.0.0.1"

# What machopodriver prints once it listens: `machopodriver listening on 127.0.0.1:N`.
LISTENING = re.compile(r"machopodriver listening on \S+:(\d+)\s*$")


def check_args(**kwargs):
    from wptrunner.browsers.base import require_arg

    require_arg(kwargs, "webdriver_binary")
    require_arg(kwargs, "binary")


def machopo_args(ca_cert_path):
    """The driver's arguments that pass machopo WPT's network settings, and the
    test fonts (Ahem, and Lato for every generic family): results that don't
    change with the platform or the fonts a machine has, as the expectations
    assume."""
    args = ["--test-fonts", "--host-resolver-rules", HOST_RESOLVER_RULES]
    if ca_cert_path:
        args += ["--additional-trust-anchors", ca_cert_path]
    return [part for arg in args for part in ("--binary-arg", arg)]


def browser_kwargs(logger, test_type, run_info_data, config, subsuite, **kwargs):
    # A subsuite's own arguments for machopo (`binary_args` in its config, as
    # Chrome's product takes them): `--scale 2` for machopo's reftests at 2x.
    subsuite_args = (subsuite.config if subsuite else {}).get("binary_args", [])
    return {
        "binary": kwargs["binary"],
        "webdriver_binary": kwargs["webdriver_binary"],
        "webdriver_args": machopo_args(kwargs.get("ca_cert_path"))
        + [part for arg in subsuite_args for part in ("--binary-arg", arg)]
        + list(kwargs.get("webdriver_args") or []),
    }


def executor_kwargs(logger, test_type, test_environment, run_info_data, subsuite, **kwargs):
    from wptrunner.executors import executor_kwargs as base_executor_kwargs

    rv = base_executor_kwargs(test_type, test_environment, run_info_data, subsuite, **kwargs)
    rv["capabilities"] = {}
    # The driver's machopo is windowless (`--automation-pipe`): wdspec's
    # `is_wayland_headful` must not take its window for a Wayland one, which
    # the compositor won't let move.
    rv["headless"] = True
    return rv


def get_product():
    import threading
    import time

    from wptrunner.browsers.base import OutputHandler, WebDriverBrowser, get_timeout_multiplier
    from wptrunner.executors.base import PytestExecutor
    from wptrunner.executors.executorwebdriver import (
        WebDriverCrashtestExecutor,
        WebDriverRefTestExecutor,
        WebDriverTestharnessExecutor,
    )
    from wptrunner.products import Product

    class MachopoBrowser(WebDriverBrowser):
        """machopodriver binds a free port itself (`--port 0`) and prints it;
        the driver's port is the one in that line. wptrunner's own choice
        binds a free port, closes it and hands the number to the driver, and
        in that gap another of its processes can take it: the driver then
        fails to start, and wptrunner tries the same port again each time it
        restarts it (or, if the other process listens there, talks to that
        process's driver)."""

        def __init__(self, *args, **kwargs):
            super().__init__(*args, **kwargs)
            self._port_announced = threading.Event()

        def make_command(self):
            return [
                self.webdriver_binary,
                "--port",
                "0",
                "--binary",
                self.binary,
            ] + self.webdriver_args

        def start(self, group_metadata, **kwargs):
            # A new driver, a new port.
            self._port = None
            self._port_announced.clear()
            super().start(group_metadata, **kwargs)

        def create_output_handler(self, cmd):
            browser = self

            class PortHandler(OutputHandler):
                def __call__(self, line):
                    if not browser._port_announced.is_set():
                        if m := LISTENING.match(line.decode("utf8", "replace")):
                            browser._port = int(m.group(1))
                            browser._port_announced.set()
                    super().__call__(line)

            return PortHandler(self.logger, cmd)

        @property
        def port(self):
            # wptrunner waits for the driver at this port right after starting
            # it: wait until the driver has said which.
            while self._port is None:
                if self._port_announced.wait(0.1):
                    break
                if not self.is_alive() or time.time() > (self.init_deadline or 0):
                    raise OSError("machopodriver exited or didn't say which port it listens on")
            return self._port

    return Product(
        name="machopo",
        browser_classes={None: MachopoBrowser},
        check_args=check_args,
        get_browser_kwargs=browser_kwargs,
        get_executor_kwargs=executor_kwargs,
        # wptserve binds to 127.0.0.1: WPT's domains are machopo's to resolve,
        # not this machine's.
        env_options={"server_host": "127.0.0.1"},
        get_env_extras=lambda **kwargs: [],
        get_timeout_multiplier=get_timeout_multiplier,
        executor_classes={
            "testharness": WebDriverTestharnessExecutor,
            "reftest": WebDriverRefTestExecutor,
            "wdspec": PytestExecutor,
            "crashtest": WebDriverCrashtestExecutor,
        },
    )
