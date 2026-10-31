#!/usr/bin/env python3
"""Lays every hero and pictogram out on one sheet for review.

    tools/studio.sh python3 -I tools/art/contact_sheet.py
    tools/studio.sh python3 -I tools/art/contact_sheet.py --art tools/out/art/try --out tools/out/art/try.png

Each hero at 480 x 160 with its name under it, the ambient hero's layers over
a grey check so their transparency shows, and the pictograms in a row at
120 px, the size a tile shows them at. Reads the PNGs in --art (by default
shell/assets/art) and writes --out (by default tools/out/art/contact-sheet.png,
outside git). The names are drawn by ffmpeg's drawtext in Inter, the shell's
typeface, so this runs in the studio container.
"""

import argparse
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path[:0] = [HERE]

import numpy as np  # noqa: E402

import artkit  # noqa: E402

ART = os.path.join(ROOT, 'shell', 'assets', 'art')
OUT = os.path.join(ROOT, 'tools', 'out', 'art', 'contact-sheet.png')
FONT = os.path.join(ROOT, 'shell', 'assets', 'fonts', 'Inter-SemiBold.ttf')

COLUMNS = 4
CELL_W, CELL_H = 480, 160
ICON = 120
GAP = 24
LABEL = 40
SHEET = (28, 28, 30)


