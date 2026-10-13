#!/usr/bin/env python3
"""Regression checks for extract_block.py's `Cut.render()` on scratch files
(no cargo). Run: python3 LLM/tools/test_extract_block.py

Each case pins one defect of the #339 review: the target equal to the host,
the Copy write-back lost on an early exit, a bare `return;` in a `()` host,
`?` next to `return Err(..)`, and closures inside a `--loop` range.
"""

import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(__file__))
import extract_block as eb  # noqa: E402


def cut(text, start, end, **kw):
    """A Cut over a scratch file holding `text`, parameters set by hand."""
    d = os.path.join(tempfile.mkdtemp(), "crates", "x", "src")
    os.makedirs(d)
    path = os.path.join(d, "host.rs")
    open(path, "w").write(text)
    params = kw.pop("params", [])
    outs = kw.pop("outs", [])
    target = kw.pop("target", path)
    c = eb.Cut(path, start, end, "helper", target, "doc", "x", **kw)
    c.params = [list(p) for p in params]
    c.outs = [list(o) for o in outs]
    return c, path


class ExtractBlock(unittest.TestCase):
    def test_target_is_host(self):
        src = "fn host() {\n    let a = 1;\n    use_it(a);\n}\n\nfn other() {}\n"
        c, path = cut(src, 3, 3, params=[("a", "u32", "val")])
        c.render(typed=True)
        out = open(path).read()
        self.assertIn("helper(a);", out)
        self.assertIn("pub(crate) fn helper(a: u32)", out)
        self.assertLess(out.index("pub(crate) fn helper"), out.index("fn other"))
        self.assertGreater(out.index("pub(crate) fn helper"), out.index("fn host"))
        self.assertEqual(out.count("use_it(a);"), 1)

    def test_copy_writeback_on_early_exit(self):
        src = ("fn host() {\n    let mut flag = true;\n    for i in 0..3 {\n"
               "        'l: {\n            if i > 1 {\n                flag = false;\n"
               "                continue;\n            }\n            flag = true;\n        }\n    }\n}\n")
        c, path = cut(src, 5, 9, loop="l", indent=12,
                      params=[("flag", "bool", "mut")])
        c.render(typed=True)
        out = open(path).read()
        self.assertIn("*flag_out = flag;", out.split("pub(crate) fn helper")[1].split("return ControlFlow::Break")[0])
        self.assertIn("{ *flag_out = flag; return ControlFlow::Break(()) }", out)

    def test_bare_return_in_unit_host(self):
        src = ("fn host(x: u32) {\n    if x == 0 {\n        return;\n    }\n    work(x);\n}\n")
        c, path = cut(src, 2, 4, ret="()", params=[("x", "u32", "val")])
        c.render(typed=True)
        out = open(path).read()
        self.assertIn("if helper(x).is_break() {\n        return;\n    }", out)
        self.assertIn("return ControlFlow::Break(())", out)
        self.assertIn("ControlFlow::Continue(())", out)
        self.assertIn("-> ControlFlow<()>", out)

    def test_try_keeps_return_err(self):
        src = ("fn host() -> Result<(), String> {\n    let v = parse()?;\n"
               "    if v == 0 {\n        return Err(\"zero\".into());\n    }\n    Ok(())\n}\n")
        c, path = cut(src, 2, 5, err="String", ret="Result<(), String>")
        self.assertEqual(c.err, "String")
        self.assertFalse(c.returns)
        c.render(typed=True)
        out = open(path).read()
        self.assertIn("return Err(\"zero\".into());", out)
        self.assertNotIn("Some(Err", out)
        self.assertIn("helper()?;", out)

    def test_try_refuses_return_ok(self):
        src = ("fn host() -> Result<u8, String> {\n    let v = parse()?;\n"
               "    if v == 0 {\n        return Ok(1);\n    }\n    Ok(0)\n}\n")
        with self.assertRaises(AssertionError):
            cut(src, 2, 5, err="String", ret="Result<u8, String>")

    def test_closure_returns_do_not_trip_loop_checks(self):
        src = ("fn host(v: &[u32]) {\n    for x in v {\n"
               "        let any = v.iter().any(|y| {\n            if *y == 0 {\n                return false;\n"
               "            }\n            *y == *x\n        });\n        use_it(any);\n    }\n}\n")
        c, _ = cut(src, 3, 9, loop="l", indent=8)
        self.assertFalse(c.fallible)


if __name__ == "__main__":
    unittest.main()
