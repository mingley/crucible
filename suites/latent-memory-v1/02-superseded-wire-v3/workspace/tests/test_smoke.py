"""Baseline smoke tests for ZephyrWire codec primitives and data frame routing."""

from __future__ import annotations

import unittest

from zephyr_wire import FRAME_TYPE_DATA, route_data_frame
from zephyr_wire.v2_codec import encode_v2_frame
from zephyr_wire.v3_codec import encode_v3_frame


class ZephyrWireSmokeTests(unittest.TestCase):
    def test_v2_and_v3_data_routing(self) -> None:
        raw2 = encode_v2_frame(FRAME_TYPE_DATA, 0, b"alpha")
        raw3 = encode_v3_frame(FRAME_TYPE_DATA, 0, b"bravo")
        self.assertEqual(route_data_frame(raw2), (2, b"alpha"))
        self.assertEqual(route_data_frame(raw3), (3, b"bravo"))


if __name__ == "__main__":
    unittest.main()
