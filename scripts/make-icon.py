"""產生 assets/icon.png（256px）與 assets/icon.ico（16/24/32/48/64/256，PNG 內嵌）。只用標準函式庫。"""
import struct, zlib, os

def shape(x, y):
    """座標 0..1，回傳 RGBA"""
    def rrect(x0, y0, x1, y1, r):
        if not (x0 <= x <= x1 and y0 <= y <= y1):
            return False
        cx = min(max(x, x0 + r), x1 - r); cy = min(max(y, y0 + r), y1 - r)
        return (x - cx) ** 2 + (y - cy) ** 2 <= r * r
    # 右側藍色 Word 頁（在後）
    if rrect(0.40, 0.18, 0.92, 0.88, 0.06):
        # 頁面上的白色文字橫線
        for ly in (0.36, 0.48, 0.60, 0.72):
            if 0.58 <= x <= 0.84 and ly <= y <= ly + 0.05:
                return (255, 255, 255, 255)
        return (43, 87, 154, 255)
    # 左側紅色 PDF 頁（在前，帶折角）
    if rrect(0.08, 0.10, 0.52, 0.76, 0.05):
        if y < 0.22 and (x - 0.40) > (y - 0.10):  # 右上折角
            return (0, 0, 0, 0)
        for ly in (0.30, 0.42, 0.54):
            if 0.16 <= x <= 0.44 and ly <= y <= ly + 0.05:
                return (255, 255, 255, 255)
        return (214, 48, 49, 255)
    # 其餘透明
    return (0, 0, 0, 0)

def render(size, ss=4):
    rows = []
    for py in range(size):
        row = bytearray()
        for px in range(size):
            acc = [0, 0, 0, 0]
            for sy in range(ss):
                for sx in range(ss):
                    c = shape((px + (sx + 0.5) / ss) / size, (py + (sy + 0.5) / ss) / size)
                    a = c[3]
                    acc[0] += c[0] * a; acc[1] += c[1] * a; acc[2] += c[2] * a; acc[3] += a
            n = ss * ss
            a = acc[3]
            row += bytes([acc[0] // a if a else 0, acc[1] // a if a else 0, acc[2] // a if a else 0, a // n])
        rows.append(bytes(row))
    raw = b''.join(b'\x00' + r for r in rows)
    def chunk(t, d):
        return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b''))

root = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'assets')
sizes = [16, 24, 32, 48, 64, 256]
pngs = {s: render(s, 8 if s <= 64 else 3) for s in sizes}
open(os.path.join(root, 'icon.png'), 'wb').write(pngs[256])
header = struct.pack('<HHH', 0, 1, len(sizes))
offset = 6 + 16 * len(sizes)
entries = b''; data = b''
for s in sizes:
    d = pngs[s]
    entries += struct.pack('<BBBBHHII', s % 256, s % 256, 0, 0, 1, 32, len(d), offset + len(data))
    data += d
open(os.path.join(root, 'icon.ico'), 'wb').write(header + entries + data)
print('ok')
