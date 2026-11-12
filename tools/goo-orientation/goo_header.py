"""Read the GOO V3 header fields karasu-slicer's writer lays out."""
import struct, sys
def text(b, at, n):
    return b[at:at+n].split(b'\0')[0].decode('latin-1')
def header(path):
    with open(path, 'rb') as f:
        b = f.read(195_477)
    assert b[:4] == b'V3.0', b[:12]
    at = 12
    fields = {}
    for name, n in [('software', 32), ('version', 24), ('time', 24), ('printer', 32), ('type', 32), ('profile', 32)]:
        fields[name] = text(b, at, n); at += n
    fields['aa'], fields['grey'], fields['blur'] = struct.unpack_from('>HHH', b, at); at += 6
    at += 116*116*2 + 2 + 290*290*2 + 2
    fields['layers'], fields['res_x'], fields['res_y'] = struct.unpack_from('>IHH', b, at); at += 8
    fields['mirror_x'], fields['mirror_y'] = b[at], b[at+1]; at += 2
    fields['x_mm'], fields['y_mm'], fields['z_mm'], fields['layer_mm'] = struct.unpack_from('>ffff', b, at)
    return fields
for p in sys.argv[1:]:
    h = header(p)
    print(p.split('/')[-2] if '/' in p else '', p.split('/')[-1])
    print('  ', {k: h[k] for k in ['software','version','printer','type','aa','layers','res_x','res_y','mirror_x','mirror_y','x_mm','y_mm','z_mm','layer_mm']})
