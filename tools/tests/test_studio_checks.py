"""Tests for studio/checks.py: python3 -I -m unittest discover -s tools/tests"""

import copy
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'studio'))

import checks  # noqa: E402
import hdr  # noqa: E402

MASTER = 'G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1)'
EXPECT = {'width': 1920, 'height': 1080, 'fps': 24, 'frames': 240,
          'max_cll': 750, 'max_fall': 18, 'master_display': MASTER}

# Shaped like ffprobe -show_streams -of json on an encode from studio/encode.py.
STREAM = {
    'codec_name': 'hevc', 'codec_tag_string': 'hev1', 'profile': 'Main 10', 'width': 1920, 'height': 1080,
    'pix_fmt': 'yuv420p10le', 'color_range': 'tv', 'color_space': 'bt2020nc', 'color_transfer': 'smpte2084',
    'color_primaries': 'bt2020', 'chroma_location': 'left', 'r_frame_rate': '24/1', 'nb_frames': '240',
}

# Shaped like ffprobe -skip_frame nokey -show_frames -of json, one keyframe.
KEYFRAME = {
    'key_frame': 1, 'pts_time': '0.000000', 'pict_type': 'I',
    'side_data_list': [
        {'side_data_type': 'H.26[45] User Data Unregistered SEI message'},
        {'side_data_type': 'Mastering display metadata',
         'red_x': '34000/50000', 'red_y': '16000/50000', 'green_x': '13250/50000', 'green_y': '34500/50000',
         'blue_x': '7500/50000', 'blue_y': '3000/50000', 'white_point_x': '15635/50000',
         'white_point_y': '16450/50000', 'min_luminance': '1/10000', 'max_luminance': '10000000/10000'},
        {'side_data_type': 'Content light level metadata', 'max_content': 750, 'max_average': 18},
    ],
}


def keyframe_at(seconds):
    frame = copy.deepcopy(KEYFRAME)
    frame['pts_time'] = f'{seconds:.6f}'
    return frame


class StreamTest(unittest.TestCase):
    def test_an_hdr10_stream_passes(self):
        self.assertEqual(checks.stream_problems(STREAM, EXPECT), [])

    def test_each_wrong_field_is_reported(self):
        for field, value in (('codec_name', 'h264'), ('profile', 'Main'), ('pix_fmt', 'yuv420p'),
                             ('color_primaries', 'bt709'), ('color_transfer', 'arib-std-b67'),
                             ('color_space', 'bt709'), ('color_range', 'pc'), ('chroma_location', 'topleft')):
            stream = dict(STREAM, **{field: value})
            problems = checks.stream_problems(stream, EXPECT)
            self.assertEqual(len(problems), 1, (field, problems))
            self.assertIn(field, problems[0])
            self.assertIn(value, problems[0])

    def test_missing_fields_are_reported(self):
        stream = dict(STREAM)
        del stream['color_transfer']
        self.assertIn('color_transfer is missing', ' '.join(checks.stream_problems(stream, EXPECT)))

    def test_matroska_needs_no_frame_count(self):
        # MKV streams carry no nb_frames; verify.py counts decoded frames instead.
        stream = dict(STREAM)
        del stream['nb_frames']
        self.assertEqual(checks.stream_problems(stream, EXPECT), [])

    def test_size_rate_and_length_must_match_the_encode(self):
        stream = dict(STREAM, width=1280, r_frame_rate='30000/1001', nb_frames='239')
        problems = ' '.join(checks.stream_problems(stream, EXPECT))
        for word in ('width', 'r_frame_rate', 'nb_frames'):
            self.assertIn(word, problems)


class MasterDisplayTest(unittest.TestCase):
    def test_parses_x265_form_into_ffprobe_fields(self):
        fields = checks.parse_master_display(MASTER)
        self.assertEqual(fields['green_x'], (13250, 50000))
        self.assertEqual(fields['white_point_y'], (16450, 50000))
        self.assertEqual(fields['max_luminance'], (10000000, 10000))
        self.assertEqual(fields['min_luminance'], (1, 10000))

    def test_refuses_other_text(self):
        for text in ('', 'G(1,2)B(3,4)', MASTER.replace('WP', 'W'), MASTER + 'X'):
            with self.assertRaises(ValueError, msg=text):
                checks.parse_master_display(text)