def checker(h, w, size=16):
    """A grey check, to show transparency behind a layer."""
    squares = ((np.arange(h)[:, None] // size + np.arange(w)[None, :] // size) % 2).astype(np.float64)
    return (0.16 + 0.08 * squares)[:, :, None] * np.ones(3)


def shrink(pixels, factor):
    """An 8-bit image made `factor` times smaller, averaged in linear light,
    and with alpha shown over a check. Returns encoded floats."""
    encoded = pixels / 255.0
    rgb = artkit.srgb_decode(encoded[:, :, :3])
    if pixels.shape[2] == 4:
        a = encoded[:, :, 3:]
        rgb = rgb * a + artkit.srgb_decode(checker(*pixels.shape[:2])) * (1.0 - a)
    return artkit.srgb_encode(artkit.downsample(rgb, factor))


def hero_names(art):
    names = sorted(f[:-4] for f in os.listdir(art) if f.startswith('hero-') and f.endswith('.png')
                   and not f.endswith('-half.png'))

    def order(name):
        group = ('ambient', 'palette', 'hdmi', 'live-tv', 'plex', 'homebrew', 'settings')
        rank = next((i for i, g in enumerate(group) if name.startswith('hero-' + g)), len(group))
        return rank, name

    return sorted(names, key=order)


def icons(art):
    return sorted(f[:-4] for f in os.listdir(art) if f.startswith('icon-') and f.endswith('.png'))


def compose(art):
    """The sheet as encoded floats, and the labels: [(x, y, text)]."""
    names, marks = hero_names(art), icons(art)
    rows = -(-len(names) // COLUMNS)
    width = COLUMNS * CELL_W + (COLUMNS + 1) * GAP
    icon_row = ICON + LABEL + GAP if marks else 0
    height = GAP + rows * (CELL_H + LABEL + GAP) + icon_row
    sheet = np.ones((height, width, 3)) * np.array(SHEET) / 255.0
    labels = []
    for i, name in enumerate(names):
        with open(os.path.join(art, name + '.png'), 'rb') as f:
            pixels = artkit.read_png(f.read())
        cell = shrink(pixels, pixels.shape[1] // CELL_W)
        x = GAP + (i % COLUMNS) * (CELL_W + GAP)
        y = GAP + (i // COLUMNS) * (CELL_H + LABEL + GAP)
        sheet[y:y + CELL_H, x:x + CELL_W] = cell[:CELL_H, :CELL_W]
        labels.append((x, y + CELL_H + 10, name))
    y = GAP + rows * (CELL_H + LABEL + GAP)
    step = (width - 2 * GAP) // max(len(marks), 1)
    for i, name in enumerate(marks):
        with open(os.path.join(art, name + '.png'), 'rb') as f:
            pixels = artkit.read_png(f.read())
        factor = pixels.shape[0] // ICON
        a = artkit.downsample(pixels[:, :, 3:] / 255.0, factor)
        rgb = artkit.srgb_decode(artkit.downsample(pixels[:, :, :3] / 255.0, factor))
        x = GAP + i * step + (step - ICON) // 2
        under = artkit.srgb_decode(sheet[y:y + ICON, x:x + ICON])
        sheet[y:y + ICON, x:x + ICON] = artkit.srgb_encode(rgb * a + under * (1.0 - a))
        labels.append((x, y + ICON + 10, name))
    return sheet, labels


def ambient_choice(frames=(0, 180, 360, 540)):
    """The ambient hero drawn from each studio scene at a few frames of its
    30 s loop, for choosing which the hero comes from. Returns the sheet and
    its labels, like compose()."""
    import heroes  # the studio's scenes come with it
    spec = heroes.load_table()['hero-ambient']
    sources = sorted(heroes.SOURCES)
    width = COLUMNS * CELL_W + (COLUMNS + 1) * GAP
    height = GAP + len(sources) * (CELL_H + LABEL + GAP)
    sheet = np.ones((height, width, 3)) * np.array(SHEET) / 255.0
    labels = []
    for row, source in enumerate(sources):
        for col, frame in enumerate(frames[:COLUMNS]):
            choice = dict(spec, source=source, frame=frame)
            if source != spec['source']:
                del choice['seed']  # the scene's own
            picture = heroes.ambient(choice, artkit.WIDTH, artkit.HEIGHT)['']
            cell = artkit.srgb_encode(artkit.downsample(artkit.srgb_decode(picture), artkit.WIDTH // CELL_W))
            x = GAP + col * (CELL_W + GAP)
            y = GAP + row * (CELL_H + LABEL + GAP)
            sheet[y:y + CELL_H, x:x + CELL_W] = cell
            labels.append((x, y + CELL_H + 10, f'{source}, frame {frame} of {round(spec["seconds"] * spec["fps"])}'))
    return sheet, labels


def crop(name, window=(120, 240), zoom=4, boost=8):
    """A 400% crop of a hero where its darkest gradient bands worst without
    dither: the 8 bits rounded (left) and dithered (right), as they are
    (top) and with the steps between codes made `boost` times larger
    (bottom), so a one-code step shows on any screen. Returns the sheet and
    its labels, like compose()."""
    import heroes
    spec = heroes.load_table()[name]
    encoded = heroes.render(name, spec)['']
    plain, dithered = artkit.quantize(encoded, dither=False), artkit.quantize(encoded)
    found = max((artkit.longest_band_at(encoded[:, :, c] * 255.0, plain[:, :, c]) for c in range(3)),
                key=lambda b: b[0])
    length, row, col, axis = found
    h, w = window
    if row is None:
        row, col = encoded.shape[0] - h, 0
    elif axis == 0:
        row += length // 2
    else:
        col += length // 2
    top = int(np.clip(row - h // 2, 0, encoded.shape[0] - h))
    left = int(np.clip(col - w // 2, 0, encoded.shape[1] - w))
    cells = []
    for pixels in (plain, dithered):
        part = pixels[top:top + h, left:left + w].astype(np.float64)
        stretched = np.clip((part - part.min()) * boost, 0, 255)
        cells.append([np.kron(p, np.ones((zoom, zoom, 1))) / 255.0 for p in (part, stretched)])
    ch, cw = h * zoom, w * zoom
    sheet = np.ones((GAP * 3 + 2 * (ch + LABEL), GAP * 3 + 2 * cw, 3)) * np.array(SHEET) / 255.0
    labels = []
    for i, (title, (as_is, stretched)) in enumerate(zip(('rounded', 'dithered'), cells)):
        x = GAP + i * (cw + GAP)
        for j, (image, how) in enumerate(((as_is, 'as it is'), (stretched, f'steps x{boost}'))):
            y = GAP + j * (ch + LABEL + GAP)
            sheet[y:y + ch, x:x + cw] = image
            labels.append((x, y + ch + 10, f'{name}, {title}, {how}: {w} x {h} px at ({left}, {top}), 400%'))
    return sheet, labels


def draw_labels(png_in, png_out, labels):
    """Writes the names onto the sheet with ffmpeg's drawtext, in Inter."""
    filters = ','.join(f"drawtext=fontfile='{FONT}':text='{text}':x={x}:y={y}:fontsize=20:fontcolor=0x8C94A5"
                       for x, y, text in labels)
    subprocess.run(['ffmpeg', '-v', 'error', '-y', '-i', png_in, '-vf', filters, '-frames:v', '1', png_out],
                   check=True)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--art', default=ART, help='where the art is (default shell/assets/art)')
    parser.add_argument('--out', help='the sheet (default tools/out/art/contact-sheet.png, '
                        'or ambient-choice.png with --ambient-choice)')
    parser.add_argument('--ambient-choice', action='store_true',
                        help='instead, the ambient hero from each studio scene at four frames')
    parser.add_argument('--crop', metavar='HERO',
                        help='instead, a 400%% crop of HERO where it bands worst, rounded and dithered')
    args = parser.parse_args(sys.argv[1:] if argv is None else argv)
    if args.crop:
        args.out = args.out or os.path.join(os.path.dirname(OUT), f'crop-{args.crop}.png')
        sheet, labels = crop(args.crop)
    elif args.ambient_choice:
        args.out = args.out or os.path.join(os.path.dirname(OUT), 'ambient-choice.png')
        sheet, labels = ambient_choice()
    else:
        args.out = args.out or OUT
        sheet, labels = compose(args.art)
    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        plain = os.path.join(tmp, 'sheet.png')
        artkit.write_png(plain, artkit.quantize(sheet))
        draw_labels(plain, args.out, labels)
    print('wrote', args.out)


if __name__ == '__main__':
    main()
