"""Tests for Lumen's art (tools/art/) and the files it made (shell/assets/art/).

The table, the pictograms' SVG rules, the files' sizes and the credits need
only the standard library, so make check runs them. Drawing and measuring
need numpy, so those tests skip in the build container and run in the
studio container:

    tools/studio.sh python3 -I -m unittest discover -s tools/tests -p 'test_art*.py'
"""

import os
import re
import shutil
import struct
import subprocess
import sys
import tomllib
import unittest

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..')
ART_TOOLS = os.path.join(ROOT, 'tools', 'art')
ART = os.path.join(ROOT, 'shell', 'assets', 'art')
sys.path[:0] = [ART_TOOLS, os.path.join(ROOT, 'studio')]

import pictograms  # noqa: E402  (the standard library, until it draws)

try:
    import numpy as np
except ImportError:
    np = None
else:
    import artkit  # noqa: E402
    import heroes  # noqa: E402

NEEDS_NUMPY = unittest.skipUnless(np, 'needs numpy: run it with tools/studio.sh')
NEEDS_FFMPEG = unittest.skipUnless(shutil.which('ffmpeg'), 'needs ffmpeg: run it with tools/studio.sh')

with open(os.path.join(ART_TOOLS, 'heroes.toml'), 'rb') as f:
    TABLE = tomllib.load(f)
HEROES = sorted(TABLE)
ICONS = ('antenna', 'bluetooth', 'gear', 'hdmi', 'input', 'moon', 'shield', 'usb')


def png_header(path):
    """(width, height, colour type) from a PNG's IHDR."""
    with open(path, 'rb') as f:
        head = f.read(33)
    if head[:8] != b'\x89PNG\r\n\x1a\n' or head[12:16] != b'IHDR':
        raise ValueError(f'{path} is not a PNG')
    width, height, _, colour = struct.unpack('>IIBB', head[16:26])
    return width, height, colour


def files(name):
    """The PNGs a hero is written as: itself and its half-size copy, and
    the same for each layer of a layered hero."""
    layers = ('', '-bg', '-mid', '-fg') if TABLE[name]['scene'] == 'ambient' else ('',)
    return [f'{name}{layer}{size}.png' for layer in layers for size in ('', '-half')]


def read(name):
    with open(os.path.join(ART, name), 'rb') as f:
        return artkit.read_png(f.read())


class TableTest(unittest.TestCase):
    """heroes.toml names the files the brief asks for, each from a seed."""

    def test_the_set(self):
        palette = [f'hero-palette-{i}' for i in range(8)]
        hdmi = [f'hero-hdmi{i}' for i in range(1, 5)]
        self.assertTrue({'hero-ambient', *palette, *hdmi} <= set(HEROES))
        self.assertTrue(set(HEROES) <= {'hero-ambient', *palette, *hdmi, 'hero-live-tv', 'hero-plex',
                                        'hero-homebrew', 'hero-settings'})

    def test_every_hero_has_a_scene_and_a_seed(self):
        scenes = {'ambient', 'palette', 'arrival', 'towers', 'beam', 'circuits', 'panels'}
        for name, spec in TABLE.items():
            with self.subTest(hero=name):
                self.assertIn(spec['scene'], scenes)
                self.assertIsInstance(spec['seed'], int)
        seeds = [spec['seed'] for spec in TABLE.values()]
        self.assertEqual(len(seeds), len(set(seeds)), 'two heroes share a seed')

    def test_one_palette_hero_per_tile_colour_each_its_own_shape(self):
        palette = {name: spec for name, spec in TABLE.items() if spec['scene'] == 'palette'}
        self.assertEqual(sorted(spec['colour'] for spec in palette.values()), list(range(8)))
        for name, spec in palette.items():
            self.assertEqual(name, f'hero-palette-{spec["colour"]}')
        self.assertEqual(len({spec['shape'] for spec in palette.values()}), 8)

    def test_inputs_differ_in_angle_and_colour_temperature(self):
        hdmi = [TABLE[f'hero-hdmi{i}'] for i in range(1, 5)]
        self.assertEqual(len({spec['angle'] for spec in hdmi}), 4)
        self.assertEqual(len({spec['kelvin'] for spec in hdmi}), 4)

    def test_the_palette_is_style_rs(self):
        with open(os.path.join(ROOT, 'shell', 'crates', 'lumen-core', 'src', 'style.rs')) as f:
            style = f.read()
        block = style[style.index('pub const TILE_PALETTE'):]
        block = block[:block.index('];')]
        theirs = [tuple(int(v) for v in m) for m in re.findall(r'rgb8\((\d+), (\d+), (\d+)\)', block)]
        with open(os.path.join(ART_TOOLS, 'heroes.py')) as f:
            source = f.read()
        block = source[source.index('TILE_PALETTE = '):]
        block = block[:block.index('))\n') + 2]
        ours = [tuple(int(v) for v in m) for m in re.findall(r'\((\d+), (\d+), (\d+)\)', block)]
        self.assertEqual(len(theirs), 8)
        self.assertEqual(ours, theirs)


