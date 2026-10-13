# Contact sheets: python3 sheet.py <dir> -> sheet_N.png, 4 frames per sheet with timestamps.
import glob, sys, os
from PIL import Image, ImageDraw, ImageFont
d = sys.argv[1]
fs = sorted(f for f in glob.glob(os.path.join(d, 't*.png')))
font = ImageFont.truetype('/usr/share/fonts/opentype/inter/Inter-SemiBold.otf', 28)
for n in range(0, len(fs), 4):
    sheet = Image.new('RGB', (1920, 1080), (40, 40, 40))
    for i, f in enumerate(fs[n:n + 4]):
        im = Image.open(f).convert('RGB').resize((956, 536))
        x, y = (i % 2) * 964, (i // 2) * 544
        sheet.paste(im, (x, y))
        ImageDraw.Draw(sheet).text((x + 12, y + 8), os.path.basename(f)[1:-4] + 's', fill=(255, 220, 0), font=font)
    sheet.save(os.path.join(d, f'sheet_{n // 4:02d}.png'))
