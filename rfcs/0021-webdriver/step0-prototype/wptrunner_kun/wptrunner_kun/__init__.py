"""machopo as a wptrunner product (RFC 0021's step 0).

Like Ladybird's (`wptrunner/browsers/ladybird.py`): wptrunner starts the
driver with `--port`, and the driver starts machopo for each session.
"""

from wptrunner.browsers.base import WebDriverBrowser, get_timeout_multiplier, require_arg
from wptrunner.executors import executor_kwargs as base_executor_kwargs
from wptrunner.executors.base import PytestExecutor
from wptrunner.executors.executorwebdriver import (
    WebDriverCrashtestExecutor,
    WebDriverRefTestExecutor,
    WebDriverTestharnessExecutor,
)
from wptrunner.products import Product


class MachopoBrowser(WebDriverBrowser):
    def make_command(self):
        return [self.webdriver_binary, "--port", str(self.port)] + self.webdriver_args


def check_args(**kwargs):
    require_arg(kwargs, "webdriver_binary")


def browser_kwargs(logger, test_type, run_info_data, config, subsuite, **kwargs):
    return {
        "binary": kwargs["binary"],
        "webdriver_binary": kwargs["webdriver_binary"],
        "webdriver_args": kwargs.get("webdriver_args"),
    }


def executor_kwargs(logger, test_type, test_environment, run_info_data, subsuite, **kwargs):
    rv = base_executor_kwargs(test_type, test_environment, run_info_data, subsuite, **kwargs)
    rv["capabilities"] = {}
    return rv


def get_product():
    return Product(
        name="machopo",
        browser_classes={None: MachopoBrowser},
        check_args=check_args,
        get_browser_kwargs=browser_kwargs,
        get_executor_kwargs=executor_kwargs,
        # Bind wptserve to 127.0.0.1 rather than WPT's domains, which only
        # machopo resolves (its host mapping rules), not this machine.
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