class FilesTest(unittest.TestCase):
    """The files: their sizes, their credits, nothing else in the folder."""

    def test_heroes_are_full_and_half_size_and_small(self):
        for name in HEROES:
            for file in files(name):
                with self.subTest(file=file):
                    path = os.path.join(ART, file)
                    width, height, colour = png_header(path)
                    self.assertEqual((width, height), (960, 320) if '-half' in file else (1920, 640))
                    layer = any(f'-{layer}' in file for layer in ('bg', 'mid', 'fg'))
                    self.assertEqual(colour, 6 if layer else 2, 'layers are RGBA, pictures RGB')
                    self.assertLess(os.path.getsize(path), 1.5e6)

    def test_pictograms_are_svg_and_240_px_png(self):
        for icon in ICONS:
            with self.subTest(icon=icon):
                self.assertEqual(png_header(os.path.join(ART, f'icon-{icon}.png')), (240, 240, 6))
                self.assertTrue(os.path.exists(os.path.join(ART, f'icon-{icon}.svg')))

    def test_nothing_else_is_here(self):
        expected = {f for name in HEROES for f in files(name)}
        expected |= {f'icon-{icon}.{ext}' for icon in ICONS for ext in ('svg', 'png')}
        expected.add('CREDITS.md')
        self.assertEqual(set(os.listdir(ART)), expected)

    def test_credits_name_every_file_its_seed_and_the_licence(self):
        with open(os.path.join(ART, 'CREDITS.md')) as f:
            credits = f.read()
        self.assertIn('Original work for Lumen, made with `tools/art/`'.lower(), credits.lower().replace('\n', ' '))
        self.assertIn('CC BY-SA 4.0', credits)
        rows = {m.group(1): m.group(0) for m in re.finditer(r'^\| `([\w-]+)` \|.*$', credits, re.M)}
        for name, spec in TABLE.items():
            with self.subTest(hero=name):
                self.assertIn(name, rows)
                self.assertIn(str(spec['seed']), rows[name])
        for icon in ICONS:
            self.assertIn(f'icon-{icon}', rows)


