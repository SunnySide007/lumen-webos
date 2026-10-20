"""Tests for studio/hdr.py: python3 -I -m unittest discover -s tools/tests"""

import math
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'studio'))

import hdr  # noqa: E402


class PqTest(unittest.TestCase):
    # Signal values for SMPTE ST 2084, as tabulated in ITU-R BT.2100 and BT.2408.
    REFERENCE = [(0.0, 0.0), (100.0, 0.508078), (1000.0, 0.751827), (10000.0, 1.0)]

    def test_encode_matches_the_standard(self):
        for nits, signal in self.REFERENCE:
            self.assertAlmostEqual(hdr.pq_encode(nits), signal, delta=2e-6, msg=nits)

    def test_reference_white_is_58_percent(self):
        # BT.2408: HDR reference white, 203 cd/m², sits at 58% of the PQ signal.
        self.assertEqual(round(hdr.pq_encode(203.0), 2), 0.58)

    def test_decode_matches_the_standard(self):
        for nits, signal in self.REFERENCE:
            self.assertAlmostEqual(hdr.pq_decode(signal), nits, delta=nits * 2e-5 + 1e-9)

    def test_round_trip(self):
        for nits in (0.0001, 0.005, 0.1, 1.0, 48.0, 203.0, 750.0, 4000.0, 9999.0):
            self.assertAlmostEqual(hdr.pq_decode(hdr.pq_encode(nits)), nits, delta=nits * 1e-9)

    def test_encode_is_monotonic(self):
        signals = [hdr.pq_encode(n / 10.0) for n in range(0, 100001, 7)]
        self.assertEqual(signals, sorted(signals))

    def test_out_of_range_values_clamp(self):
        self.assertEqual(hdr.pq_encode(-5.0), hdr.pq_encode(0.0))
        self.assertEqual(hdr.pq_encode(20000.0), 1.0)
        self.assertEqual(hdr.pq_decode(-0.1), 0.0)
        self.assertEqual(hdr.pq_decode(1.5), 10000.0)

    def test_array_forms_are_plain_arithmetic(self):
        # pq_from_linear and ycbcr_from_rgb take numpy arrays too, where min(),
        # max() and if-tests fail; Arith stands in for an array and allows
        # only arithmetic.
        class Arith:
            def __init__(self, v):
                self.v = v

            def _value(self, o):
                return o.v if isinstance(o, Arith) else o

            def __pow__(self, e):
                return Arith(self.v ** e)

            def __mul__(self, o):
                return Arith(self.v * self._value(o))

            __rmul__ = __mul__

            def __add__(self, o):
                return Arith(self.v + self._value(o))

            __radd__ = __add__

            def __sub__(self, o):
                return Arith(self.v - self._value(o))

            def __rsub__(self, o):
                return Arith(self._value(o) - self.v)

            def __truediv__(self, o):
                return Arith(self.v / self._value(o))

            def __rtruediv__(self, o):
                return Arith(self._value(o) / self.v)

            def _refuse(self, *_):
                raise TypeError('not plain arithmetic')

            __lt__ = __le__ = __gt__ = __ge__ = __eq__ = __bool__ = __float__ = _refuse

        self.assertAlmostEqual(hdr.pq_from_linear(Arith(0.1)).v, hdr.pq_encode(1000.0))
        y, cb, cr = hdr.ycbcr_from_rgb(Arith(0.5), Arith(0.5), Arith(0.5))
        self.assertAlmostEqual(y.v, 0.5)
        self.assertAlmostEqual(cb.v, 0.0)
        self.assertAlmostEqual(cr.v, 0.0)

    def test_ten_bit_narrow_range_codes(self):
        self.assertEqual(hdr.luma_code(hdr.pq_encode(0.0)), 64)
        self.assertEqual(hdr.luma_code(hdr.pq_encode(100.0)), 509)
        self.assertEqual(hdr.luma_code(hdr.pq_encode(1000.0)), 723)
        self.assertEqual(hdr.luma_code(hdr.pq_encode(10000.0)), 940)
        self.assertEqual(hdr.chroma_code(0.0), 512)
        self.assertEqual(hdr.chroma_code(-0.5), 64)
        self.assertEqual(hdr.chroma_code(0.5), 960)

    def test_other_bit_depths(self):
        self.assertEqual(hdr.luma_code(0.0, bits=8), 16)
        self.assertEqual(hdr.luma_code(1.0, bits=8), 235)
        self.assertEqual(hdr.luma_code(1.0, bits=12), 3760)
        self.assertEqual(hdr.chroma_code(0.0, bits=12), 2048)

    def test_codes_clamp_to_the_legal_range(self):
        self.assertEqual(hdr.luma_code(1.2), 940)
        self.assertEqual(hdr.luma_code(-0.1), 64)
        self.assertEqual(hdr.chroma_code(0.7), 960)


