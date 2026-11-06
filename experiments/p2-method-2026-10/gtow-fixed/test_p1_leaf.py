"""Regression checks for the experiment-only P1 leaf menus."""
import re
import unittest

from p1_leaf import MENUS

SIZE_LIST = re.compile(r"\b(?:bet|raise)\s*\[([^\]]*)\]")
EXPLICIT = re.compile(r"a|min|\d+(?:\.\d+)?(?:%|x|bb|c|%stack|%effective)|\d*e")


class MenuSizesTest(unittest.TestCase):
    def test_every_size_states_its_unit(self):
        # Since v0.2 a bare number is BB, so a pot percentage without `%` would silently become a BB size.
        for name, (script, _) in MENUS.items():
            lists = SIZE_LIST.findall(script)
            self.assertTrue(lists, name)
            for sizes in lists:
                for size in (s.strip() for s in sizes.split(",")):
                    self.assertRegex(size, f"^(?:{EXPLICIT.pattern})$", f"{name}: {sizes}")


if __name__ == "__main__":
    unittest.main()