class PictogramRulesTest(unittest.TestCase):
    """The hand-written SVGs: strokes only, one colour, the 24-unit grid."""

    def test_every_pictogram_keeps_the_rules(self):
        for icon in ICONS:
            with self.subTest(icon=icon):
                with open(os.path.join(ART, f'icon-{icon}.svg')) as f:
                    self.assertEqual(pictograms.check(f.read()), [])

    def test_the_rules_catch_what_they_should(self):
        good = ('<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" '
                'stroke="#EBEEF5" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{}</svg>')
        self.assertEqual(pictograms.check(good.format('<path d="M3 3H21"/>')), [])
        for bad in ('<path d="M3 3H21" fill="#fff"/>', '<image href="data:image/png;base64,AA=="/>',
                    '<text>HDMI</text>', '<g transform="scale(2)"><path d="M3 3H21"/></g>',
                    '<metadata/>', '<!-- made in an editor --><path d="M3 3H21"/>'):
            with self.subTest(bad=bad):
                self.assertNotEqual(pictograms.check(good.format(bad)), [])
        self.assertNotEqual(pictograms.check(good.replace('stroke-width="2"', 'stroke-width="3"').format('')), [])
        self.assertNotEqual(pictograms.check(good.replace('#EBEEF5', '#FFFFFF').format('')), [])

    def test_paths_read_as_drawn(self):
        lines = pictograms.outline(
            '<svg xmlns="http://www.w3.org/2000/svg"><path d="M2 2h4v4H2Z M10 10l2 0"/></svg>')
        self.assertEqual(lines[0], ([(2, 2), (6, 2), (6, 6), (2, 6)], True))
        self.assertEqual(lines[1], ([(10, 10), (12, 10)], False))
        # A half circle by an arc, right to left over the top (sweep 1 on screen).
        (points, closed), = pictograms.outline(
            '<svg xmlns="http://www.w3.org/2000/svg"><path d="M4 12A8 8 0 0 1 20 12"/></svg>')
        self.assertFalse(closed)
        self.assertAlmostEqual(points[-1][0], 20.0)
        self.assertLess(min(y for _, y in points), 4.05)
        self.assertTrue(all(abs(((x - 12) ** 2 + (y - 12) ** 2) ** 0.5 - 8) < 1e-9 for x, y in points))


@NEEDS_NUMPY
class ArtkitTest(unittest.TestCase):
    """The shared pieces: colour, dither, PNG, the band measure."""

    def test_srgb_round_trip(self):
        v = np.linspace(0.0, 1.0, 1001)
        self.assertTrue(np.allclose(artkit.srgb_encode(artkit.srgb_decode(v)), v, atol=1e-12))
        self.assertAlmostEqual(float(artkit.srgb_encode(0.18)), 0.4614, places=4)

    def test_black_body_colours(self):
        warm, white, cold = artkit.blackbody(2400), artkit.blackbody(6500), artkit.blackbody(12000)
        self.assertGreater(warm[0], 2 * warm[2])
        # D65 lies a little off the Planckian locus, on the green side.
        self.assertTrue(np.allclose(white, 1.0, atol=0.07), white)
        self.assertGreater(cold[2], cold[0])

    def test_blue_noise_is_even_and_has_little_low_frequency(self):
        t = artkit.blue_noise()
        self.assertEqual(len(np.unique(t)), t.size, 'every threshold once')
        spectrum = np.abs(np.fft.fft2(t - t.mean())) ** 2
        f = np.hypot(*np.meshgrid(np.fft.fftfreq(64), np.fft.fftfreq(64)))
        low, high = spectrum[(f > 0) & (f < 0.1)].mean(), spectrum[f > 0.3].mean()
        self.assertLess(low, 0.01 * high)

    def test_dither_keeps_the_mean_and_black_and_white(self):
        for level in (0.0, 0.3 / 255, 2.4 / 255, 0.5, 1.0):
            flat = np.full((128, 128, 3), level)
            codes = artkit.quantize(flat)
            self.assertAlmostEqual(codes.mean() / 255.0, level, delta=0.01 / 255)
        self.assertEqual(artkit.quantize(np.zeros((8, 8, 3))).max(), 0)
        self.assertEqual(artkit.quantize(np.ones((8, 8, 3))).min(), 255)

    def test_png_round_trip_through_every_filter(self):
        rng = np.random.default_rng(1)
        smooth = np.cumsum(rng.integers(-2, 3, (40, 50, 4)), axis=1) % 256
        for pixels in (smooth[:, :, :3], smooth, rng.integers(0, 256, (31, 17, 3))):
            pixels = pixels.astype(np.uint8)
            data = artkit.png_bytes(pixels)
            self.assertTrue(np.array_equal(artkit.read_png(data), pixels))
            self.assertEqual(data, artkit.png_bytes(pixels), 'the same pixels give the same bytes')

    @NEEDS_FFMPEG
    def test_ffmpeg_reads_our_png_the_same(self):
        rng = np.random.default_rng(2)
        pixels = (np.cumsum(rng.integers(-3, 4, (24, 40, 4)), axis=0) % 256).astype(np.uint8)
        out = subprocess.run(['ffmpeg', '-v', 'error', '-i', '-', '-f', 'rawvideo', '-pix_fmt', 'rgba', '-'],
                             input=artkit.png_bytes(pixels), capture_output=True, check=True).stdout
        self.assertTrue(np.array_equal(np.frombuffer(out, np.uint8).reshape(pixels.shape), pixels))

    def test_dither_breaks_the_bands_of_a_dark_gradient(self):
        # The darkest gradient the heroes have: code 0 to 8 over the 640 px
        # height, the slope of a title band. Rounded, it is nine flat bands;
        # dithered, no run of one code lasts.
        ramp = np.repeat(np.linspace(0.0, 8.0 / 255, 640)[:, None, None], 1920, axis=1).repeat(3, axis=2)
        before = artkit.longest_band(ramp[:, :, 1] * 255, artkit.quantize(ramp, dither=False)[:, :, 1])
        after = artkit.longest_band(ramp[:, :, 1] * 255, artkit.quantize(ramp)[:, :, 1])
        self.assertGreater(before, 70)
        self.assertLess(after, 16)
        # A flat patch, or one bright pixel in it, is not a band.
        flat = np.full((64, 64), 3.0)
        flat[30, 30] = 9.0
        self.assertEqual(artkit.longest_band(flat, np.round(flat)), 0)


