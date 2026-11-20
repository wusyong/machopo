"""machopo's executors: wptrunner's WebDriver ones, which delete every
cookie after each test. Classes at the top of a module, which the test
runner's processes can import (wptrunner pickles them across)."""

from wptrunner.executors.executorwebdriver import (
    WebDriverCrashtestExecutor,
    WebDriverRefTestExecutor,
    WebDriverTestharnessExecutor,
)


class ClearsCookies:
    """Deletes every cookie after each test, as Chromium's web tests do
    (`WebTestControlHost::ResetBrowserAfterWebTest`): a test that stops
    before it cleans up (at `window.open`, say) leaves its cookies for
    the tests after it in the same browser, which isn't restarted when
    that result is the expected one. machopodriver's own command
    deletes those of every site; WebDriver's Delete All Cookies only
    the current document's."""

    def do_test(self, test):
        try:
            return super().do_test(test)
        finally:
            try:
                self.protocol.webdriver.send_session_command(
                    "POST", "machopo/cookies/clear", {}
                )
            except Exception as error:
                # A browser that has gone is restarted for the next
                # test, with no cookies.
                self.logger.warning(f"clearing the cookies: {error}")


class MachopoTestharnessExecutor(ClearsCookies, WebDriverTestharnessExecutor):
    pass


class MachopoRefTestExecutor(ClearsCookies, WebDriverRefTestExecutor):
    pass


class MachopoCrashtestExecutor(ClearsCookies, WebDriverCrashtestExecutor):
    pass