class SideDataTest(unittest.TestCase):
    def test_every_keyframe_carrying_hdr10_passes(self):
        frames = [keyframe_at(0.0), keyframe_at(2.0), keyframe_at(4.0)]
        self.assertEqual(checks.keyframe_problems(frames, EXPECT), [])

    def test_a_keyframe_without_mastering_display_fails(self):
        bare = keyframe_at(2.0)
        bare['side_data_list'] = [s for s in bare['side_data_list']
                                  if s['side_data_type'] != 'Mastering display metadata']
        problems = checks.keyframe_problems([keyframe_at(0.0), bare], EXPECT)
        self.assertEqual(len(problems), 1)
        self.assertIn('2.000000', problems[0])
        self.assertIn('mastering display', problems[0])

    def test_a_keyframe_without_any_side_data_fails(self):
        bare = keyframe_at(0.0)
        del bare['side_data_list']
        self.assertEqual(len(checks.keyframe_problems([bare], EXPECT)), 2)

    def test_light_levels_must_match_the_measurement(self):
        frame = keyframe_at(0.0)
        frame['side_data_list'][2]['max_content'] = 1000
        problems = checks.keyframe_problems([frame], EXPECT)
        self.assertEqual(len(problems), 1)
        self.assertIn('1000', problems[0])

    def test_mastering_display_must_match_the_encode(self):
        frame = keyframe_at(0.0)
        frame['side_data_list'][1]['max_luminance'] = '40000000/10000'
        problems = checks.keyframe_problems([frame], EXPECT)
        self.assertEqual(len(problems), 1)
        self.assertIn('max_luminance', problems[0])

    def test_equal_fractions_in_other_terms_pass(self):
        frame = keyframe_at(0.0)
        frame['side_data_list'][1]['green_x'] = '53/200'
        self.assertEqual(checks.keyframe_problems([frame], EXPECT), [])

    def test_no_keyframes_fails(self):
        self.assertTrue(checks.keyframe_problems([], EXPECT))


class LightTest(unittest.TestCase):
    # Y' never exceeds max(R', G', B'), so decoded luma bounds MaxCLL from below.
    def test_luma_within_maxcll_passes(self):
        self.assertEqual(checks.light_problems(723, 1000), [])  # 723 is 1000 cd/m²
        self.assertEqual(checks.light_problems(600, 1000), [])

    def test_compression_ringing_on_a_star_passes(self):
        self.assertEqual(checks.light_problems(723 + 24, 1000), [])

    def test_luma_brighter_than_maxcll_fails(self):
        problems = checks.light_problems(723 + 25, 1000)
        self.assertEqual(len(problems), 1)
        self.assertIn('MaxCLL', problems[0])

    def test_a_maxcll_measured_without_its_scale_fails(self):
        # 750 cd/m² stars, but MaxCLL taken from EXR values (1.0 = 100 cd/m²).
        self.assertTrue(checks.light_problems(hdr.luma_code(hdr.pq_encode(750.0)), 8))


class LoopTest(unittest.TestCase):
    def test_a_seam_like_any_other_step_passes(self):
        self.assertEqual(checks.loop_problems(0.9, [0.8, 0.85, 0.9, 0.95, 1.0]), [])

    def test_a_jump_at_the_seam_fails(self):
        problems = checks.loop_problems(6.0, [0.8, 0.85, 0.9, 0.95, 1.0])
        self.assertEqual(len(problems), 1)
        self.assertIn('6.00', problems[0])

    def test_one_odd_step_does_not_loosen_the_limit(self):
        # A scene cut inside the loop must not hide a jump at the seam.
        self.assertTrue(checks.loop_problems(6.0, [0.8, 0.8, 30.0, 0.8, 0.8]))

    def test_still_content_has_a_floor(self):
        self.assertEqual(checks.loop_problems(0.3, [0.0, 0.0, 0.0]), [])
        self.assertTrue(checks.loop_problems(3.0, [0.0, 0.0, 0.0]))

    def test_needs_steps(self):
        self.assertTrue(checks.loop_problems(0.0, []))


if __name__ == '__main__':
    unittest.main()
