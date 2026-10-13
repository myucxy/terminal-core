"""Generate project-owned PNG oracles with Python's standard zlib (no external images).

Run with the pinned inventory Python. Committed vectors are consumed offline by Rust.
PNG encoding follows ISO/IEC 15948; expected RGBA is derived directly from source samples.
"""
from pathlib import Path
import struct
import zlib

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'crates/termlink-terminal-core/tests/fixtures/graphics-png.txt'
PASSES = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4),
          (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)]


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def pack(samples, depth):
    if depth == 16:
        return struct.pack('>' + 'H' * len(samples), *samples)
    if depth == 8:
        return bytes(samples)
    result = bytearray((len(samples) * depth + 7) // 8)
    for i, value in enumerate(samples):
        result[i * depth // 8] |= value << (8 - depth - i * depth % 8)
    return bytes(result)


def paeth(a, b, c):
    p = a + b - c
    return min((a, b, c), key=lambda v: abs(p - v))


def vector(color, depth, interlace, width=9, height=11):
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[color]
    max_sample = (1 << depth) - 1
    scale = lambda v: v >> 8 if depth == 16 else v * 255 // max_sample
    samples = [[[(x * 19 + y * 43 + c * 61) * (257 if depth == 16 else 1) & max_sample
                 for c in range(channels)] for x in range(width)] for y in range(height)]
    palette = bytes(c for i in range(min(256, max_sample + 1))
                    for c in ((i * 31) % 256, (i * 53) % 256, (i * 71) % 256))
    alpha = bytes([0, 111])
    transparent = samples[0][0]
    expected = bytearray()
    for row in samples:
        for pixel in row:
            if color in (0, 4):
                rgba = [scale(pixel[0])] * 3 + [scale(pixel[1]) if color == 4 else (0 if pixel == transparent else 255)]
            elif color in (2, 6):
                rgba = list(map(scale, pixel[:3])) + [scale(pixel[3]) if color == 6 else (0 if pixel == transparent else 255)]
            else:
                i = pixel[0]
                rgba = list(palette[3*i:3*i+3]) + [alpha[i] if i < len(alpha) else 255]
            expected.extend(rgba)
    raw = bytearray()
    for x0, y0, dx, dy in PASSES if interlace else [(0, 0, 1, 1)]:
        xs, ys = list(range(x0, width, dx)), list(range(y0, height, dy))
        if not xs or not ys:
            continue
        prev = bytes((len(xs) * channels * depth + 7) // 8)
        bpp = max(1, (channels * depth + 7) // 8)
        for n, y in enumerate(ys):
            row = pack([c for x in xs for c in samples[y][x]], depth)
            f = n % 5
            raw.append(f)
            for i, value in enumerate(row):
                a, b, c = row[i-bpp] if i >= bpp else 0, prev[i], prev[i-bpp] if i >= bpp else 0
                prediction = (0, a, b, (a+b)//2, paeth(a, b, c))[f]
                raw.append((value - prediction) % 256)
            prev = row
    png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, depth, color, 0, 0, interlace))
    if color == 3:
        png += chunk(b'PLTE', palette) + chunk(b'tRNS', alpha)
    elif color in (0, 2):
        png += chunk(b'tRNS', struct.pack('>' + 'H' * channels, *transparent))
    # Exercise IDAT reconstruction and checksum locations across chunk boundaries.
    encoded = zlib.compress(raw)
    png += b''.join(chunk(b'IDAT', encoded[i:i+7]) for i in range(0, len(encoded), 7))
    png += chunk(b'IEND', b'')
    return f'c{color}-d{depth}-i{interlace}-{width}x{height}|{width}|{height}|{png.hex()}|{expected.hex()}'


def main():
    lines = ['# Self-generated PNG + source RGBA; ISO/IEC 15948; regenerate scripts/Generate-GraphicsFixtures.py']
    for color, depths in [(0, (1, 2, 4, 8, 16)), (2, (8, 16)), (3, (1, 2, 4, 8)), (4, (8, 16)), (6, (8, 16))]:
        for depth in depths:
            for interlace in (0, 1):
                lines.append(vector(color, depth, interlace))
    for width, height in [(1, 1), (1, 9), (9, 1), (2, 2), (3, 5)]:
        lines.append(vector(6, 16, 1, width, height))
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text('\n'.join(lines) + '\n', encoding='utf-8')
    print(f'{len(lines)-1} PNG oracles: {OUT}')


if __name__ == '__main__':
    main()
