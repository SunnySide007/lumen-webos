"""What an HDR10 loop from the studio must look like to ffprobe.

Pure functions over ffprobe's JSON and a few measurements, standard library
only, so their tests run under make check; studio/verify.py gathers the
inputs. `expect` is the <video>.json that studio/encode.py writes.
"""

import re
import statistics
from fractions import Fraction

import hdr

# HEVC Main10, BT.2020 / PQ, narrow range, chroma sited as encode.py samples it.
STREAM = {
    'codec_name': 'hevc', 'profile': 'Main 10', 'pix_fmt': 'yuv420p10le', 'color_primaries': 'bt2020',
    'color_transfer': 'smpte2084', 'color_space': 'bt2020nc', 'color_range': 'tv', 'chroma_location': 'left',
}


def stream_problems(stream, expect):
    """Ways the video stream differs from an HDR10 stream of the encode.

    The frame count is checked only where the container states it (MP4 does,
    Matroska does not); verify.py also counts the frames it decodes.
    """
    wanted = dict(STREAM, width=expect['width'], height=expect['height'],
                  r_frame_rate=f"{expect['fps']}/1")
    if 'nb_frames' in stream:
        wanted['nb_frames'] = str(expect['frames'])
    problems = []
    for field, value in wanted.items():
        if field not in stream:
            problems.append(f'{field} is missing, should be {value}')
        elif stream[field] != value:
            problems.append(f'{field} is {stream[field]}, should be {value}')
    return problems


_MASTER = re.compile(r'G\((\d+),(\d+)\)B\((\d+),(\d+)\)R\((\d+),(\d+)\)WP\((\d+),(\d+)\)L\((\d+),(\d+)\)')


def parse_master_display(text):
    """x265's --master-display string as ffprobe's (numerator, denominator) fields."""
    match = _MASTER.fullmatch(text)
    if not match:
        raise ValueError(f'not a mastering display: {text!r}')
    v = [int(n) for n in match.groups()]
    chroma = ('green_x', 'green_y', 'blue_x', 'blue_y', 'red_x', 'red_y', 'white_point_x', 'white_point_y')
    fields = {name: (n, 50000) for name, n in zip(chroma, v)}
    fields['max_luminance'] = (v[8], 10000)
    fields['min_luminance'] = (v[9], 10000)
    return fields


def _fraction(text):
    try:
        return Fraction(text)
    except (TypeError, ValueError, ZeroDivisionError):
        return None


def keyframe_problems(keyframes, expect):
    """Keyframes missing the HDR10 SEI, or carrying values the encode did not measure.

    Every keyframe must carry both, so that a player starting or looping
    there switches the TV into HDR with the right metadata.
    """
    if not keyframes:
        return ['no keyframes']
    master = {name: Fraction(*nd) for name, nd in parse_master_display(expect['master_display']).items()}
    problems = []
    for frame in keyframes:
        at = frame.get('pts_time', '?')
        side = {s.get('side_data_type'): s for s in frame.get('side_data_list', [])}
        display = side.get('Mastering display metadata')
        if display is None:
            problems.append(f'keyframe at {at} s has no mastering display metadata')
        else:
            wrong = [f'{name} {display.get(name)}' for name, value in master.items()
                     if _fraction(display.get(name)) != value]
            if wrong:
                problems.append(f'keyframe at {at} s has the wrong mastering display: {", ".join(wrong)}')
        light = side.get('Content light level metadata')
        if light is None:
            problems.append(f'keyframe at {at} s has no content light level metadata')
        elif (light.get('max_content'), light.get('max_average')) != (expect['max_cll'], expect['max_fall']):
            problems.append(f'keyframe at {at} s says MaxCLL {light.get("max_content")}, MaxFALL '
                            f'{light.get("max_average")}; the frames measured {expect["max_cll"]}, '
                            f'{expect["max_fall"]}')
    return problems


def light_problems(peak_luma_code, max_cll, slack=24):
    """Whether the decoded picture is brighter than the MaxCLL the stream claims.

    Luma never exceeds the brightest of R', G' and B', so the brightest
    decoded luma code may not pass MaxCLL's code. Compression rings around
    one-pixel highlights such as stars, so `slack` allows about a quarter
    more light near 700 cd/m²: enough to pass honest encodes, while a wrong
    measurement (a missed scale is a factor of 100) still fails. A check
    independent of encode.py's own measurement.
    """
    limit = hdr.luma_code(hdr.pq_encode(max_cll)) + slack
    if peak_luma_code > limit:
        nits = hdr.pq_decode((peak_luma_code - hdr.narrow_range().black) / hdr.narrow_range().luma_scale)
        return [f'decoded luma reaches code {peak_luma_code} ({nits:.0f} cd/m²), above MaxCLL '
                f'{max_cll} (code {limit - slack})']
    return []


def loop_problems(seam, steps, factor=2.0, floor=0.5):
    """Whether playing the last frame and then the first looks like any other step.

    seam is the mean absolute luma difference (in 10-bit code values) from
    the last frame to the first, steps the same for each consecutive pair.
    The limit follows the median step, so one scene cut inside the loop
    cannot loosen it, with a floor for still content.
    """
    if not steps:
        return ['a loop needs at least two frames']
    typical = statistics.median(steps)
    limit = max(floor, factor * typical)
    if seam > limit:
        return [f'the loop jumps: the last frame differs from the first by {seam:.2f} code values, '
                f'consecutive frames typically by {typical:.2f} (limit {limit:.2f})']
    return []