class YCbCrTest(unittest.TestCase):
    def test_bt2020_coefficients(self):
        self.assertEqual((hdr.KR, hdr.KB), (0.2627, 0.0593))

    def test_grey_has_no_chroma(self):
        y, cb, cr = hdr.ycbcr_from_rgb(0.6, 0.6, 0.6)
        self.assertAlmostEqual(y, 0.6)
        self.assertAlmostEqual(cb, 0.0)
        self.assertAlmostEqual(cr, 0.0)

    def test_primaries_reach_the_chroma_extremes(self):
        _, cb, _ = hdr.ycbcr_from_rgb(0.0, 0.0, 1.0)
        _, _, cr = hdr.ycbcr_from_rgb(1.0, 0.0, 0.0)
        self.assertAlmostEqual(cb, 0.5)
        self.assertAlmostEqual(cr, 0.5)
        y, _, _ = hdr.ycbcr_from_rgb(0.0, 1.0, 0.0)
        self.assertAlmostEqual(y, 0.6780)


class PrimariesTest(unittest.TestCase):
    def assertMatrixAlmostEqual(self, actual, expected, delta):
        for row_a, row_e in zip(actual, expected):
            for a, e in zip(row_a, row_e):
                self.assertAlmostEqual(a, e, delta=delta, msg=(actual, expected))

    def test_bt709_to_bt2020_matches_bt2087(self):
        # ITU-R BT.2087-0, equation (4), rounded there to four places.
        self.assertMatrixAlmostEqual(
            hdr.conversion_matrix(hdr.BT709, hdr.BT2020),
            [[0.6274, 0.3293, 0.0433],
             [0.0691, 0.9195, 0.0114],
             [0.0164, 0.0880, 0.8956]], delta=1e-4)

    def test_p3_d65_to_bt2020(self):
        self.assertMatrixAlmostEqual(
            hdr.conversion_matrix(hdr.P3_D65, hdr.BT2020),
            [[0.7538, 0.1986, 0.0476],
             [0.0457, 0.9418, 0.0125],
             [-0.0012, 0.0176, 0.9836]], delta=1e-4)

    def test_bt709_luminance_row(self):
        # The Y row of BT.709's RGB-to-XYZ matrix is its luma coefficients.
        y_row = hdr.rgb_to_xyz(hdr.BT709)[1]
        self.assertMatrixAlmostEqual([y_row], [[0.2126, 0.7152, 0.0722]], delta=1e-4)

    def test_white_stays_white(self):
        for src in (hdr.BT709, hdr.P3_D65):
            m = hdr.conversion_matrix(src, hdr.BT2020)
            self.assertMatrixAlmostEqual([hdr.apply(m, (1.0, 1.0, 1.0))], [[1.0, 1.0, 1.0]], 1e-12)

    def test_conversion_round_trips(self):
        there = hdr.conversion_matrix(hdr.P3_D65, hdr.BT2020)
        back = hdr.conversion_matrix(hdr.BT2020, hdr.P3_D65)
        rgb = (0.2, 0.7, 0.05)
        self.assertMatrixAlmostEqual([hdr.apply(back, hdr.apply(there, rgb))], [rgb], 1e-12)

    def test_degenerate_primaries_are_refused(self):
        on_a_line = hdr.Primaries((0.1, 0.1), (0.2, 0.2), (0.3, 0.3), hdr.D65)
        with self.assertRaises(ValueError):
            hdr.rgb_to_xyz(on_a_line)
        with self.assertRaises(ValueError):
            hdr.rgb_to_xyz(hdr.Primaries((0.64, 0.0), (0.3, 0.6), (0.15, 0.06), hdr.D65))

    def test_different_white_points_are_refused(self):
        d50 = hdr.Primaries(hdr.BT709.red, hdr.BT709.green, hdr.BT709.blue, (0.3457, 0.3585))
        with self.assertRaises(ValueError):
            hdr.conversion_matrix(d50, hdr.BT2020)