@NEEDS_NUMPY
class HeroTest(unittest.TestCase):
    """The committed heroes: made for an OLED in a dark room, the title
    readable, no bands, and drawn again byte for byte from their seeds."""

    @classmethod
    def setUpClass(cls):
        cls.rendered = {}

    def render(self, name):
        if name not in self.rendered:
            self.rendered[name] = heroes.render(name, TABLE[name])
        return self.rendered[name]

    def test_light_levels(self):
        for name in HEROES:
            for file in (f'{name}.png', f'{name}-half.png'):
                with self.subTest(file=file):
                    s = artkit.light_stats(read(file))
                    self.assertLess(s['mean'], 0.20, 'mean luma under 20%')
                    self.assertLess(s['median'], 0.10, 'most pixels far lower')
                    self.assertLess(s['p99'], 0.60, 'no large bright areas')
                    self.assertLess(s['max'], 0.95, 'nothing near white')
                    self.assertEqual(s['white'], 0, 'no pure white')

    def test_the_title_band_darkens_toward_black(self):
        for name in HEROES:
            with self.subTest(hero=name):
                s = artkit.light_stats(read(f'{name}.png'))
                self.assertLess(s['band_mean'], 0.06)
                self.assertLess(s['band_end'], 0.02)
                self.assertLess(s['band_end'], s['band_mean'] + 1e-9)
                self.assertGreater(s['contrast'], 7.0, 'the title at 7:1 or more over 99% of the band')

    def test_drawn_again_from_the_seed_byte_for_byte(self):
        for name in HEROES:
            images = self.render(name)
            small = heroes.halves(images)
            for suffix, encoded in images.items():
                for size, picture in (('', encoded), ('-half', small[suffix])):
                    with self.subTest(file=f'{name}{suffix}{size}.png'):
                        with open(os.path.join(ART, f'{name}{suffix}{size}.png'), 'rb') as f:
                            self.assertEqual(artkit.png_bytes(artkit.quantize(picture)), f.read())

    def test_dither_breaks_every_heros_bands(self):
        for name in HEROES:
            encoded = self.render(name)['']
            with self.subTest(hero=name):
                before, after = artkit.banding(encoded, artkit.quantize(encoded))
                # A band longer than the blue-noise tile would mean the
                # dither failed; and every band left is shorter than before.
                self.assertLess(after, 64)
                self.assertLessEqual(after, before)

    def test_palette_heroes_differ_in_shape(self):
        small = {}
        for i in range(8):
            luma = artkit.downsample(read(f'hero-palette-{i}-half.png') / 255.0, 8) @ artkit.LUMA
            small[i] = (luma - luma.mean()) / luma.std()
        for i in range(8):
            for j in range(i + 1, 8):
                with self.subTest(pair=(i, j)):
                    self.assertLess(abs((small[i] * small[j]).mean()), 0.6)

    def test_ambient_layers_make_the_picture(self):
        for size in ('', '-half'):
            with self.subTest(size=size or 'full'):
                bg, mid, fg = (read(f'hero-ambient-{layer}{size}.png') / 255.0 for layer in ('bg', 'mid', 'fg'))
                self.assertTrue(np.all(bg[:, :, 3] == 1.0), 'the sky is opaque')
                self.assertEqual(fg[: fg.shape[0] // 4, :, 3].max(), 0.0, 'nothing in front of the high sky')
                self.assertTrue(np.all(fg[-10:, :, 3] == 1.0), 'the lake is opaque')
                self.assertLess(mid[:, :, 3].mean(), 0.2)
                self.assertLess((mid[:, :, 3] == 1.0).mean(), 0.01, 'light is seldom opaque')
                under = heroes.over(mid[:, :, :3], mid[:, :, 3], bg[:, :, :3])
                composed = heroes.over(fg[:, :, :3], fg[:, :, 3], under)
                flat = read(f'hero-ambient{size}.png') / 255.0
                diff = np.abs(composed - flat) * 255.0
                self.assertLess(diff.mean(), 0.6)
                self.assertLess(np.percentile(diff, 99.9), 4.0)

    def test_the_same_seed_draws_the_same(self):
        spec = TABLE['hero-palette-4']
        a = heroes.render('hero-palette-4', spec, 480, 160)['']
        b = heroes.render('hero-palette-4', spec, 480, 160)['']
        self.assertTrue(np.array_equal(a, b))
        other = heroes.render('hero-palette-4', dict(spec, seed=spec['seed'] + 1), 480, 160)['']
        self.assertFalse(np.array_equal(a, other))


@NEEDS_NUMPY
class PictogramTest(unittest.TestCase):
    """The PNGs are the SVGs drawn, in one colour, readable at 120 px."""

    def svg(self, icon):
        with open(os.path.join(ART, f'icon-{icon}.svg')) as f:
            return f.read()

    def test_the_png_is_the_svg_drawn(self):
        for icon in ICONS:
            with self.subTest(icon=icon):
                pixels = read(f'icon-{icon}.png')
                self.assertTrue(np.array_equal(pixels, pictograms.render(self.svg(icon))))
                self.assertTrue(np.all(pixels[:, :, :3] == (0xEB, 0xEE, 0xF5)))
                self.assertGreater(pixels[:, :, 3].max(), 250)
                edge = np.concatenate([pixels[0, :, 3], pixels[-1, :, 3], pixels[:, 0, 3], pixels[:, -1, 3]])
                self.assertEqual(edge.max(), 0, 'the stroke stays inside the square')

    def test_bold_at_tile_size(self):
        # At 120 px a 2-unit stroke is 10 px: a solid core shows in every
        # pictogram, and each covers a fair share of its square.
        for icon in ICONS:
            with self.subTest(icon=icon):
                cover = pictograms.raster(self.svg(icon), 120)
                self.assertGreater((cover == 1.0).mean(), 0.12)
                self.assertLess(cover.mean(), 0.45)

    @NEEDS_FFMPEG
    def test_librsvg_draws_them_the_same(self):
        for icon in ICONS:
            with self.subTest(icon=icon):
                out = subprocess.run(['ffmpeg', '-v', 'error', '-width', '240', '-height', '240',
                                      '-i', os.path.join(ART, f'icon-{icon}.svg'), '-f', 'rawvideo',
                                      '-pix_fmt', 'rgba', '-'], capture_output=True, check=True).stdout
                theirs = np.frombuffer(out, np.uint8).reshape(240, 240, 4)[:, :, 3] > 127
                ours = read(f'icon-{icon}.png')[:, :, 3] > 127
                self.assertGreater((ours & theirs).sum() / (ours | theirs).sum(), 0.99)


if __name__ == '__main__':
    unittest.main()
