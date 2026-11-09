"""The screenshots of a run's failing reftests, out of wptrunner's raw log
(`--reftest-screenshot fail` puts them there, base64 PNGs), as Chromium's
web tests leave them: for each, under DIR at the test's path,
`<test>-actual.png`, `<test>-expected.png` (the reference the comparison
that failed was with) and `<test>-diff.png` (the actual one, with the
pixels that differ in red).

`run.py` writes them after a run, into `screenshots/` under `--out`.
"""

import base64
import io
import json
import os
import shutil


def failing_comparisons(raw_log):
    """(test, actual PNG, expected PNG) for each reftest with screenshots
    in the log. They come as [test, "==" or "!=", reference, ...]: a chain
    adds a relation and a reference for each step, and its last two
    screenshots are the comparison that decided."""
    with open(raw_log, encoding="utf-8") as log:
        for line in log:
            entry = json.loads(line)
            if entry["action"] != "test_end":
                continue
            shots = (entry.get("extra") or {}).get("reftest_screenshots")
            if not shots or len(shots) < 3:
                continue
            yield entry["test"], base64.b64decode(shots[-3]["screenshot"]), base64.b64decode(shots[-1]["screenshot"])


def diff(actual, expected):
    """`actual` with the pixels that differ from `expected` in red."""
    from PIL import Image, ImageChops

    a = Image.open(io.BytesIO(actual)).convert("RGB")
    b = Image.open(io.BytesIO(expected)).convert("RGB")
    if a.size != b.size:
        b = b.resize(a.size)
    mask = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v else 0)
    out = a.copy()
    out.paste(Image.new("RGB", a.size, (255, 0, 0)), mask=mask)
    data = io.BytesIO()
    out.save(data, "PNG")
    return data.getvalue()


def write(raw_log, out_dir):
    """Writes them under `out_dir`, emptied first; returns how many tests."""
    shutil.rmtree(out_dir, ignore_errors=True)
    count = 0
    for test, actual, expected in failing_comparisons(raw_log):
        base = os.path.join(out_dir, *test.lstrip("/").split("?", 1)[0].split("/"))
        os.makedirs(os.path.dirname(base), exist_ok=True)
        for suffix, data in (("actual", actual), ("expected", expected), ("diff", diff(actual, expected))):
            with open(f"{base}-{suffix}.png", "wb") as f:
                f.write(data)
        count += 1
    return count