class MetadataTest(unittest.TestCase):
    def test_master_display_for_a_p3_monitor(self):
        # The string x265's documentation gives for a P3-D65, 1000 cd/m² display.
        self.assertEqual(hdr.master_display(hdr.P3_D65, 1000.0, 0.0001),
                         'G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1)')

    def test_master_display_for_bt2020(self):
        self.assertEqual(hdr.master_display(hdr.BT2020, 4000.0, 0.005),
                         'G(8500,39850)B(6550,2300)R(35400,14600)WP(15635,16450)L(40000000,50)')

    def test_master_display_allows_a_true_black(self):
        self.assertTrue(hdr.master_display(hdr.P3_D65, 1000.0, 0.0).endswith('L(10000000,0)'))

    def test_master_display_refuses_impossible_luminance(self):
        for peak, black in ((1000.0, -0.0001), (1000.0, 1000.0), (0.0, 0.0), (20000.0, 0.0001)):
            with self.assertRaises(ValueError, msg=(peak, black)):
                hdr.master_display(hdr.P3_D65, peak, black)

    def test_content_light_level_takes_the_brightest_pixel_and_frame(self):
        frames = [(120.0, 3.2), (749.2, 11.6), (300.0, 17.01)]
        self.assertEqual(hdr.content_light_level(frames), (750, 18))

    def test_content_light_level_of_exact_integers(self):
        self.assertEqual(hdr.content_light_level([(1000.0, 400.0)]), (1000, 400))

    def test_content_light_level_ignores_float_noise(self):
        # A float32 mean can land an ulp above the max of a flat frame, and a
        # sum can land a hair above a whole number.
        self.assertEqual(hdr.content_light_level([(5.0, 5.0000003), (1000.0000001, 2.0)]), (1000, 5))

    def test_content_light_level_of_black(self):
        self.assertEqual(hdr.content_light_level([(0.0, 0.0), (0.0, 0.0)]), (0, 0))

    def test_content_light_level_refuses_bad_stats(self):
        bad = ([], [(100.0, 200.0)], [(-1.0, 0.0)], [(math.nan, 1.0)], [(70000.0, 1.0)])
        for frames in bad:
            with self.assertRaises(ValueError, msg=frames):
                hdr.content_light_level(frames)

    def test_x265_params_carry_hdr10(self):
        params = hdr.x265_params(max_cll=750, max_fall=18, master=hdr.master_display(hdr.P3_D65, 1000.0, 0.0001))
        fields = dict(p.split('=', 1) for p in params.split(':'))
        self.assertEqual(fields['colorprim'], 'bt2020')
        self.assertEqual(fields['transfer'], 'smpte2084')
        self.assertEqual(fields['colormatrix'], 'bt2020nc')
        self.assertEqual(fields['range'], 'limited')
        self.assertEqual(fields['hdr10'], '1')
        self.assertEqual(fields['hdr10-opt'], '1')
        self.assertEqual(fields['repeat-headers'], '1')
        self.assertEqual(fields['chromaloc'], '0')
        self.assertEqual(fields['max-cll'], '750,18')
        self.assertEqual(fields['master-display'],
                         'G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1)')

    def test_x265_params_pass_extra_options_through(self):
        params = hdr.x265_params(1, 1, 'G(0,0)B(0,0)R(0,0)WP(0,0)L(1,0)', {'crf': 16, 'aq-mode': 3})
        self.assertTrue(params.endswith(':crf=16:aq-mode=3'))

    def test_x265_params_refuse_separators_in_values(self):
        for extra in ({'crf': '16:hdr10=0'}, {'crf=1': 2}, {'a:b': 1}):
            with self.assertRaises(ValueError, msg=extra):
                hdr.x265_params(1, 1, 'x', extra)

    def test_x265_params_keep_the_hdr10_fields(self):
        with self.assertRaises(ValueError):
            hdr.x265_params(1, 1, 'x', {'transfer': 'bt709'})


if __name__ == '__main__':
    unittest.main()
